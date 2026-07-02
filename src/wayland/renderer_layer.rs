use std::{
    io::ErrorKind,
    os::fd::{AsFd, AsRawFd},
    path::{Path, PathBuf},
    sync::mpsc,
    time::Duration,
};

use anyhow::{Context, Result};
use tracing::info;
use wayland_backend::client::WaylandError;
use wayland_client::{
    delegate_noop,
    globals::registry_queue_init,
    globals::GlobalListContents,
    protocol::{
        wl_buffer::WlBuffer, wl_compositor::WlCompositor, wl_output, wl_output::WlOutput,
        wl_region::WlRegion, wl_registry, wl_seat::WlSeat, wl_shm::WlShm, wl_shm_pool::WlShmPool,
        wl_surface::WlSurface,
    },
    Connection, Dispatch, EventQueue, Proxy, QueueHandle, WEnum,
};
use wayland_protocols::wp::{
    fractional_scale::v1::client::{
        wp_fractional_scale_manager_v1::WpFractionalScaleManagerV1,
        wp_fractional_scale_v1::{Event as FractionalScaleEvent, WpFractionalScaleV1},
    },
    linux_dmabuf::zv1::client::{
        zwp_linux_buffer_params_v1::ZwpLinuxBufferParamsV1, zwp_linux_dmabuf_v1::ZwpLinuxDmabufV1,
    },
    viewporter::client::{wp_viewport::WpViewport, wp_viewporter::WpViewporter},
};
use wayland_protocols_wlr::layer_shell::v1::client::{
    zwlr_layer_shell_v1::{Layer, ZwlrLayerShellV1},
    zwlr_layer_surface_v1::{
        Anchor, Event as LayerSurfaceEvent, KeyboardInteractivity, ZwlrLayerSurfaceV1,
    },
};
use we_renderer::{RenderConfig, RendererLibrary, Session, Source};

use crate::{
    config::Config,
    ipc::{ControlCommand, RuntimeLoopExit},
    wayland::{
        frame_present::{BufferState, FramePresenter},
        outputs,
    },
};

const FRACTIONAL_SCALE_DENOMINATOR: u32 = 120;

#[derive(Debug)]
struct RendererLayerState {
    running: bool,
    configured: bool,
    interactive: bool,
    compositor_version: u32,
    logical_width: u32,
    logical_height: u32,
    output_scale: u32,
    preferred_fractional_scale: u32,
    output_mode_width: u32,
    output_mode_height: u32,
    fallback_width: u32,
    fallback_height: u32,
}

impl RendererLayerState {
    fn new(interactive: bool) -> Self {
        Self {
            running: true,
            configured: false,
            interactive,
            compositor_version: 0,
            logical_width: 0,
            logical_height: 0,
            output_scale: 1,
            preferred_fractional_scale: 0,
            output_mode_width: 0,
            output_mode_height: 0,
            fallback_width: 1920,
            fallback_height: 1080,
        }
    }

    fn render_extent(&self) -> (u32, u32) {
        if self.output_mode_width > 0 && self.output_mode_height > 0 {
            return (self.output_mode_width, self.output_mode_height);
        }

        let logical_width = self.logical_width.max(self.fallback_width).max(1);
        let logical_height = self.logical_height.max(self.fallback_height).max(1);
        let scale = if self.preferred_fractional_scale >= FRACTIONAL_SCALE_DENOMINATOR {
            self.preferred_fractional_scale as f64 / FRACTIONAL_SCALE_DENOMINATOR as f64
        } else {
            self.output_scale.max(1) as f64
        };

        (
            (logical_width as f64 * scale).round().max(1.0) as u32,
            (logical_height as f64 * scale).round().max(1.0) as u32,
        )
    }
}

struct RendererRuntime {
    event_queue: EventQueue<RendererLayerState>,
    qh: QueueHandle<RendererLayerState>,
    state: RendererLayerState,
    compositor: WlCompositor,
    surface: WlSurface,
    _output: Option<WlOutput>,
    viewport: Option<WpViewport>,
    _fractional_scale: Option<WpFractionalScaleV1>,
    _layer_surface: ZwlrLayerSurfaceV1,
    presenter: FramePresenter,
    session: Session,
    session_configured_extent: Option<(u32, u32)>,
    paused: bool,
}

