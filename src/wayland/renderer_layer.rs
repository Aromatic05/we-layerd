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
    globals::{Global, GlobalListContents},
    protocol::{
        wl_buffer::WlBuffer, wl_compositor::WlCompositor, wl_output, wl_output::WlOutput,
        wl_pointer, wl_pointer::WlPointer, wl_region::WlRegion, wl_registry, wl_seat::WlSeat,
        wl_shm::WlShm, wl_shm_pool::WlShmPool, wl_surface::WlSurface,
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
use we_renderer::{InputEvent, RenderConfig, RendererLibrary, Session, Source};

use crate::{
    config::Config,
    ipc::{ControlCommand, RuntimeLoopExit},
    wayland::{
        frame_present::{BufferState, FramePresenter},
        outputs,
    },
};

const FRACTIONAL_SCALE_DENOMINATOR: u32 = 120;

#[derive(Debug, Clone)]
struct OutputState {
    name: String,
    configured: bool,
    logical_width: u32,
    logical_height: u32,
    output_scale: u32,
    preferred_fractional_scale: u32,
    output_mode_width: u32,
    output_mode_height: u32,
    fallback_width: u32,
    fallback_height: u32,
    pointer_x: f64,
    pointer_y: f64,
}

impl OutputState {
    fn new(name: String) -> Self {
        Self {
            name,
            configured: false,
            logical_width: 0,
            logical_height: 0,
            output_scale: 1,
            preferred_fractional_scale: 0,
            output_mode_width: 0,
            output_mode_height: 0,
            fallback_width: 1920,
            fallback_height: 1080,
            pointer_x: 0.0,
            pointer_y: 0.0,
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

    fn normalized_pointer(&self) -> Option<(f32, f32)> {
        if self.logical_width == 0 || self.logical_height == 0 {
            return None;
        }
        Some((
            (self.pointer_x / self.logical_width as f64) as f32,
            (self.pointer_y / self.logical_height as f64) as f32,
        ))
    }
}

#[derive(Debug)]
struct RendererLayerState {
    running: bool,
    interactive: bool,
    outputs: Vec<OutputState>,
    focused_output: Option<usize>,
    pending_input_events: Vec<(usize, InputEvent)>,
    _pointer: Option<WlPointer>,
}

impl RendererLayerState {
    fn new(interactive: bool, outputs: Vec<OutputState>) -> Self {
        Self {
            running: true,
            interactive,
            outputs,
            focused_output: None,
            pending_input_events: Vec::new(),
            _pointer: None,
        }
    }

    fn all_outputs_configured(&self) -> bool {
        self.outputs.iter().all(|output| output.configured)
    }
}

struct OutputRuntime {
    surface: WlSurface,
    _output: Option<WlOutput>,
    viewport: Option<WpViewport>,
    _fractional_scale: Option<WpFractionalScaleV1>,
    _layer_surface: ZwlrLayerSurfaceV1,
    presenter: FramePresenter,
    session: Session,
    session_configured_extent: Option<(u32, u32)>,
}

struct RendererRuntime {
    event_queue: EventQueue<RendererLayerState>,
    qh: QueueHandle<RendererLayerState>,
    state: RendererLayerState,
    compositor: WlCompositor,
    _seat: Option<WlSeat>,
    outputs: Vec<OutputRuntime>,
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
    let seat = globals.bind::<WlSeat, _, _>(&qh, 1..=5, ()).ok();
    let viewporter = globals.bind::<WpViewporter, _, _>(&qh, 1..=1, ()).ok();
    let fractional_scale_manager =
        globals.bind::<WpFractionalScaleManagerV1, _, _>(&qh, 1..=1, ()).ok();

    let globals_snapshot = globals.contents().clone_list();
    let output_globals = outputs::output_globals(&globals_snapshot);
    let output_specs = if output_globals.is_empty() {
        vec![OutputSpec { name: "output-fallback".to_string(), global: None }]
    } else {
        output_globals
            .into_iter()
            .map(|global| OutputSpec {
                name: format!("output-{}", global.name),
                global: Some(global),
            })
            .collect::<Vec<_>>()
    };

    let cache_path = expand_tilde(&cfg.renderer.cache_path);
    let source_path = expand_tilde(&cfg.renderer.source);
    let assets_path = expand_tilde(&cfg.renderer.assets_path);
    let source = Source {
        uri: source_path.display().to_string(),
        assets_uri: assets_path.display().to_string(),
        fps: cfg.renderer.fps as i32,
        speed: cfg.renderer.speed,
        volume: cfg.renderer.volume,
        muted: cfg.renderer.muted,
        options_json: None,
    };

    let library_path = resolve_renderer_library_path(&cfg.renderer.library_path);
    let library = RendererLibrary::load(&library_path)
        .with_context(|| format!("failed to load renderer library {}", library_path.display()))?;
    let cache_path_arg =
        if cfg.renderer.cache_path.trim().is_empty() { None } else { Some(cache_path.as_path()) };

    let mut output_states = Vec::with_capacity(output_specs.len());
    let mut output_runtimes = Vec::with_capacity(output_specs.len());

    for (index, spec) in output_specs.iter().enumerate() {
        output_states.push(OutputState::new(spec.name.clone()));
        output_runtimes.push(create_output_runtime(
            &globals,
            &qh,
            &compositor,
            compositor_version,
            &layer_shell,
            dmabuf.clone(),
            shm.clone(),
            viewporter.as_ref(),
            fractional_scale_manager.as_ref(),
            &library,
            cache_path_arg,
            &source,
            spec,
            index,
        )?);
    }

    let seat_available = seat.is_some();
    let mut runtime = RendererRuntime {
        event_queue,
        qh,
        state: RendererLayerState::new(cfg.general.interactive, output_states),
        compositor,
        _seat: seat,
        outputs: output_runtimes,
        paused: false,
    };

    if !seat_available {
        info!("wl_seat unavailable; pointer forwarding disabled");
    }

    runtime.apply_all_input_regions()?;
    runtime.commit_all_surfaces();

    while !runtime.state.all_outputs_configured() {
        runtime
            .event_queue
            .roundtrip(&mut runtime.state)
            .context("failed waiting for initial layer-surface configure")?;
        runtime.apply_all_surface_geometry()?;
    }

    runtime.apply_all_surface_geometry()?;
    runtime.configure_sessions_if_needed(cfg)?;

    for output in &runtime.state.outputs {
        let (render_width, render_height) = output.render_extent();
        info!(
            output = %output.name,
            logical_width = output.logical_width,
            logical_height = output.logical_height,
            render_width,
            render_height,
            "starting renderer-backed layer-shell output"
        );
    }

    loop {
        while let Ok(cmd) = control_rx.try_recv() {
            match runtime.handle_control_command(cmd)? {
                Some(exit) => return Ok(exit),
                None => {}
            }
        }

        let input_events = std::mem::take(&mut runtime.state.pending_input_events);
        for (output_index, event) in input_events {
            if let Some(output) = runtime.outputs.get_mut(output_index) {
                output
                    .session
                    .send_input_event(event)
                    .context("failed to forward input event to renderer")?;
            }
        }

        runtime.configure_sessions_if_needed(cfg)?;
        if !runtime.paused {
            for (index, output) in runtime.outputs.iter_mut().enumerate() {
                output.session.tick().context("renderer tick failed")?;
                if let Some(frame) =
                    output.session.acquire_frame().context("failed to acquire frame")?
                {
                    output.presenter.present(&output.surface, &runtime.qh, frame).with_context(
                        || format!("failed to present renderer frame for output {index}"),
                    )?;
                }
            }
        }

        let flush_blocked = match runtime.event_queue.flush() {
            Ok(()) => {
                runtime.release_pending_send_fds();
                false
            }
            Err(WaylandError::Io(err)) if err.kind() == ErrorKind::WouldBlock => true,
            Err(err) => return Err(err).context("failed to flush Wayland connection"),
        };

        runtime.collect_released_buffers();
        runtime.dispatch_wayland(Duration::from_millis(5), flush_blocked)?;
        runtime.collect_released_buffers();

        if !runtime.state.running {
            runtime.stop_sessions();
            return Ok(RuntimeLoopExit::Stop);
        }
    }
}

impl RendererRuntime {
    fn handle_control_command(&mut self, cmd: ControlCommand) -> Result<Option<RuntimeLoopExit>> {
        match cmd {
            ControlCommand::Stop => {
                self.stop_sessions();
                Ok(Some(RuntimeLoopExit::Stop))
            }
            ControlCommand::Pause => {
                for output in &mut self.outputs {
                    output.session.pause().context("failed to pause renderer session")?;
                }
                self.paused = true;
                Ok(None)
            }
            ControlCommand::Resume => {
                for output in &mut self.outputs {
                    output.session.play().context("failed to resume renderer session")?;
                }
                self.paused = false;
                Ok(None)
            }
            ControlCommand::Reload => Ok(Some(RuntimeLoopExit::RestartCurrent)),
            ControlCommand::Reconfigure => Ok(Some(RuntimeLoopExit::Reconfigure)),
        }
    }

    fn configure_sessions_if_needed(&mut self, cfg: &Config) -> Result<()> {
        for (state, runtime) in self.state.outputs.iter().zip(self.outputs.iter_mut()) {
            let extent = state.render_extent();
            if runtime.session_configured_extent == Some(extent) {
                continue;
            }
            runtime
                .session
                .configure(RenderConfig {
                    width: extent.0,
                    height: extent.1,
                    enable_valid_layer: false,
                    prefer_dmabuf: cfg.renderer.prefer_dmabuf,
                    allow_shm_fallback: cfg.renderer.allow_shm_fallback,
                })
                .with_context(|| {
                    format!(
                        "failed to configure renderer session for {} at {}x{}",
                        state.name, extent.0, extent.1
                    )
                })?;
            runtime.session_configured_extent = Some(extent);
        }
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
            self.apply_all_surface_geometry()?;
        } else {
            drop(read_guard);
        }
        if flush_blocked && (poll_fd.revents & libc::POLLOUT) != 0 {
            match self.event_queue.flush() {
                Ok(()) => self.release_pending_send_fds(),
                Err(WaylandError::Io(err)) if err.kind() == ErrorKind::WouldBlock => {}
                Err(err) => return Err(err).context("failed to flush Wayland fd after POLLOUT"),
            }
        }
        Ok(())
    }

    fn apply_all_surface_geometry(&mut self) -> Result<()> {
        for index in 0..self.outputs.len() {
            self.apply_surface_geometry(index)?;
        }
        Ok(())
    }

    fn apply_surface_geometry(&mut self, index: usize) -> Result<()> {
        let Some(state) = self.state.outputs.get(index) else {
            return Ok(());
        };
        let Some(output) = self.outputs.get_mut(index) else {
            return Ok(());
        };
        if let Some(viewport) = &output.viewport {
            viewport.set_destination(state.logical_width as i32, state.logical_height as i32);
        }
        self.apply_input_region(index)?;
        Ok(())
    }

    fn apply_all_input_regions(&self) -> Result<()> {
        for index in 0..self.outputs.len() {
            self.apply_input_region(index)?;
        }
        Ok(())
    }

    fn apply_input_region(&self, index: usize) -> Result<()> {
        let Some(state) = self.state.outputs.get(index) else {
            return Ok(());
        };
        let Some(output) = self.outputs.get(index) else {
            return Ok(());
        };
        if state.logical_width == 0 || state.logical_height == 0 {
            return Ok(());
        }
        let region = self.compositor.create_region(&self.qh, ());
        if self.state.interactive {
            region.add(0, 0, state.logical_width as i32, state.logical_height as i32);
            output.surface.set_input_region(Some(&region));
        } else {
            output.surface.set_input_region(Some(&region));
        }
        region.destroy();
        Ok(())
    }

    fn commit_all_surfaces(&self) {
        for output in &self.outputs {
            output.surface.commit();
        }
    }

    fn release_pending_send_fds(&mut self) {
        for output in &mut self.outputs {
            output.presenter.release_pending_send_fds();
        }
    }

    fn collect_released_buffers(&mut self) {
        for output in &mut self.outputs {
            output.presenter.collect_released_buffers();
        }
    }

    fn stop_sessions(&mut self) {
        for output in &mut self.outputs {
            output.session.stop().ok();
        }
    }
}

struct OutputSpec {
    name: String,
    global: Option<Global>,
}

#[allow(clippy::too_many_arguments)]
fn create_output_runtime(
    globals: &wayland_client::globals::GlobalList,
    qh: &QueueHandle<RendererLayerState>,
    compositor: &WlCompositor,
    compositor_version: u32,
    layer_shell: &ZwlrLayerShellV1,
    dmabuf: Option<ZwpLinuxDmabufV1>,
    shm: WlShm,
    viewporter: Option<&WpViewporter>,
    fractional_scale_manager: Option<&WpFractionalScaleManagerV1>,
    library: &RendererLibrary,
    cache_path: Option<&Path>,
    source: &Source,
    spec: &OutputSpec,
    index: usize,
) -> Result<OutputRuntime> {
    let output = spec
        .global
        .as_ref()
        .map(|global| outputs::bind_output(globals.registry(), qh, global, index))
        .transpose()
        .context("failed to bind output for renderer runtime")?;

    let surface = compositor.create_surface(qh, ());
    surface.set_buffer_scale(1);
    let viewport = viewporter.map(|global| global.get_viewport(&surface, qh, ()));
    let fractional_scale =
        fractional_scale_manager.map(|global| global.get_fractional_scale(&surface, qh, index));

    let layer_surface = layer_shell.get_layer_surface(
        &surface,
        output.as_ref(),
        Layer::Background,
        "we-layerd".to_string(),
        qh,
        index,
    );
    layer_surface.set_anchor(Anchor::Top | Anchor::Bottom | Anchor::Left | Anchor::Right);
    layer_surface.set_size(0, 0);
    layer_surface.set_exclusive_zone(-1);
    layer_surface.set_margin(0, 0, 0, 0);
    layer_surface.set_keyboard_interactivity(KeyboardInteractivity::None);

    let mut session =
        library.create_session(cache_path).context("failed to create renderer session")?;
    session.set_source(source).context("failed to set renderer source")?;
    session.play().context("failed to start renderer session")?;

    Ok(OutputRuntime {
        surface,
        _output: output,
        viewport,
        _fractional_scale: fractional_scale,
        _layer_surface: layer_surface,
        presenter: FramePresenter::new(compositor_version, dmabuf, Some(shm)),
        session,
        session_configured_extent: None,
    })
}

fn expand_tilde(raw: &str) -> PathBuf {
    if let Some(rest) = raw.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join(rest);
        }
    }
    PathBuf::from(raw)
}

