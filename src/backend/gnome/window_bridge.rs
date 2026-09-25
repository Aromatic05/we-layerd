use std::{
    os::fd::{AsRawFd, RawFd},
    sync::atomic::Ordering,
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use tracing::{info, warn};
use we_core::{
    install_layout::{expand_tilde, resolve_renderer_library},
    wallpaper::{wallpaper_type_from_source, WallpaperType},
};
use we_renderer::{FillMode, Frame, RenderConfig, Source};
use x11rb::connection::Connection as _;

use crate::{
    backend::traits::BackendContext,
    ipc::{ControlCommand, RuntimeLoopExit},
    runtime::{
        renderer_session::RendererSession,
        status::{
            FrameStats, OptionsJsonDiagnostics, PresentBackend, PresentationStatus,
            RuntimeDiagnostics, RuntimeStatusSnapshot,
        },
    },
};

use super::x11_window::X11Windows;

const MAX_RUNTIME_IDLE_WAIT: Duration = Duration::from_millis(100);

struct OutputRuntime {
    window_index: usize,
    session: RendererSession,
    frame_fd: RawFd,
    status: RuntimeStatusSnapshot,
    media_generation: u64,
    audio_generation: u64,
    policy_generation: u64,
    policy_paused: bool,
    rule_muted: bool,
    applied_muted: bool,
    manually_paused: bool,
    next_tick: Instant,
}

impl OutputRuntime {
    fn paused(&self) -> bool {
        self.manually_paused || self.policy_paused
    }
}

pub(crate) fn run(ctx: BackendContext<'_>) -> Result<RuntimeLoopExit> {
    let _version = super::dbus::ping_extension(&ctx.cfg.gnome.extension_dbus_name)?;
    let cfg = ctx.cfg;
    if ctx.shutdown_requested.load(Ordering::Relaxed) {
        return Ok(RuntimeLoopExit::Stop);
    }

    let source_path = expand_tilde(&cfg.renderer.source);
    let assets_path = expand_tilde(&cfg.renderer.assets_path);
    let cache_path = expand_tilde(&cfg.renderer.cache_path);
    let library_path = resolve_renderer_library(&cfg.renderer.library_path)?;
    let wallpaper_type = wallpaper_type_from_source(&source_path).unwrap_or(WallpaperType::Unknown);
    if let Some(install_root) = option_env!("WE_LAYERD_RENDERER_INSTALL_ROOT") {
        std::env::set_var("WE_LAYERD_RENDERER_INSTALL_ROOT", install_root);
    }
    let cache_path_arg =
        if cfg.renderer.cache_path.trim().is_empty() { None } else { Some(cache_path.as_path()) };

    let render_size = match (cfg.renderer.render_width, cfg.renderer.render_height) {
        (Some(width), Some(height)) => Some((width.max(1), height.max(1))),
        _ => None,
    };
    let windows = X11Windows::create(render_size)?;
    if windows.windows.is_empty() {
        anyhow::bail!("XWayland did not expose any active GNOME monitors");
    }

    let source = Source {
        uri: source_path.display().to_string(),
        assets_uri: assets_path.display().to_string(),
        fps: cfg.renderer.fps as i32,
        speed: cfg.renderer.speed,
        volume: cfg.renderer.volume,
        muted: cfg.renderer.muted,
        options_json: cfg.renderer.options_json.clone(),
    };
    let (options_json_present, options_json_len, options_json_valid) =
        cfg.renderer.options_json_diagnostics();
    let mut outputs = Vec::with_capacity(windows.windows.len());
    for (window_index, window) in windows.windows.iter().enumerate() {
        let mut session = RendererSession::create(&library_path, cache_path_arg)?;
        session.set_source(source.clone())?;
        // The GNOME bridge presents through XWayland windows, so DMA-BUF frames cannot be
        // attached directly. An empty format set makes the renderer use its SHM frame path.
        session.set_dmabuf_formats(&[])?;
        let (render_width, render_height) =
            render_size.unwrap_or((window.monitor.width, window.monitor.height));
        session.configure(RenderConfig {
            width: render_width.max(1),
            height: render_height.max(1),
            enable_valid_layer: false,
            prefer_dmabuf: false,
            allow_shm_fallback: true,
            msaa_samples: cfg.renderer.msaa_samples.max(1),
            fill_mode: renderer_fill_mode(cfg.renderer.fill_mode),
            rotation_degrees: cfg.renderer.rotation_degrees,
        })?;
        session.play()?;
        let frame_fd = session.frame_ready_fd()?;
        let supports_media =
            matches!(wallpaper_type, WallpaperType::Scene) && session.supports_media_state();
        let supports_audio = matches!(wallpaper_type, WallpaperType::Scene | WallpaperType::Web)
            && session.supports_audio_samples();
        let status = RuntimeStatusSnapshot {
            output_name: window.monitor.name.clone(),
            output_source: cfg.renderer.source.clone(),
            runtime: RuntimeDiagnostics {
                shm_available: true,
                prefer_dmabuf_configured: cfg.renderer.prefer_dmabuf,
                prefer_dmabuf_effective: false,
                allow_shm_fallback: true,
                media_integration_supported: supports_media,
                audio_integration_supported: supports_audio,
                options_json: OptionsJsonDiagnostics {
                    present: options_json_present,
                    len: options_json_len,
                    valid: options_json_valid,
                },
                renderer_diagnostics: session.diagnostics().ok().map(std::sync::Arc::new),
                ..RuntimeDiagnostics::default()
            },
            presentation: PresentationStatus {
                configured: true,
                logical_width: window.monitor.width,
                logical_height: window.monitor.height,
                render_width,
                render_height,
                output_mode_width: window.monitor.width,
                output_mode_height: window.monitor.height,
                viewport_width: window.monitor.width,
                viewport_height: window.monitor.height,
                scale_mode: cfg.general.scale_mode,
                ..PresentationStatus::default()
            },
            frame_stats: FrameStats::default(),
            ..RuntimeStatusSnapshot::default()
        };
        outputs.push(OutputRuntime {
            window_index,
            session,
            frame_fd,
            status,
            media_generation: u64::MAX,
            audio_generation: u64::MAX,
            policy_generation: u64::MAX,
            policy_paused: false,
            rule_muted: false,
            applied_muted: cfg.renderer.muted,
            manually_paused: false,
            next_tick: Instant::now(),
        });
    }

    let bridge_window = windows.windows[0].xid;
    if let Err(error) = super::dbus::register_window(
        &cfg.gnome.extension_dbus_name,
        bridge_window,
        std::process::id(),
        "we-layerd wallpaper renderer",
        "we-layerd",
    ) {
        super::dbus::unregister_window(&cfg.gnome.extension_dbus_name, bridge_window);
        return Err(error);
    }
    info!(
        monitors = outputs.len(),
        renderer = %library_path.display(),
        source = %source_path.display(),
        "registered XWayland wallpaper windows with GNOME Shell"
    );

    let interval = Duration::from_secs_f64(1.0 / cfg.renderer.fps.clamp(1, 360) as f64);
    let result = (|| -> Result<RuntimeLoopExit> {
        loop {
            if ctx.shutdown_requested.load(Ordering::Relaxed) {
                return Ok(RuntimeLoopExit::Stop);
            }

            while let Ok(command) = ctx.control_rx.try_recv() {
                match command {
                    ControlCommand::Stop => return Ok(RuntimeLoopExit::Stop),
                    ControlCommand::Pause | ControlCommand::Resume => {
                        for output in &mut outputs {
                            output.manually_paused = matches!(command, ControlCommand::Pause);
                            let paused = output.paused();
                            if paused {
                                output.session.pause();
                            } else {
                                output.session.resume();
                            }
                            output.status.presentation.paused = paused;
                            (ctx.status_sink)(output.status.clone());
                        }
                    }
                    ControlCommand::Reload => return Ok(RuntimeLoopExit::RestartCurrent),
                    ControlCommand::Reconfigure => return Ok(RuntimeLoopExit::Reconfigure),
                }
            }

            for output in &mut outputs {
                let integration =
                    ctx.host_integrations.snapshot_for_output(&output.status.output_name);
                if integration.policy_generation != output.policy_generation {
                    let was_paused = output.paused();
                    output.policy_generation = integration.policy_generation;
                    output.policy_paused = integration.policy.pause;
                    output.rule_muted = integration.policy.mute;
                    let is_paused = output.paused();
                    if was_paused != is_paused {
                        if is_paused {
                            output.session.pause();
                        } else {
                            output.session.resume();
                        }
                    }
                }
                let muted = cfg.renderer.muted || output.rule_muted;
                if muted != output.applied_muted {
                    if let Err(error) = output.session.set_muted(muted) {
                        warn!(output = %output.status.output_name, %error, "failed to update GNOME renderer mute state");
                    } else {
                        output.applied_muted = muted;
                    }
                }
                if integration.media_generation != output.media_generation {
                    output.media_generation = integration.media_generation;
                    if matches!(wallpaper_type, WallpaperType::Scene) {
                        match output.session.set_media_state(&integration.media) {
                            Ok(supported) => {
                                output.status.runtime.media_integration_supported = supported
                            }
                            Err(error) => {
                                warn!(output = %output.status.output_name, %error, "failed to update GNOME renderer media state")
                            }
                        }
                    }
                }
                if integration.audio_generation != output.audio_generation {
                    output.audio_generation = integration.audio_generation;
                    if matches!(wallpaper_type, WallpaperType::Scene | WallpaperType::Web) {
                        match output.session.push_audio_samples(&integration.audio) {
                            Ok(supported) => {
                                output.status.runtime.audio_integration_supported = supported
                            }
                            Err(error) => {
                                warn!(output = %output.status.output_name, %error, "failed to update GNOME renderer audio samples")
                            }
                        }
                    }
                }
                output.status.runtime.rule_muted = output.rule_muted;
                output.status.presentation.paused = output.paused();
                if !output.paused() && Instant::now() >= output.next_tick {
                    output.session.tick()?;
                    output.next_tick = Instant::now() + interval;
                }

                if fd_ready(output.frame_fd)? {
                    if let Some(frame) = output.session.acquire_frame(0)? {
                        let frame = match frame {
                            Frame::Shm(frame) => frame,
                            Frame::Dmabuf(_) => anyhow::bail!(
                                "GNOME XWayland bridge requires an SHM frame; renderer returned DMA-BUF"
                            ),
                        };
                        let width = frame.width;
                        let height = frame.height;
                        windows.present(&windows.windows[output.window_index], frame)?;
                        output.status.frame_stats.acquired += 1;
                        output.status.frame_stats.presented += 1;
                        output.status.frame_stats.last_present_backend = Some(PresentBackend::Shm);
                        output.status.frame_stats.last_frame_width = width;
                        output.status.frame_stats.last_frame_height = height;
                        (ctx.status_sink)(output.status.clone());
                    }
                }
            }
            windows.connection.flush().context("failed to flush XWayland connection")?;
            if windows.topology_changed()? {
                info!("XWayland monitor topology changed; restarting GNOME wallpaper runtime");
                return Ok(RuntimeLoopExit::RestartCurrent);
            }
            wait_for_runtime_events(&windows, &outputs)?;
        }
    })();
    super::dbus::unregister_window(&cfg.gnome.extension_dbus_name, bridge_window);
    result
}

fn wait_for_runtime_events(windows: &X11Windows, outputs: &[OutputRuntime]) -> Result<()> {
    let x11_fd = windows.connection.stream().as_raw_fd();
    let mut poll_fds = Vec::with_capacity(outputs.len() + 1);
    poll_fds.push(libc::pollfd { fd: x11_fd, events: libc::POLLIN, revents: 0 });
    poll_fds.extend(outputs.iter().map(|output| libc::pollfd {
        fd: output.frame_fd,
        events: libc::POLLIN,
        revents: 0,
    }));

    let next_tick =
        outputs.iter().filter(|output| !output.paused()).map(|output| output.next_tick).min();
    let timeout = next_tick
        .map(|at| at.saturating_duration_since(Instant::now()).min(MAX_RUNTIME_IDLE_WAIT))
        .unwrap_or(MAX_RUNTIME_IDLE_WAIT);
    let deadline = Instant::now() + timeout;

    loop {
        let timeout_ms =
            deadline.saturating_duration_since(Instant::now()).as_millis().min(i32::MAX as u128)
                as i32;
        let ready = unsafe {
            libc::poll(poll_fds.as_mut_ptr(), poll_fds.len() as libc::nfds_t, timeout_ms)
        };
        if ready >= 0 {
            for fd in &poll_fds {
                if fd.revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0 {
                    if fd.fd == x11_fd {
                        anyhow::bail!("XWayland connection fd became invalid");
                    }
                    anyhow::bail!("renderer frame-ready fd became invalid");
                }
            }
            return Ok(());
        }

        let error = std::io::Error::last_os_error();
        if error.kind() != std::io::ErrorKind::Interrupted {
            return Err(error).context("failed to wait for GNOME wallpaper events");
        }
        if Instant::now() >= deadline {
            return Ok(());
        }
    }
}

fn fd_ready(fd: RawFd) -> Result<bool> {
    let mut pollfd = libc::pollfd { fd, events: libc::POLLIN, revents: 0 };
    loop {
        let result = unsafe { libc::poll(&mut pollfd, 1, 0) };
        if result >= 0 {
            return Ok(result > 0 && pollfd.revents & libc::POLLIN != 0);
        }
        let error = std::io::Error::last_os_error();
        if error.kind() != std::io::ErrorKind::Interrupted {
            return Err(error).context("failed to poll renderer frame notification");
        }
    }
}

fn renderer_fill_mode(value: we_core::wallpaper::settings::WallpaperFillMode) -> FillMode {
    match value {
        we_core::wallpaper::settings::WallpaperFillMode::Cover => FillMode::Cover,
        we_core::wallpaper::settings::WallpaperFillMode::Fit => FillMode::Fit,
        we_core::wallpaper::settings::WallpaperFillMode::Stretch => FillMode::Stretch,
        we_core::wallpaper::settings::WallpaperFillMode::Center => FillMode::Center,
    }
}