pub fn run_renderer_background_surface(
    cfg: &Config,
    control_rx: &mpsc::Receiver<ControlCommand>,
) -> Result<RuntimeLoopExit> {
    let conn = Connection::connect_to_env().context("failed to connect to Wayland display")?;
    let (globals, event_queue) = registry_queue_init::<RendererLayerState>(&conn)
        .context("failed to init Wayland registry")?;
    let qh = event_queue.handle();

    let compositor: WlCompositor =
        globals.bind(&qh, 4..=6, ()).context("failed to bind wl_compositor")?;
    let compositor_version = compositor.version();
    let layer_shell: ZwlrLayerShellV1 =
        globals.bind(&qh, 1..=5, ()).context("failed to bind zwlr_layer_shell_v1")?;
    let shm: WlShm = globals.bind(&qh, 1..=1, ()).context("failed to bind wl_shm")?;
    let dmabuf = globals.bind::<ZwpLinuxDmabufV1, _, _>(&qh, 3..=4, ()).ok();
    let viewporter = globals.bind::<WpViewporter, _, _>(&qh, 1..=1, ()).ok();
    let fractional_scale_manager =
        globals.bind::<WpFractionalScaleManagerV1, _, _>(&qh, 1..=1, ()).ok();

    let globals_snapshot = globals.contents().clone_list();
    let output_global = outputs::output_globals(&globals_snapshot).into_iter().next();
    let output = output_global
        .as_ref()
        .map(|global| outputs::bind_output::<RendererLayerState>(globals.registry(), &qh, global))
        .transpose()
        .context("failed to bind wl_output")?;

    let surface = compositor.create_surface(&qh, ());
    surface.set_buffer_scale(1);
    let viewport = viewporter.as_ref().map(|global| global.get_viewport(&surface, &qh, ()));
    let fractional_scale = fractional_scale_manager
        .as_ref()
        .map(|global| global.get_fractional_scale(&surface, &qh, ()));

    let layer_surface = layer_shell.get_layer_surface(
        &surface,
        output.as_ref(),
        Layer::Background,
        "we-layerd".to_string(),
        &qh,
        (),
    );
    layer_surface.set_anchor(Anchor::Top | Anchor::Bottom | Anchor::Left | Anchor::Right);
    layer_surface.set_size(0, 0);
    layer_surface.set_exclusive_zone(-1);
    layer_surface.set_margin(0, 0, 0, 0);
    layer_surface.set_keyboard_interactivity(KeyboardInteractivity::None);

    let cache_path = expand_tilde(&cfg.renderer.cache_path);
    let source_path = expand_tilde(&cfg.renderer.source);
    let assets_path = expand_tilde(&cfg.renderer.assets_path);

    let library =
        RendererLibrary::load(Path::new(&cfg.renderer.library_path)).with_context(|| {
            format!("failed to load renderer library {}", cfg.renderer.library_path)
        })?;
    let cache_path_arg =
        if cfg.renderer.cache_path.trim().is_empty() { None } else { Some(cache_path.as_path()) };
    let mut session =
        library.create_session(cache_path_arg).context("failed to create renderer session")?;
    session
        .set_source(&Source {
            uri: source_path.display().to_string(),
            assets_uri: assets_path.display().to_string(),
            fps: cfg.renderer.fps as i32,
            speed: cfg.renderer.speed,
            volume: cfg.renderer.volume,
            muted: cfg.renderer.muted,
            options_json: None,
        })
        .context("failed to set renderer source")?;
    session.play().context("failed to start renderer session")?;

    let presenter = FramePresenter::new(compositor_version, dmabuf, Some(shm));
    let mut runtime = RendererRuntime {
        event_queue,
        qh,
        state: RendererLayerState::new(cfg.general.interactive),
        compositor,
        surface,
        _output: output,
        viewport,
        _fractional_scale: fractional_scale,
        _layer_surface: layer_surface,
        presenter,
        session,
        session_configured_extent: None,
        paused: false,
    };
    runtime.state.compositor_version = compositor_version;
    runtime.apply_input_region()?;
    runtime.surface.commit();

    while !runtime.state.configured {
        runtime
            .event_queue
            .roundtrip(&mut runtime.state)
            .context("failed waiting for initial layer-surface configure")?;
        runtime.apply_surface_geometry()?;
    }

    runtime.apply_surface_geometry()?;
    runtime.configure_session_if_needed(cfg)?;

    info!(
        logical_width = runtime.state.logical_width,
        logical_height = runtime.state.logical_height,
        render_width = runtime.state.render_extent().0,
        render_height = runtime.state.render_extent().1,
        "starting renderer-backed layer-shell runtime"
    );

    loop {
        while let Ok(cmd) = control_rx.try_recv() {
            match runtime.handle_control_command(cmd)? {
                Some(exit) => return Ok(exit),
                None => {}
            }
        }

        runtime.configure_session_if_needed(cfg)?;
        if !runtime.paused {
            runtime.session.tick().context("renderer tick failed")?;
            if let Some(frame) =
                runtime.session.acquire_frame().context("failed to acquire frame")?
            {
                runtime
                    .presenter
                    .present(&runtime.surface, &runtime.qh, frame)
                    .context("failed to present renderer frame")?;
            }
        }

        let flush_blocked = match runtime.event_queue.flush() {
            Ok(()) => {
                runtime.presenter.release_pending_send_fds();
                false
            }
            Err(WaylandError::Io(err)) if err.kind() == ErrorKind::WouldBlock => true,
            Err(err) => return Err(err).context("failed to flush Wayland connection"),
        };

        runtime.presenter.collect_released_buffers();
        runtime.dispatch_wayland(Duration::from_millis(5), flush_blocked)?;
        runtime.presenter.collect_released_buffers();

        if !runtime.state.running {
            runtime.session.stop().ok();
            return Ok(RuntimeLoopExit::Stop);
        }
    }
}