fn resolve_renderer_library_path(configured_path: &str) -> PathBuf {
    let configured = PathBuf::from(configured_path);
    if configured.exists() {
        return configured;
    }

    let mut candidates = Vec::new();
    if let Some(install_root) = option_env!("WE_LAYERD_RENDERER_INSTALL_ROOT") {
        candidates.push(
            Path::new(install_root).join("lib/libwallpaper-engine-renderer.so"),
        );
    }
    if let Some(home) = std::env::var_os("HOME") {
        candidates.push(Path::new(&home).join(".local/bin/lib/libwallpaper-engine-renderer.so"));
    }
    if let Ok(current_exe) = std::env::current_exe() {
        if let Some(bin_dir) = current_exe.parent() {
            candidates.push(bin_dir.join("../lib/libwallpaper-engine-renderer.so"));
            candidates.push(bin_dir.join("libwallpaper-engine-renderer.so"));
        }
    }
    candidates.push(PathBuf::from("/usr/local/lib/libwallpaper-engine-renderer.so"));
    candidates.push(PathBuf::from("/usr/local/lib64/libwallpaper-engine-renderer.so"));
    candidates.push(PathBuf::from("/usr/lib/libwallpaper-engine-renderer.so"));
    candidates.push(PathBuf::from("/usr/lib64/libwallpaper-engine-renderer.so"));

    candidates
        .into_iter()
        .find(|candidate| candidate.exists())
        .unwrap_or(configured)
}

