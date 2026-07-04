use std::{io::ErrorKind, os::fd::AsRawFd, sync::mpsc, time::Duration};

use anyhow::{Context, Result};
use tracing::info;
use wayland_backend::client::WaylandError;
use wayland_client::{
    globals::registry_queue_init,
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
use we_renderer::{Frame, RenderConfig, RendererLibrary, Source};

use crate::{
    config::Config,
    ipc::{ControlCommand, RuntimeLoopExit},
};

use super::{
    diagnostics::{OptionsJsonDiagnostics, PresentBackend, RuntimeDiagnostics},
    state::{BufferBookkeeping, FrameCallbackState, LayerState, OutputState, WaylandObjects},
};
use super::wayland;

fn env_var_enabled(name: &str) -> bool {
    std::env::var(name).map(|v| !v.is_empty() && v != "0").unwrap_or(false)
}

fn env_var_equals(name: &str, expected: &str) -> bool {
    std::env::var(name).map(|v| v == expected).unwrap_or(false)
}

fn note_frame_acquired(state: &mut LayerState, frame: &Frame) {
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

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

pub fn run_renderer_background_surface(
    cfg: &Config,
    control_rx: &mpsc::Receiver<ControlCommand>,
) -> Result<RuntimeLoopExit> {
    let conn = Connection::connect_to_env().context("failed to connect to Wayland display")?;
    let (globals, mut event_queue) =
        registry_queue_init::<LayerState>(&conn).context("failed to init Wayland registry")?;
    let qh = event_queue.handle();

    let compositor: WlCompositor =
        globals.bind(&qh, 4..=6, ()).context("failed to bind wl_compositor")?;
    let layer_shell: ZwlrLayerShellV1 =
        globals.bind(&qh, 1..=5, ()).context("failed to bind zwlr_layer_shell_v1")?;
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
    let library_path = resolve_renderer_library(&cfg.renderer.library_path)?;
    let library = RendererLibrary::load(&library_path)
        .with_context(|| format!("failed to load renderer library {}", library_path.display()))?;

    let cache_path_arg =
        if cfg.renderer.cache_path.trim().is_empty() { None } else { Some(cache_path.as_path()) };

    let mut session =
        library.create_session(cache_path_arg).context("failed to create renderer session")?;

    let mut state = LayerState {
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
                present: false,
                len: 0,
                valid: true,
            },
            ..Default::default()
        },
        dmabuf_version: 0,
        compositor_version: 0,
        output_count: 0,
        running: true,
        configured: false,
        session: None,
        _library: Some(library),
        interactive: cfg.general.interactive,
        paused: false,
        pending_input_events: Vec::new(),
    };

    wayland::init_wayland(
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
        event_queue.roundtrip(&mut state).context("failed waiting for layer configure")?;
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
        options_json: None,
    };
    session.set_source(&source).context("failed to set renderer source")?;

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
    session
        .configure(RenderConfig {
            width: state.output.geometry.render_width,
            height: state.output.geometry.render_height,
            enable_valid_layer: false,
            prefer_dmabuf,
            allow_shm_fallback: cfg.renderer.allow_shm_fallback,
        })
        .context("failed to set render config")?;

    session.play().context("failed to start renderer session")?;
    state.session = Some(session);

    info!(
        logical_width = state.output.logical_width,
        logical_height = state.output.logical_height,
        render_width = state.output.geometry.render_width,
        render_height = state.output.geometry.render_height,
        scale = state.output.render_scale_factor(),
        "starting renderer-backed layer-shell surface"
    );

    let mut last_acquire_status: i32 = 1;
    let mut last_log = std::time::Instant::now();

    // Main loop
    loop {
        // Handle control commands
        while let Ok(cmd) = control_rx.try_recv() {
            match cmd {
                ControlCommand::Stop => {
                    if let Some(ref mut session) = state.session {
                        session.stop().ok();
                    }
                    return Ok(RuntimeLoopExit::Stop);
                }
                ControlCommand::Pause => {
                    if let Some(ref mut session) = state.session {
                        session.pause().ok();
                    }
                    state.paused = true;
                }
                ControlCommand::Resume => {
                    if let Some(ref mut session) = state.session {
                        session.play().ok();
                    }
                    state.paused = false;
                    state.frame_callback.ready_for_next_frame = true;
                }
                ControlCommand::Reload => {
                    if let Some(ref mut session) = state.session {
                        session.stop().ok();
                    }
                    return Ok(RuntimeLoopExit::RestartCurrent);
                }
                ControlCommand::Reconfigure => {
                    if let Some(ref mut session) = state.session {
                        session.stop().ok();
                    }
                    return Ok(RuntimeLoopExit::Reconfigure);
                }
            }
        }

        // Forward input events
        let input_events = std::mem::take(&mut state.pending_input_events);
        if let Some(ref mut session) = state.session {
            for event in input_events {
                session.send_input_event(event).ok();
            }
        }

        // Tick and acquire frame
        if !state.paused {
            if let Some(ref mut session) = state.session {
                session.tick().context("renderer tick failed")?;
                match session.acquire_frame().context("failed to acquire frame")? {
                    Some(frame) => {
                        note_frame_acquired(&mut state, &frame);
                        wayland::present_frame(&mut state, &qh, frame)
                            .context("failed to present frame")?;
                        last_acquire_status = 0;
                    }
                    None => {
                        state.frame_stats.no_frame_polls =
                            state.frame_stats.no_frame_polls.saturating_add(1);
                        last_acquire_status = 1;
                    }
                }
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

        state.collect_released_buffers();

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
                last_acquire_status = status_text,
                last_acquire_status_code = last_acquire_status,
                "renderer stats"
            );
        }

        // Poll and dispatch
        event_queue
            .dispatch_pending(&mut state)
            .context("failed to dispatch pending Wayland events")?;

        let Some(read_guard) = event_queue.prepare_read() else {
            state.collect_released_buffers();
            if !state.running {
                if let Some(ref mut session) = state.session {
                    session.stop().ok();
                }
                return Ok(RuntimeLoopExit::Stop);
            }
            continue;
        };

        let fd = read_guard.connection_fd().as_raw_fd();
        let mut poll_fd = libc::pollfd {
            fd,
            events: libc::POLLIN | if flush_blocked { libc::POLLOUT } else { 0 },
            revents: 0,
        };
        let poll_result = unsafe {
            libc::poll(&mut poll_fd, 1, 5 /* ms */)
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
            continue;
        }
        if (poll_fd.revents & (libc::POLLERR | libc::POLLHUP)) != 0 {
            state.running = false;
            drop(read_guard);
            continue;
        }
        if (poll_fd.revents & libc::POLLIN) != 0 {
            read_guard.read().context("failed to read Wayland events")?;
            event_queue
                .dispatch_pending(&mut state)
                .context("failed to dispatch Wayland events after read")?;
            wayland::update_input_region(&state, &qh);
        } else {
            drop(read_guard);
        }
        if flush_blocked && (poll_fd.revents & libc::POLLOUT) != 0 {
            match event_queue.flush() {
                Ok(()) => state.release_pending_send_fds(),
                Err(WaylandError::Io(err)) if err.kind() == ErrorKind::WouldBlock => {}
                Err(err) => return Err(err).context("failed to flush Wayland fd after POLLOUT"),
            }
        }

        state.collect_released_buffers();

        if !state.running {
            if let Some(ref mut session) = state.session {
                session.stop().ok();
            }
            return Ok(RuntimeLoopExit::Stop);
        }
    }
}