impl RendererRuntime {
    fn handle_control_command(&mut self, cmd: ControlCommand) -> Result<Option<RuntimeLoopExit>> {
        match cmd {
            ControlCommand::Stop => {
                self.session.stop().ok();
                Ok(Some(RuntimeLoopExit::Stop))
            }
            ControlCommand::Pause => {
                self.session.pause().context("failed to pause renderer session")?;
                self.paused = true;
                Ok(None)
            }
            ControlCommand::Resume => {
                self.session.play().context("failed to resume renderer session")?;
                self.paused = false;
                Ok(None)
            }
            ControlCommand::Reload => Ok(Some(RuntimeLoopExit::RestartCurrent)),
            ControlCommand::Reconfigure => Ok(Some(RuntimeLoopExit::Reconfigure)),
        }
    }

    fn configure_session_if_needed(&mut self, cfg: &Config) -> Result<()> {
        let extent = self.state.render_extent();
        if self.session_configured_extent == Some(extent) {
            return Ok(());
        }

        self.session
            .configure(RenderConfig {
                width: extent.0,
                height: extent.1,
                enable_valid_layer: false,
                prefer_dmabuf: cfg.renderer.prefer_dmabuf,
                allow_shm_fallback: cfg.renderer.allow_shm_fallback,
            })
            .with_context(|| {
                format!("failed to configure renderer session for {}x{}", extent.0, extent.1)
            })?;
        self.session_configured_extent = Some(extent);
        Ok(())
    }

    fn dispatch_wayland(&mut self, timeout: Duration, flush_blocked: bool) -> Result<()> {
        self.event_queue
            .dispatch_pending(&mut self.state)
            .context("failed to dispatch pending Wayland events")?;

        let Some(read_guard) = self.event_queue.prepare_read() else {
            return Ok(());
        };

        let fd = self.event_queue.as_fd().as_raw_fd();
        let mut poll_fd = libc::pollfd {
            fd,
            events: libc::POLLIN | if flush_blocked { libc::POLLOUT } else { 0 },
            revents: 0,
        };
        let poll_result = unsafe { libc::poll(&mut poll_fd, 1, timeout.as_millis() as i32) };
        if poll_result < 0 {
            let err = std::io::Error::last_os_error();
            if err.kind() == ErrorKind::Interrupted {
                drop(read_guard);
                return Ok(());
            }
            return Err(err).context("failed to poll Wayland fd");
        }
        if poll_result == 0 {
            drop(read_guard);
            return Ok(());
        }
        if (poll_fd.revents & (libc::POLLERR | libc::POLLHUP)) != 0 {
            self.state.running = false;
            drop(read_guard);
            return Ok(());
        }
        if (poll_fd.revents & libc::POLLIN) != 0 {
            read_guard.read().context("failed to read Wayland events")?;
            self.event_queue
                .dispatch_pending(&mut self.state)
                .context("failed to dispatch Wayland events after read")?;
            self.apply_surface_geometry()?;
        } else {
            drop(read_guard);
        }
        if flush_blocked && (poll_fd.revents & libc::POLLOUT) != 0 {
            match self.event_queue.flush() {
                Ok(()) => self.presenter.release_pending_send_fds(),
                Err(WaylandError::Io(err)) if err.kind() == ErrorKind::WouldBlock => {}
                Err(err) => return Err(err).context("failed to flush Wayland fd after POLLOUT"),
            }
        }
        Ok(())
    }

    fn apply_surface_geometry(&mut self) -> Result<()> {
        if let Some(viewport) = &self.viewport {
            viewport
                .set_destination(self.state.logical_width as i32, self.state.logical_height as i32);
        }
        self.apply_input_region()?;
        Ok(())
    }