impl Dispatch<ZwlrLayerSurfaceV1, usize> for RendererLayerState {
    fn event(
        state: &mut Self,
        layer_surface: &ZwlrLayerSurfaceV1,
        event: LayerSurfaceEvent,
        index: &usize,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        let Some(output) = state.outputs.get_mut(*index) else {
            return;
        };
        match event {
            LayerSurfaceEvent::Configure { serial, width, height } => {
                layer_surface.ack_configure(serial);
                output.configured = true;
                output.logical_width = width.max(1);
                output.logical_height = height.max(1);
            }
            LayerSurfaceEvent::Closed => state.running = false,
            _ => {}
        }
    }
}

impl Dispatch<WlOutput, usize> for RendererLayerState {
    fn event(
        state: &mut Self,
        _proxy: &WlOutput,
        event: wl_output::Event,
        index: &usize,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        let Some(output) = state.outputs.get_mut(*index) else {
            return;
        };
        match event {
            wl_output::Event::Mode { flags, width, height, .. } if matches!(flags, WEnum::Value(value) if value.contains(wl_output::Mode::Current)) =>
            {
                output.output_mode_width = width.max(0) as u32;
                output.output_mode_height = height.max(0) as u32;
            }
            wl_output::Event::Scale { factor } => {
                output.output_scale = factor.max(1) as u32;
            }
            _ => {}
        }
    }
}

