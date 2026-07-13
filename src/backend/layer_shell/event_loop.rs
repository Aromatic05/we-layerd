use std::{
    io::ErrorKind,
    os::fd::AsRawFd,
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use tracing::info;
use wayland_backend::client::WaylandError;
use wayland_client::{
    protocol::{wl_compositor::WlCompositor, wl_seat::WlSeat, wl_shm::WlShm},
    Connection,
};
use wayland_protocols::wp::{
    fractional_scale::v1::client::wp_fractional_scale_manager_v1::WpFractionalScaleManagerV1,
    linux_dmabuf::zv1::client::zwp_linux_dmabuf_v1::ZwpLinuxDmabufV1,
    viewporter::client::wp_viewporter::WpViewporter,
};
use wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_shell_v1::ZwlrLayerShellV1;
use we_core::install_layout::{expand_tilde, resolve_renderer_library};
use we_renderer::{FillMode, Frame, RenderConfig, RuntimeSettings, Source};

use crate::{
    backend::{
        layer_shell::{
            presenter,
            state::{BufferBookkeeping, FrameCallbackState, LayerShellState, WaylandObjects},
            surface,
        },
        traits::BackendContext,
        wayland_common::{connection, output::OutputState, registry},
    },
    ipc::{ControlCommand, RuntimeLoopExit},
    runtime::{
        renderer_session::RendererSession,
        status::{OptionsJsonDiagnostics, PresentBackend, RuntimeDiagnostics},
    },
};

fn env_var_enabled(name: &str) -> bool {
    std::env::var(name).map(|v| !v.is_empty() && v != "0").unwrap_or(false)
}

fn env_var_equals(name: &str, expected: &str) -> bool {
    std::env::var(name).map(|v| v == expected).unwrap_or(false)
}

fn note_frame_acquired(state: &mut LayerShellState, frame: &Frame) {
    state.frame_stats.acquired = state.frame_stats.acquired.saturating_add(1);
    match frame {
        Frame::Dmabuf(dmabuf) => {
            state.frame_stats.last_present_backend = Some(PresentBackend::Dmabuf);
            state.frame_stats.last_frame_width = dmabuf.width;
            state.frame_stats.last_frame_height = dmabuf.height;
        }
        Frame::Shm(shm) => {
            state.frame_stats.last_present_backend = Some(PresentBackend::Shm);
            state.frame_stats.last_frame_width = shm.width;
            state.frame_stats.last_frame_height = shm.height;
        }
    }
}

fn frame_interval(fps: u32) -> Duration {
    Duration::from_secs_f64(1.0 / fps.max(1) as f64)
}

fn renderer_fill_mode(value: we_core::wallpaper::settings::WallpaperFillMode) -> FillMode {
    match value {
        we_core::wallpaper::settings::WallpaperFillMode::Cover => FillMode::Cover,
        we_core::wallpaper::settings::WallpaperFillMode::Fit => FillMode::Fit,
        we_core::wallpaper::settings::WallpaperFillMode::Stretch => FillMode::Stretch,
        we_core::wallpaper::settings::WallpaperFillMode::Center => FillMode::Center,
    }
}

fn can_present_next_frame(state: &LayerShellState) -> bool {
    !state.paused
        && !state.stopping
        && state.frame_callback.ready_for_next_frame
        && !state.frame_callback.pending
        && state.buffers.in_flight.len() < state.buffers.max_in_flight
}

fn poll_timeout(now: Instant, next_tick_at: Instant, state: &LayerShellState) -> i32 {
    if can_present_next_frame(state) {
        return next_tick_at
            .checked_duration_since(now)
            .unwrap_or_default()
            .min(Duration::from_millis(100))
            .as_millis() as i32;
    }

    100
}

fn flush_wayland_stop(conn: &Connection, state: &mut LayerShellState) -> Result<()> {
    info!("flush begin");
    let deadline = Instant::now() + Duration::from_millis(500);
    loop {
        match conn.flush() {
            Ok(()) => {
                state.release_pending_send_fds();
                info!("flush end");
                return Ok(());
            }
            Err(WaylandError::Io(err)) if err.kind() == ErrorKind::WouldBlock => {
                let Some(read_guard) = conn.prepare_read() else {
                    continue;
                };
                let fd = read_guard.connection_fd().as_raw_fd();
                let mut poll_fd = libc::pollfd { fd, events: libc::POLLOUT, revents: 0 };
                let timeout_ms = deadline
                    .checked_duration_since(Instant::now())
                    .unwrap_or_default()
                    .as_millis()
                    .min(i32::MAX as u128) as i32;
                if timeout_ms == 0 {
                    drop(read_guard);
                    return Err(anyhow::anyhow!("timed out flushing Wayland stop detach"));
                }
                let poll_result = unsafe { libc::poll(&mut poll_fd, 1, timeout_ms) };
                drop(read_guard);
                if poll_result < 0 {
                    let poll_err = std::io::Error::last_os_error();
                    if poll_err.kind() == ErrorKind::Interrupted {
                        continue;
                    }
                    return Err(poll_err).context("failed polling Wayland fd during stop flush");
                }
            }
            Err(err) => return Err(err).context("failed to flush Wayland stop detach"),
        }
    }
}

fn stop_and_drop_session(state: &mut LayerShellState) {
    let Some(session) = state.session.take() else {
        return;
    };

    info!("session.stop begin");
    session.stop();
    info!("session.stop end");
}

fn exit_runtime_loop(
    conn: &Connection,
    state: &mut LayerShellState,
    exit: RuntimeLoopExit,
    label: &'static str,
) -> Result<RuntimeLoopExit> {
    info!("{label} received");
    info!("detach surface begin");
    surface::begin_stop_teardown(state).context("failed to detach Wayland surface")?;
    info!("detach surface end");

    flush_wayland_stop(conn, state)?;
    state.clear_in_flight_buffers();
    stop_and_drop_session(state);

    match exit {
        RuntimeLoopExit::Stop => info!("returning RuntimeLoopExit::Stop"),
        RuntimeLoopExit::RestartCurrent => info!("returning RuntimeLoopExit::RestartCurrent"),
        RuntimeLoopExit::Reconfigure => info!("returning RuntimeLoopExit::Reconfigure"),
    }
    Ok(exit)
}

pub(crate) fn run(ctx: BackendContext<'_>) -> Result<RuntimeLoopExit> {
    if ctx.shutdown_requested.load(std::sync::atomic::Ordering::Relaxed) {
        return Ok(RuntimeLoopExit::Stop);
    }

    let cfg = ctx.cfg;
    let control_rx = ctx.control_rx;
    let status_sink = ctx.status_sink;

    let conn = connection::connect_to_env()?;
    let (globals, mut event_queue) = registry::init_registry::<LayerShellState>(&conn)?;
    let qh = event_queue.handle();

    let compositor: WlCompositor =
        globals.bind(&qh, 4..=6, ()).context("failed to bind wl_compositor")?;
    let layer_shell = globals.bind::<ZwlrLayerShellV1, _, _>(&qh, 1..=5, ()).ok();
    let shm: WlShm = globals.bind(&qh, 1..=1, ()).context("failed to bind wl_shm")?;

    let dmabuf_global =
        globals.contents().clone_list().into_iter().find(|g| g.interface == "zwp_linux_dmabuf_v1");
    let dmabuf_version = dmabuf_global.as_ref().map(|g| g.version).unwrap_or(0);
    let dmabuf = globals.bind::<ZwpLinuxDmabufV1, _, _>(&qh, 3..=4, ()).ok();

    let seat = globals.bind::<WlSeat, _, _>(&qh, 1..=5, ()).ok();
    let viewporter = globals.bind::<WpViewporter, _, _>(&qh, 1..=1, ()).ok();
    let fractional_scale_manager =
        globals.bind::<WpFractionalScaleManagerV1, _, _>(&qh, 1..=1, ()).ok();

    let cache_path = expand_tilde(&cfg.renderer.cache_path);
    let source_path = expand_tilde(&cfg.renderer.source);
    let assets_path = expand_tilde(&cfg.renderer.assets_path);

    if let Some(install_root) = option_env!("WE_LAYERD_RENDERER_INSTALL_ROOT") {
        std::env::set_var("WE_LAYERD_RENDERER_INSTALL_ROOT", install_root);
    }
    let cache_path_arg =
        if cfg.renderer.cache_path.trim().is_empty() { None } else { Some(cache_path.as_path()) };

    let library_path = resolve_renderer_library(&cfg.renderer.library_path)?;
    let mut session = RendererSession::create(&library_path, cache_path_arg)?;

    let (options_json_present, options_json_len, options_json_valid) =
        cfg.renderer.options_json_diagnostics();

    let mut state = LayerShellState {
        objects: WaylandObjects::default(),
        output: OutputState::new(cfg.general.scale_mode),
        buffers: BufferBookkeeping::default(),
        frame_callback: FrameCallbackState {
            pending: false,
            ready_for_next_frame: true,
            last_done_msec: None,
        },
        frame_stats: Default::default(),
        diagnostics: RuntimeDiagnostics {
            prefer_dmabuf_configured: cfg.renderer.prefer_dmabuf,
            prefer_dmabuf_effective: cfg.renderer.prefer_dmabuf,
            allow_shm_fallback: cfg.renderer.allow_shm_fallback,
            options_json: OptionsJsonDiagnostics {
                present: options_json_present,
                len: options_json_len,
                valid: options_json_valid,
            },
            ..Default::default()
        },
        dmabuf_version: 0,
        compositor_version: 0,
        output_count: 0,
        running: true,
        configured: false,
        session: None,
        interactive: cfg.general.interactive,
        render_resolution_follows_output: cfg.renderer.render_width.is_none()
            && cfg.renderer.render_height.is_none(),
        paused: false,
        stopping: false,
        pending_input_events: crate::runtime::input::PendingInput::default(),
    };
    state.output.render_size_override = match (cfg.renderer.render_width, cfg.renderer.render_height) {
        (Some(width), Some(height)) => Some((width.max(1), height.max(1))),
        _ => None,
    };

    surface::init_wayland(
        &conn,
        &qh,
        &mut state,
        &globals,
        compositor,
        layer_shell,
        shm,
        dmabuf,
        dmabuf_version,
        seat,
        viewporter,
        fractional_scale_manager,
    )?;
    status_sink(state.snapshot());

    // Set input region and commit initial surface state
    {
        if let (Some(ref compositor), Some(ref surface)) =
            (&state.objects.compositor, &state.objects.surface)
        {
            let region = compositor.create_region(&qh, ());
            if state.interactive {
                region.add(
                    0,
                    0,
                    state.output.logical_width as i32,
                    state.output.logical_height as i32,
                );
            }
            surface.set_input_region(Some(&region));
            region.destroy();
        }
        if let Some(ref surface) = &state.objects.surface {
            surface.commit();
        }
    }

    // Wait for the first configure
    while !state.configured {
        event_queue.roundtrip(&mut state).context("failed waiting for surface configure")?;
        state.update_render_extent();
        state.update_viewport_destination();
    }

    if state.output.logical_width == 0 {
        state.output.logical_width = state.output.fallback_width;
    }
    if state.output.logical_height == 0 {
        state.output.logical_height = state.output.fallback_height;
    }
    state.update_render_extent();
    state.update_viewport_destination();

    // Set source
    let source = Source {
        uri: source_path.display().to_string(),
        assets_uri: assets_path.display().to_string(),
        fps: cfg.renderer.fps as i32,
        speed: cfg.renderer.speed,
        volume: cfg.renderer.volume,
        muted: cfg.renderer.muted,
        options_json: cfg.renderer.options_json.clone(),
    };
    session.set_source(source)?;

    // Determine dmabuf preference
    let prefer_dmabuf = if env_var_enabled("__NV_PRIME_RENDER_OFFLOAD")
        || env_var_equals("__VK_LAYER_NV_optimus", "NVIDIA_only")
    {
        info!("NVIDIA prime-render-offload detected; forcing SHM fallback");
        state.diagnostics.nvidia_prime_offload_detected = true;
        state.diagnostics.prefer_dmabuf_effective = false;
        false
    } else {
        cfg.renderer.prefer_dmabuf
    };

    // Set render config BEFORE play
    let render_width = cfg.renderer.render_width.unwrap_or(state.output.geometry.render_width);
    let render_height = cfg.renderer.render_height.unwrap_or(state.output.geometry.render_height);
    session.configure(RenderConfig {
        width: render_width,
        height: render_height,
        enable_valid_layer: false,
        prefer_dmabuf,
        allow_shm_fallback: cfg.renderer.allow_shm_fallback,
        msaa_samples: 0,
        fill_mode: renderer_fill_mode(cfg.renderer.fill_mode),
        rotation_degrees: cfg.renderer.rotation_degrees,
    })?;
    session.apply_runtime_settings(RuntimeSettings {
        fps: Some(cfg.renderer.fps as i32),
        speed: Some(cfg.renderer.speed),
        volume: Some(cfg.renderer.volume),
        muted: Some(cfg.renderer.muted),
        fill_mode: Some(renderer_fill_mode(cfg.renderer.fill_mode)),
    })?;

    session.play()?;
    let renderer_frame_fd = session.frame_ready_fd()?;
    state.session = Some(session);
    status_sink(state.snapshot());

    info!(
        logical_width = state.output.logical_width,
        logical_height = state.output.logical_height,
        render_width = state.output.geometry.render_width,
        render_height = state.output.geometry.render_height,
        scale = state.output.render_scale_factor(),
        "starting renderer-backed wayland surface"
    );

    let mut last_acquire_status: i32 = 1;
    let mut last_log = std::time::Instant::now();
    let render_interval = frame_interval(cfg.renderer.fps);
    let mut next_tick_at = Instant::now();

    // Main loop
    loop {
        // Handle control commands
        while let Ok(cmd) = control_rx.try_recv() {
            match cmd {
                ControlCommand::Stop => {
                    let exit = exit_runtime_loop(&conn, &mut state, RuntimeLoopExit::Stop, "Stop")?;
                    status_sink(state.snapshot());
                    return Ok(exit);
                }
                ControlCommand::Pause => {
                    if let Some(ref mut session) = state.session {
                        session.pause();
                    }
                    state.paused = true;
                    status_sink(state.snapshot());
                }
                ControlCommand::Resume => {
                    if let Some(ref mut session) = state.session {
                        session.resume();
                    }
                    state.paused = false;
                    state.frame_callback.ready_for_next_frame = true;
                    status_sink(state.snapshot());
                }
                ControlCommand::Reload => {
                    let exit = exit_runtime_loop(
                        &conn,
                        &mut state,
                        RuntimeLoopExit::RestartCurrent,
                        "Reload",
                    )?;
                    status_sink(state.snapshot());
                    return Ok(exit);
                }
                ControlCommand::Reconfigure => {
                    let exit = exit_runtime_loop(
                        &conn,
                        &mut state,
                        RuntimeLoopExit::Reconfigure,
                        "Reconfigure",
                    )?;
                    status_sink(state.snapshot());
                    return Ok(exit);
                }
            }
        }

        // Forward input events
        let input_events = state.pending_input_events.drain();
        if let Some(ref mut session) = state.session {
            for event in input_events {
                let _ = session.session.send_input_event(event);
            }
        }

        state.collect_released_buffers();
        status_sink(state.snapshot());

        // Tick at the configured rate. The renderer eventfd signals when that work produces a frame.
        let now = Instant::now();
        let blocked_by_backpressure = state.buffers.in_flight.len() >= state.buffers.max_in_flight;
        if blocked_by_backpressure && now >= next_tick_at && !state.paused {
            state.frame_stats.skipped_by_backpressure =
                state.frame_stats.skipped_by_backpressure.saturating_add(1);
        }
        if can_present_next_frame(&state) && now >= next_tick_at {
            if let Some(ref mut session) = state.session {
                session.tick()?;
                next_tick_at = now + render_interval;
            }
        }

        // Flush
        let flush_blocked = match event_queue.flush() {
            Ok(()) => {
                state.release_pending_send_fds();
                false
            }
            Err(WaylandError::Io(err)) if err.kind() == ErrorKind::WouldBlock => true,
            Err(err) => return Err(err).context("failed to flush Wayland connection"),
        };

        // 5-second stats log
        let now = std::time::Instant::now();
        if now.duration_since(last_log) >= Duration::from_secs(5) {
            last_log = now;
            let status_text = match last_acquire_status {
                0 => "ok",
                1 => "no-frame",
                _ => "error",
            };
            info!(
                acquired = state.frame_stats.acquired,
                presented = state.frame_stats.presented,
                no_frame_polls = state.frame_stats.no_frame_polls,
                in_flight = state.frame_stats.in_flight_count,
                last_present_backend = state
                    .frame_stats
                    .last_present_backend
                    .map(|backend| backend.as_str())
                    .unwrap_or("unknown"),
                last_acquire_status = status_text,
                last_acquire_status_code = last_acquire_status,
                "renderer stats"
            );
        }

        // Poll and dispatch
        event_queue
            .dispatch_pending(&mut state)
            .context("failed to dispatch pending Wayland events")?;
        status_sink(state.snapshot());

        let Some(read_guard) = event_queue.prepare_read() else {
            state.collect_released_buffers();
            status_sink(state.snapshot());
            if !state.running {
                let exit = exit_runtime_loop(&conn, &mut state, RuntimeLoopExit::Stop, "Stop")?;
                status_sink(state.snapshot());
                return Ok(exit);
            }
            continue;
        };

        let wayland_fd = read_guard.connection_fd().as_raw_fd();
        let mut poll_fds = [
            libc::pollfd {
                fd: wayland_fd,
                events: libc::POLLIN | if flush_blocked { libc::POLLOUT } else { 0 },
                revents: 0,
            },
            libc::pollfd { fd: renderer_frame_fd, events: libc::POLLIN, revents: 0 },
        ];
        let timeout_ms = poll_timeout(Instant::now(), next_tick_at, &state);
        let poll_result = unsafe {
            libc::poll(poll_fds.as_mut_ptr(), poll_fds.len() as libc::nfds_t, timeout_ms)
        };

        if poll_result < 0 {
            let err = std::io::Error::last_os_error();
            if err.kind() == ErrorKind::Interrupted {
                drop(read_guard);
                continue;
            }
            return Err(err).context("failed to poll Wayland fd");
        }
        if poll_result == 0 {
            drop(read_guard);
            while event_queue.dispatch_pending(&mut state).unwrap_or(0) > 0 {}
            state.collect_released_buffers();
            status_sink(state.snapshot());
            continue;
        }
        if (poll_fds[0].revents & (libc::POLLERR | libc::POLLHUP)) != 0 {
            state.running = false;
            drop(read_guard);
            continue;
        }
        if (poll_fds[1].revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL)) != 0 {
            drop(read_guard);
            return Err(anyhow::anyhow!("renderer frame-ready fd became invalid"));
        }
        if (poll_fds[0].revents & libc::POLLIN) != 0 {
            read_guard.read().context("failed to read Wayland events")?;
            event_queue
                .dispatch_pending(&mut state)
                .context("failed to dispatch Wayland events after read")?;
            surface::update_input_region(&state, &qh);
            status_sink(state.snapshot());
        } else {
            drop(read_guard);
        }
        if flush_blocked && (poll_fds[0].revents & libc::POLLOUT) != 0 {
            match event_queue.flush() {
                Ok(()) => state.release_pending_send_fds(),
                Err(WaylandError::Io(err)) if err.kind() == ErrorKind::WouldBlock => {}
                Err(err) => return Err(err).context("failed to flush Wayland fd after POLLOUT"),
            }
        }

        if (poll_fds[1].revents & libc::POLLIN) != 0 && can_present_next_frame(&state) {
            if let Some(ref mut session) = state.session {
                match session.acquire_frame()? {
                    Some(frame) => {
                        note_frame_acquired(&mut state, &frame);
                        presenter::present_frame(&mut state, &qh, frame)
                            .context("failed to present frame")?;
                        last_acquire_status = 0;
                        status_sink(state.snapshot());
                    }
                    None => {
                        state.frame_stats.no_frame_polls =
                            state.frame_stats.no_frame_polls.saturating_add(1);
                        last_acquire_status = 1;
                    }
                }
            }
        }

        state.collect_released_buffers();
        status_sink(state.snapshot());

        if !state.running {
            let exit = exit_runtime_loop(&conn, &mut state, RuntimeLoopExit::Stop, "Stop")?;
            status_sink(state.snapshot());
            return Ok(exit);
        }
    }
}