    fn apply_input_region(&self) -> Result<()> {
        if self.state.logical_width == 0 || self.state.logical_height == 0 {
            return Ok(());
        }
        let region = self.compositor.create_region(&self.qh, ());
        if self.state.interactive {
            region.add(0, 0, self.state.logical_width as i32, self.state.logical_height as i32);
            self.surface.set_input_region(Some(&region));
        } else {
            self.surface.set_input_region(Some(&region));
        }
        region.destroy();
        Ok(())
    }
}

fn expand_tilde(raw: &str) -> PathBuf {
    if let Some(rest) = raw.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join(rest);
        }
    }
    PathBuf::from(raw)
}

impl Dispatch<ZwlrLayerSurfaceV1, ()> for RendererLayerState {
    fn event(
        state: &mut Self,
        layer_surface: &ZwlrLayerSurfaceV1,
        event: LayerSurfaceEvent,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        match event {
            LayerSurfaceEvent::Configure { serial, width, height } => {
                layer_surface.ack_configure(serial);
                state.configured = true;
                state.logical_width = width.max(1);
                state.logical_height = height.max(1);
            }
            LayerSurfaceEvent::Closed => state.running = false,
            _ => {}
        }
    }
}

impl Dispatch<WlOutput, ()> for RendererLayerState {
    fn event(
        state: &mut Self,
        _proxy: &WlOutput,
        event: wl_output::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        match event {
            wl_output::Event::Mode { flags, width, height, .. } if matches!(flags, WEnum::Value(value) if value.contains(wl_output::Mode::Current)) =>
            {
                state.output_mode_width = width.max(0) as u32;
                state.output_mode_height = height.max(0) as u32;
            }
            wl_output::Event::Scale { factor } => {
                state.output_scale = factor.max(1) as u32;
            }
            _ => {}
        }
    }
}

impl Dispatch<WpFractionalScaleV1, ()> for RendererLayerState {
    fn event(
        state: &mut Self,
        _proxy: &WpFractionalScaleV1,
        event: FractionalScaleEvent,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        if let FractionalScaleEvent::PreferredScale { scale } = event {
            state.preferred_fractional_scale = scale.max(FRACTIONAL_SCALE_DENOMINATOR);
        }
    }
}

impl Dispatch<WlBuffer, BufferState> for RendererLayerState {
    fn event(
        _state: &mut Self,
        _proxy: &WlBuffer,
        event: wayland_client::protocol::wl_buffer::Event,
        data: &BufferState,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        if let wayland_client::protocol::wl_buffer::Event::Release = event {
            data.released.store(true, std::sync::atomic::Ordering::SeqCst);
        }
    }
}

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for RendererLayerState {
    fn event(
        _state: &mut Self,
        _proxy: &wl_registry::WlRegistry,
        _event: wl_registry::Event,
        _data: &GlobalListContents,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
    }
}

delegate_noop!(RendererLayerState: ignore WlCompositor);
delegate_noop!(RendererLayerState: ignore WlSurface);
delegate_noop!(RendererLayerState: ignore WlRegion);
delegate_noop!(RendererLayerState: ignore ZwlrLayerShellV1);
delegate_noop!(RendererLayerState: ignore WlShm);
delegate_noop!(RendererLayerState: ignore WlShmPool);
delegate_noop!(RendererLayerState: ignore ZwpLinuxDmabufV1);
delegate_noop!(RendererLayerState: ignore ZwpLinuxBufferParamsV1);
delegate_noop!(RendererLayerState: ignore WpViewporter);
delegate_noop!(RendererLayerState: ignore WpViewport);
delegate_noop!(RendererLayerState: ignore WpFractionalScaleManagerV1);
delegate_noop!(RendererLayerState: ignore WlSeat);

#[cfg(test)]
mod tests {
    use super::{expand_tilde, RendererLayerState, FRACTIONAL_SCALE_DENOMINATOR};

    #[test]
    fn render_extent_prefers_output_mode() {
        let mut state = RendererLayerState::new(true);
        state.output_mode_width = 2560;
        state.output_mode_height = 1440;
        state.logical_width = 1920;
        state.logical_height = 1080;

        assert_eq!(state.render_extent(), (2560, 1440));
    }

    #[test]
    fn render_extent_uses_fractional_scale_when_available() {
        let mut state = RendererLayerState::new(true);
        state.fallback_width = 0;
        state.fallback_height = 0;
        state.logical_width = 100;
        state.logical_height = 50;
        state.preferred_fractional_scale = FRACTIONAL_SCALE_DENOMINATOR + 60;

        assert_eq!(state.render_extent(), (150, 75));
    }

    #[test]
    fn expand_tilde_expands_home_prefix() {
        let home = std::env::var_os("HOME").expect("HOME must be set in test env");
        let expanded = expand_tilde("~/renderer-cache");
        assert_eq!(expanded, std::path::PathBuf::from(home).join("renderer-cache"));
    }
}