impl Dispatch<WpFractionalScaleV1, usize> for RendererLayerState {
    fn event(
        state: &mut Self,
        _proxy: &WpFractionalScaleV1,
        event: FractionalScaleEvent,
        index: &usize,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        let Some(output) = state.outputs.get_mut(*index) else {
            return;
        };
        if let FractionalScaleEvent::PreferredScale { scale } = event {
            output.preferred_fractional_scale = scale.max(FRACTIONAL_SCALE_DENOMINATOR);
        }
    }
}

impl Dispatch<WlSeat, ()> for RendererLayerState {
    fn event(
        state: &mut Self,
        seat: &WlSeat,
        event: wayland_client::protocol::wl_seat::Event,
        _data: &(),
        _conn: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wayland_client::protocol::wl_seat::Event::Capabilities { capabilities } = event {
            let has_pointer = matches!(
                capabilities,
                WEnum::Value(value)
                    if value.contains(wayland_client::protocol::wl_seat::Capability::Pointer)
            );
            if has_pointer && state._pointer.is_none() {
                state._pointer = Some(seat.get_pointer(qh, ()));
            } else if !has_pointer {
                state._pointer = None;
            }
        }
    }
}

impl Dispatch<WlPointer, ()> for RendererLayerState {
    fn event(
        state: &mut Self,
        _proxy: &WlPointer,
        event: wl_pointer::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        if !state.interactive {
            return;
        }

        match event {
            wl_pointer::Event::Enter { surface, surface_x, surface_y, .. } => {
                if let Some(index) = focused_output_index(state, &surface) {
                    state.focused_output = Some(index);
                    if let Some(output) = state.outputs.get_mut(index) {
                        output.pointer_x = surface_x;
                        output.pointer_y = surface_y;
                        if let Some((x, y)) = output.normalized_pointer() {
                            state
                                .pending_input_events
                                .push((index, InputEvent::PointerMove { x, y }));
                        }
                    }
                }
            }
            wl_pointer::Event::Leave { .. } => {
                state.focused_output = None;
            }
            wl_pointer::Event::Motion { surface_x, surface_y, .. } => {
                let Some(index) = state.focused_output else {
                    return;
                };
                if let Some(output) = state.outputs.get_mut(index) {
                    output.pointer_x = surface_x;
                    output.pointer_y = surface_y;
                    if let Some((x, y)) = output.normalized_pointer() {
                        state.pending_input_events.push((index, InputEvent::PointerMove { x, y }));
                    }
                }
            }
            wl_pointer::Event::Button { button, state: button_state, .. } => {
                let Some(index) = state.focused_output else {
                    return;
                };
                let Some(output) = state.outputs.get(index) else {
                    return;
                };
                let Some((x, y)) = output.normalized_pointer() else {
                    return;
                };
                let mapped_button = match button {
                    0x110 => 0,
                    0x111 => 1,
                    0x112 => 2,
                    _ => button as i32,
                };
                match button_state {
                    WEnum::Value(wl_pointer::ButtonState::Pressed) => {
                        state
                            .pending_input_events
                            .push((index, InputEvent::PointerDown { x, y, button: mapped_button }));
                    }
                    WEnum::Value(wl_pointer::ButtonState::Released) => {
                        state
                            .pending_input_events
                            .push((index, InputEvent::PointerUp { x, y, button: mapped_button }));
                    }
                    _ => {}
                }
            }
            wl_pointer::Event::Axis { axis, value, .. } => {
                let Some(index) = state.focused_output else {
                    return;
                };
                let Some(output) = state.outputs.get(index) else {
                    return;
                };
                let Some((x, y)) = output.normalized_pointer() else {
                    return;
                };
                let mut delta_x = 0;
                let mut delta_y = 0;
                match axis {
                    WEnum::Value(wl_pointer::Axis::VerticalScroll) => {
                        delta_y = value.round() as i32
                    }
                    WEnum::Value(wl_pointer::Axis::HorizontalScroll) => {
                        delta_x = value.round() as i32
                    }
                    _ => {}
                }
                if delta_x != 0 || delta_y != 0 {
                    state
                        .pending_input_events
                        .push((index, InputEvent::PointerWheel { x, y, delta_x, delta_y }));
                }
            }
            _ => {}
        }
    }
}

fn focused_output_index(state: &RendererLayerState, surface: &WlSurface) -> Option<usize> {
    let surface_id = surface.id();
    state
        .outputs
        .iter()
        .enumerate()
        .find(|(_, output)| output.name.ends_with(&surface_id.protocol_id().to_string()))
        .map(|(index, _)| index)
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

#[cfg(test)]
mod tests {
    use super::{expand_tilde, OutputState, FRACTIONAL_SCALE_DENOMINATOR};

    #[test]
    fn render_extent_prefers_output_mode() {
        let mut state = OutputState::new("output-1".to_string());
        state.output_mode_width = 2560;
        state.output_mode_height = 1440;
        state.logical_width = 1920;
        state.logical_height = 1080;

        assert_eq!(state.render_extent(), (2560, 1440));
    }

    #[test]
    fn render_extent_uses_fractional_scale_when_available() {
        let mut state = OutputState::new("output-1".to_string());
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
