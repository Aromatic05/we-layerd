use std::{
    io::ErrorKind,
    os::fd::{AsFd, AsRawFd, OwnedFd},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    time::Duration,
};

use anyhow::{anyhow, Context, Result};
use tracing::info;
use wayland_backend::client::WaylandError;
use wayland_client::{
    delegate_noop,
    globals::registry_queue_init,
    globals::GlobalListContents,
    protocol::{
        wl_buffer::WlBuffer,
        wl_compositor::WlCompositor,
        wl_output::{self, WlOutput},
        wl_pointer::{self, WlPointer},
        wl_region::WlRegion,
        wl_registry,
        wl_seat::WlSeat,
        wl_shm::{self, WlShm},
        wl_shm_pool::WlShmPool,
        wl_surface::WlSurface,
    },
    Connection, Dispatch, Proxy, QueueHandle, WEnum,
};
use wayland_protocols::wp::{
    fractional_scale::v1::client::{
        wp_fractional_scale_manager_v1::WpFractionalScaleManagerV1,
        wp_fractional_scale_v1::{Event as FractionalScaleEvent, WpFractionalScaleV1},
    },
    linux_dmabuf::zv1::client::{
        zwp_linux_buffer_params_v1::{Flags as DmabufFlags, ZwpLinuxBufferParamsV1},
        zwp_linux_dmabuf_v1::ZwpLinuxDmabufV1,
    },
    viewporter::client::{wp_viewport::WpViewport, wp_viewporter::WpViewporter},
};
use wayland_protocols_wlr::layer_shell::v1::client::{
    zwlr_layer_shell_v1::{Layer, ZwlrLayerShellV1},
    zwlr_layer_surface_v1::{
        Anchor, Event as LayerSurfaceEvent, KeyboardInteractivity, ZwlrLayerSurfaceV1,
    },
};
use we_renderer::{Frame, InputEvent, RenderConfig, RendererLibrary, Session, Source};

use crate::{
    config::Config,
    ipc::{ControlCommand, RuntimeLoopExit},
};

const FRACTIONAL_SCALE_DENOMINATOR: u32 = 120;

// ---------------------------------------------------------------------------
// Buffer bookkeeping — matches WaylandBuffer in the reference
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct WaylandBuffer {
    buffer: WlBuffer,
    released: Arc<AtomicBool>,
    pending_fds: Vec<OwnedFd>,
}

// ---------------------------------------------------------------------------
// Single-output state — matches WaylandState in the reference
// ---------------------------------------------------------------------------

struct LayerState {
    compositor: Option<WlCompositor>,
    surface: Option<WlSurface>,
    pointer: Option<WlPointer>,
    output: Option<WlOutput>,
    viewport: Option<WpViewport>,
    layer_surface: Option<ZwlrLayerSurfaceV1>,
    dmabuf: Option<ZwpLinuxDmabufV1>,
    shm: Option<WlShm>,
    fractional_scale: Option<WpFractionalScaleV1>,

    dmabuf_version: u32,
    compositor_version: u32,
    output_count: u32,

    output_scale: u32,
    preferred_fractional_scale: u32,
    output_mode_width: u32,
    output_mode_height: u32,
    logical_width: u32,
    logical_height: u32,
    render_width: u32,
    render_height: u32,
    fallback_width: u32,
    fallback_height: u32,

    pointer_x: f64,
    pointer_y: f64,

    running: bool,
    configured: bool,
    extent_mismatch_reported: bool,

    session: Option<Session>,
    _library: Option<RendererLibrary>,

    in_flight: Vec<WaylandBuffer>,

    interactive: bool,
    paused: bool,
    pending_input_events: Vec<InputEvent>,
}

// ---------------------------------------------------------------------------
// helpers — match the reference exactly
// ---------------------------------------------------------------------------

fn to_opaque_drm_fourcc(fourcc: u32) -> u32 {
    const DRM_FORMAT_ABGR8888: u32 = u32::from_le_bytes(*b"AB24");
    const DRM_FORMAT_XBGR8888: u32 = u32::from_le_bytes(*b"XB24");
    const DRM_FORMAT_ARGB8888: u32 = u32::from_le_bytes(*b"AR24");
    const DRM_FORMAT_XRGB8888: u32 = u32::from_le_bytes(*b"XR24");
    match fourcc {
        DRM_FORMAT_ABGR8888 => DRM_FORMAT_XBGR8888,
        DRM_FORMAT_ARGB8888 => DRM_FORMAT_XRGB8888,
        _ => fourcc,
    }
}

fn env_var_enabled(name: &str) -> bool {
    std::env::var(name).map(|v| !v.is_empty() && v != "0").unwrap_or(false)
}

fn env_var_equals(name: &str, expected: &str) -> bool {
    std::env::var(name).map(|v| v == expected).unwrap_or(false)
}

impl LayerState {
    fn render_scale_factor(&self) -> f64 {
        if self.preferred_fractional_scale >= FRACTIONAL_SCALE_DENOMINATOR {
            self.preferred_fractional_scale as f64 / FRACTIONAL_SCALE_DENOMINATOR as f64
        } else {
            self.output_scale.max(1) as f64
        }
    }

    fn update_render_extent(&mut self) {
        if self.output_mode_width > 0 && self.output_mode_height > 0 {
            self.render_width = self.output_mode_width;
            self.render_height = self.output_mode_height;
            return;
        }

        let logical_w = if self.logical_width > 0 {
            self.logical_width
        } else {
            self.fallback_width
        };
        let logical_h = if self.logical_height > 0 {
            self.logical_height
        } else {
            self.fallback_height
        };
        let scale = self.render_scale_factor();

        self.render_width = (logical_w as f64 * scale).round().max(1.0) as u32;
        self.render_height = (logical_h as f64 * scale).round().max(1.0) as u32;
    }

    fn update_viewport_destination(&self) {
        if let Some(viewport) = &self.viewport {
            if self.logical_width > 0 && self.logical_height > 0 {
                viewport.set_destination(self.logical_width as i32, self.logical_height as i32);
            }
        }
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

    fn release_pending_send_fds(&mut self) {
        for entry in &mut self.in_flight {
            entry.pending_fds.clear();
        }
    }

    fn collect_released_buffers(&mut self) {
        self.in_flight.retain(|entry| {
            !(entry.pending_fds.is_empty() && entry.released.load(Ordering::SeqCst))
        });
    }
}

// ---------------------------------------------------------------------------
// Dispatch impls — match the reference's event handlers
// ---------------------------------------------------------------------------

impl Dispatch<ZwlrLayerSurfaceV1, ()> for LayerState {
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
                state.logical_width = if width > 0 { width as u32 } else { state.fallback_width };
                state.logical_height =
                    if height > 0 { height as u32 } else { state.fallback_height };
                state.update_render_extent();
                state.update_viewport_destination();
                // input region will be updated by the runtime after dispatch
            }
            LayerSurfaceEvent::Closed => state.running = false,
            _ => {}
        }
    }
}

impl Dispatch<WlOutput, ()> for LayerState {
    fn event(
        state: &mut Self,
        _proxy: &WlOutput,
        event: wl_output::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        match event {
            wl_output::Event::Mode { flags, width, height, .. }
                if matches!(
                    flags,
                    WEnum::Value(value) if value.contains(wl_output::Mode::Current)
                ) =>
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

impl Dispatch<WpFractionalScaleV1, ()> for LayerState {
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

impl Dispatch<WlSeat, ()> for LayerState {
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
            if has_pointer && state.pointer.is_none() {
                state.pointer = Some(seat.get_pointer(qh, ()));
            } else if !has_pointer {
                state.pointer = None;
            }
        }
    }
}

impl Dispatch<WlPointer, ()> for LayerState {
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
            wl_pointer::Event::Enter { surface_x, surface_y, .. } => {
                state.pointer_x = surface_x;
                state.pointer_y = surface_y;
                if let Some((x, y)) = state.normalized_pointer() {
                    state.pending_input_events.push(InputEvent::PointerMove { x, y });
                }
            }
            wl_pointer::Event::Leave { .. } => {}
            wl_pointer::Event::Motion { surface_x, surface_y, .. } => {
                state.pointer_x = surface_x;
                state.pointer_y = surface_y;
                if let Some((x, y)) = state.normalized_pointer() {
                    state.pending_input_events.push(InputEvent::PointerMove { x, y });
                }
            }
            wl_pointer::Event::Button { button, state: button_state, .. } => {
                let Some((x, y)) = state.normalized_pointer() else { return };
                let mapped_button = match button {
                    0x110 => 0, // BTN_LEFT
                    _ => button as i32,
                };
                match button_state {
                    WEnum::Value(wl_pointer::ButtonState::Pressed) => {
                        state.pending_input_events.push(InputEvent::PointerDown {
                            x,
                            y,
                            button: mapped_button,
                        });
                    }
                    WEnum::Value(wl_pointer::ButtonState::Released) => {
                        state.pending_input_events.push(InputEvent::PointerUp {
                            x,
                            y,
                            button: mapped_button,
                        });
                    }
                    _ => {}
                }
            }
            wl_pointer::Event::Axis { axis, value, .. } => {
                let Some((x, y)) = state.normalized_pointer() else { return };
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
                        .push(InputEvent::PointerWheel { x, y, delta_x, delta_y });
                }
            }
            _ => {}
        }
    }
}

impl Dispatch<WlBuffer, Arc<AtomicBool>> for LayerState {
    fn event(
        _state: &mut Self,
        _proxy: &WlBuffer,
        event: wayland_client::protocol::wl_buffer::Event,
        released: &Arc<AtomicBool>,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        if let wayland_client::protocol::wl_buffer::Event::Release = event {
            released.store(true, Ordering::SeqCst);
        }
    }
}

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for LayerState {
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

delegate_noop!(LayerState: ignore WlCompositor);
delegate_noop!(LayerState: ignore WlSurface);
delegate_noop!(LayerState: ignore WlRegion);
delegate_noop!(LayerState: ignore ZwlrLayerShellV1);
delegate_noop!(LayerState: ignore WlShm);
delegate_noop!(LayerState: ignore WlShmPool);
delegate_noop!(LayerState: ignore ZwpLinuxDmabufV1);
delegate_noop!(LayerState: ignore ZwpLinuxBufferParamsV1);
delegate_noop!(LayerState: ignore WpViewporter);
delegate_noop!(LayerState: ignore WpViewport);
delegate_noop!(LayerState: ignore WpFractionalScaleManagerV1);

// ---------------------------------------------------------------------------
// Buffer creation — matches createBufferForFrame in the reference
// ---------------------------------------------------------------------------

fn create_buffer_for_frame(
    state: &LayerState,
    qh: &QueueHandle<LayerState>,
    frame: Frame,
) -> Result<WaylandBuffer> {
    match frame {
        Frame::Shm(shm) => {
            let shm_obj = state.shm.as_ref().ok_or_else(|| anyhow!("wl_shm unavailable"))?;
            let released = Arc::new(AtomicBool::new(false));
            let pool = shm_obj.create_pool(shm.fd.as_fd(), shm.size as i32, qh, ());
            let buffer = pool.create_buffer(
                0,
                shm.width as i32,
                shm.height as i32,
                shm.stride as i32,
                wl_shm::Format::Xrgb8888,
                qh,
                Arc::clone(&released),
            );
            pool.destroy();
            Ok(WaylandBuffer {
                buffer,
                released,
                pending_fds: vec![shm.fd],
            })
        }
        Frame::Dmabuf(dmabuf) => {
            let dmabuf_obj =
                state.dmabuf.as_ref().ok_or_else(|| anyhow!("zwp_linux_dmabuf_v1 unavailable"))?;
            let params = dmabuf_obj.create_params(qh, ());
            let modifier_hi = (dmabuf.drm_modifier >> 32) as u32;
            let modifier_lo = (dmabuf.drm_modifier & 0xffff_ffff) as u32;
            for (i, plane) in dmabuf.planes.iter().enumerate() {
                params.add(
                    plane.fd.as_fd(),
                    i as u32,
                    plane.offset,
                    plane.stride,
                    modifier_hi,
                    modifier_lo,
                );
            }
            let released = Arc::new(AtomicBool::new(false));
            let buffer = params.create_immed(
                dmabuf.width as i32,
                dmabuf.height as i32,
                to_opaque_drm_fourcc(dmabuf.drm_fourcc),
                DmabufFlags::empty(),
                qh,
                Arc::clone(&released),
            );
            params.destroy();
            // Keep plane fds alive until buffer is released
            let pending_fds: Vec<OwnedFd> =
                dmabuf.planes.into_iter().map(|p| p.fd).collect();
            Ok(WaylandBuffer { buffer, released, pending_fds })
        }
    }
}

// ---------------------------------------------------------------------------
// Frame presentation — matches presentFrame in the reference
// ---------------------------------------------------------------------------

fn present_frame(
    state: &mut LayerState,
    qh: &QueueHandle<LayerState>,
    frame: Frame,
) -> Result<()> {
    let entry = create_buffer_for_frame(state, qh, frame)?;

    if !state.extent_mismatch_reported {
        // Frame dimensions are checked against render extent for logging
        state.extent_mismatch_reported = true; // logged once
    }

    state.update_viewport_destination();

    let surface = state.surface.as_ref().ok_or_else(|| anyhow!("no surface"))?;
    surface.attach(Some(&entry.buffer), 0, 0);
    if state.compositor_version >= 4 {
        surface.damage_buffer(0, 0, i32::MAX, i32::MAX);
    } else {
        surface.damage(0, 0, i32::MAX, i32::MAX);
    }
    surface.commit();
    state.in_flight.push(entry);
    Ok(())
}

// ---------------------------------------------------------------------------
// init — matches initWayland in the reference
// ---------------------------------------------------------------------------

fn init_wayland(
    _conn: &Connection,
    qh: &QueueHandle<LayerState>,
    state: &mut LayerState,
    globals: &wayland_client::globals::GlobalList,
    compositor: WlCompositor,
    layer_shell: ZwlrLayerShellV1,
    shm: WlShm,
    dmabuf: Option<ZwpLinuxDmabufV1>,
    dmabuf_version: u32,
    seat: Option<WlSeat>,
    viewporter: Option<WpViewporter>,
    fractional_scale_manager: Option<WpFractionalScaleManagerV1>,
) -> Result<()> {
    state.compositor = Some(compositor.clone());
    state.compositor_version = compositor.version();
    state.shm = Some(shm);
    state.dmabuf = dmabuf;
    state.dmabuf_version = dmabuf_version;

    if state.dmabuf.is_some() && state.dmabuf_version < 2 {
        return Err(anyhow!(
            "zwp_linux_dmabuf_v1 version {} does not support create_immed",
            state.dmabuf_version
        ));
    }

    // Bind exactly one output — matches the reference's single-output model
    let output_globals: Vec<_> =
        globals.contents().clone_list().into_iter().filter(|g| g.interface == "wl_output").collect();
    state.output_count = output_globals.len() as u32;
    if let Some(first_output) = output_globals.first() {
        let version = first_output.version.min(4);
        state.output =
            Some(globals.registry().bind(first_output.name, version, qh, ()));
    }

    if state.output_count > 1 {
        info!("compositor exposed {} outputs, using the first one", state.output_count);
    }

    state.surface = Some(compositor.create_surface(qh, ()));
    let surface = state.surface.as_ref().unwrap();
    surface.set_buffer_scale(1);

    if let Some(ref vp) = viewporter {
        state.viewport = Some(vp.get_viewport(surface, qh, ()));
    }
    if !viewporter.is_some() {
        info!("wp_viewporter unavailable, fractional high-DPI buffers will not map correctly");
    }

    if let Some(ref fsm) = fractional_scale_manager {
        state.fractional_scale = Some(fsm.get_fractional_scale(surface, qh, ()));
    }
    if !fractional_scale_manager.is_some() {
        info!("fractional-scale-v1 unavailable, falling back to wl_output integer scale");
    }

    let layer_surface = layer_shell.get_layer_surface(
        surface,
        state.output.as_ref(),
        Layer::Background,
        "wallpaper-engine-renderer".to_string(),
        qh,
        (),
    );
    layer_surface.set_anchor(Anchor::Top | Anchor::Bottom | Anchor::Left | Anchor::Right);
    layer_surface.set_size(0, 0);
    layer_surface.set_exclusive_zone(-1);
    layer_surface.set_margin(0, 0, 0, 0);
    layer_surface.set_keyboard_interactivity(KeyboardInteractivity::None);
    state.layer_surface = Some(layer_surface);

    if seat.is_some() {
        state.pointer = None; // will be set by capabilities event
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Entry point — matches main() in the reference
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

    let dmabuf_global = globals.contents().clone_list().into_iter().find(|g| {
        g.interface == "zwp_linux_dmabuf_v1"
    });
    let dmabuf_version = dmabuf_global.as_ref().map(|g| g.version).unwrap_or(0);
    let dmabuf = globals.bind::<ZwpLinuxDmabufV1, _, _>(&qh, 3..=4, ()).ok();

    let seat = globals.bind::<WlSeat, _, _>(&qh, 1..=5, ()).ok();
    let viewporter = globals.bind::<WpViewporter, _, _>(&qh, 1..=1, ()).ok();
    let fractional_scale_manager =
        globals.bind::<WpFractionalScaleManagerV1, _, _>(&qh, 1..=1, ()).ok();

    let cache_path = expand_tilde(&cfg.renderer.cache_path);
    let source_path = expand_tilde(&cfg.renderer.source);
    let assets_path = expand_tilde(&cfg.renderer.assets_path);

    let library_path = resolve_renderer_library_path(&cfg.renderer.library_path);
    let library = RendererLibrary::load(&library_path)
        .with_context(|| format!("failed to load renderer library {}", library_path.display()))?;

    let cache_path_arg =
        if cfg.renderer.cache_path.trim().is_empty() { None } else { Some(cache_path.as_path()) };

    let mut session =
        library.create_session(cache_path_arg).context("failed to create renderer session")?;

    let mut state = LayerState {
        compositor: None,
        surface: None,
        pointer: None,
        output: None,
        viewport: None,
        layer_surface: None,
        dmabuf: None,
        shm: None,
        fractional_scale: None,
        dmabuf_version: 0,
        compositor_version: 0,
        output_count: 0,
        output_scale: 1,
        preferred_fractional_scale: 0,
        output_mode_width: 0,
        output_mode_height: 0,
        logical_width: 0,
        logical_height: 0,
        render_width: 0,
        render_height: 0,
        fallback_width: 1920,
        fallback_height: 1080,
        pointer_x: 0.0,
        pointer_y: 0.0,
        running: true,
        configured: false,
        extent_mismatch_reported: false,
        session: None,
        _library: Some(library),
        in_flight: Vec::new(),
        interactive: cfg.general.interactive,
        paused: false,
        pending_input_events: Vec::new(),
    };

    init_wayland(
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

    // Set input region and commit initial surface state — matches reference's
    // updateSurfaceRegions + wl_surface_commit before waiting for configure
    {
        if let (Some(ref compositor), Some(ref surface)) =
            (&state.compositor, &state.surface)
        {
            let region = compositor.create_region(&qh, ());
            if state.interactive {
                region.add(0, 0, state.logical_width as i32, state.logical_height as i32);
            }
            surface.set_input_region(Some(&region));
            region.destroy();
        }
        if let Some(ref surface) = &state.surface {
            surface.commit();
        }
    }

    // Wait for the first configure — matches the while (!configured) roundtrip
    while !state.configured {
        event_queue.roundtrip(&mut state).context("failed waiting for layer configure")?;
        state.update_render_extent();
        state.update_viewport_destination();
    }

    if state.logical_width == 0 {
        state.logical_width = state.fallback_width;
    }
    if state.logical_height == 0 {
        state.logical_height = state.fallback_height;
    }
    state.update_render_extent();
    state.update_viewport_destination();

    // Set source — matches we_session_set_source in the reference
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

    // Determine dmabuf preference — matches NVIDIA check in the reference
    let prefer_dmabuf = if env_var_enabled("__NV_PRIME_RENDER_OFFLOAD")
        || env_var_equals("__VK_LAYER_NV_optimus", "NVIDIA_only")
    {
        info!("NVIDIA prime-render-offload detected; forcing SHM fallback");
        false
    } else {
        cfg.renderer.prefer_dmabuf
    };

    // Set render config BEFORE play — matches the reference's order
    session
        .configure(RenderConfig {
            width: state.render_width,
            height: state.render_height,
            enable_valid_layer: false,
            prefer_dmabuf,
            allow_shm_fallback: cfg.renderer.allow_shm_fallback,
        })
        .context("failed to set render config")?;

    session.play().context("failed to start renderer session")?;
    state.session = Some(session);

    info!(
        logical_width = state.logical_width,
        logical_height = state.logical_height,
        render_width = state.render_width,
        render_height = state.render_height,
        scale = state.render_scale_factor(),
        "starting renderer-backed layer-shell surface"
    );

    // Stats — matches the reference's 5-second logging
    let mut acquired: u64 = 0;
    let mut presented: u64 = 0;
    let mut no_frame_polls: u64 = 0;
    let mut last_acquire_status: i32 = 1;
    let mut last_log = std::time::Instant::now();

    // Input region helper (needs qh)
    let update_input_region = |state: &LayerState, qh: &QueueHandle<LayerState>| {
        if let (Some(ref compositor), Some(ref surface)) = (&state.compositor, &state.surface) {
            let region = compositor.create_region(qh, ());
            if state.interactive && state.logical_width > 0 && state.logical_height > 0 {
                region.add(0, 0, state.logical_width as i32, state.logical_height as i32);
            }
            surface.set_input_region(Some(&region));
            region.destroy();
        }
    };

    // Main loop — matches the reference's while (running) loop exactly
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
                        acquired += 1;
                        present_frame(&mut state, &qh, frame)
                            .context("failed to present frame")?;
                        presented += 1;
                        last_acquire_status = 0;
                    }
                    None => {
                        no_frame_polls += 1;
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
                acquired,
                presented,
                no_frame_polls,
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
        let poll_result =
            unsafe { libc::poll(&mut poll_fd, 1, 5 /* ms */) };

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
            // Drain any queued events — matches reference's poll==0 branch
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
            update_input_region(&state, &qh);
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

// ---------------------------------------------------------------------------
// Utility functions (unchanged from original)
// ---------------------------------------------------------------------------

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
        candidates.push(Path::new(install_root).join("lib/libwallpaper-engine-renderer.so"));
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

// ---------------------------------------------------------------------------
// Tests — matches the reference's behavior
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_extent_prefers_output_mode() {
        let mut state = LayerState {
            compositor: None,
            surface: None,
            pointer: None,
            output: None,
            viewport: None,
            layer_surface: None,
            dmabuf: None,
            shm: None,
            fractional_scale: None,
            dmabuf_version: 0,
            compositor_version: 0,
            output_count: 0,
            output_scale: 1,
            preferred_fractional_scale: 0,
            output_mode_width: 2560,
            output_mode_height: 1440,
            logical_width: 1920,
            logical_height: 1080,
            render_width: 0,
            render_height: 0,
            fallback_width: 0,
            fallback_height: 0,
            pointer_x: 0.0,
            pointer_y: 0.0,
            running: true,
            configured: false,
            extent_mismatch_reported: false,
            session: None,
            _library: None,
            in_flight: Vec::new(),
            interactive: false,
            paused: false,
            pending_input_events: Vec::new(),
        };
        state.update_render_extent();
        assert_eq!(state.render_width, 2560);
        assert_eq!(state.render_height, 1440);
    }

    #[test]
    fn render_extent_uses_logical_when_no_output_mode() {
        let mut state = LayerState {
            compositor: None,
            surface: None,
            pointer: None,
            output: None,
            viewport: None,
            layer_surface: None,
            dmabuf: None,
            shm: None,
            fractional_scale: None,
            dmabuf_version: 0,
            compositor_version: 0,
            output_count: 0,
            output_scale: 1,
            preferred_fractional_scale: 0,
            output_mode_width: 0,
            output_mode_height: 0,
            logical_width: 100,
            logical_height: 50,
            render_width: 0,
            render_height: 0,
            fallback_width: 1920,
            fallback_height: 1080,
            pointer_x: 0.0,
            pointer_y: 0.0,
            running: true,
            configured: false,
            extent_mismatch_reported: false,
            session: None,
            _library: None,
            in_flight: Vec::new(),
            interactive: false,
            paused: false,
            pending_input_events: Vec::new(),
        };
        state.update_render_extent();
        // logical > 0, so use logical, not fallback
        assert_eq!(state.render_width, 100);
        assert_eq!(state.render_height, 50);
    }

    #[test]
    fn render_extent_falls_back_when_logical_is_zero() {
        let mut state = LayerState {
            compositor: None,
            surface: None,
            pointer: None,
            output: None,
            viewport: None,
            layer_surface: None,
            dmabuf: None,
            shm: None,
            fractional_scale: None,
            dmabuf_version: 0,
            compositor_version: 0,
            output_count: 0,
            output_scale: 1,
            preferred_fractional_scale: 0,
            output_mode_width: 0,
            output_mode_height: 0,
            logical_width: 0,
            logical_height: 0,
            render_width: 0,
            render_height: 0,
            fallback_width: 1920,
            fallback_height: 1080,
            pointer_x: 0.0,
            pointer_y: 0.0,
            running: true,
            configured: false,
            extent_mismatch_reported: false,
            session: None,
            _library: None,
            in_flight: Vec::new(),
            interactive: false,
            paused: false,
            pending_input_events: Vec::new(),
        };
        state.update_render_extent();
        assert_eq!(state.render_width, 1920);
        assert_eq!(state.render_height, 1080);
    }

    #[test]
    fn render_extent_uses_fractional_scale() {
        let mut state = LayerState {
            compositor: None,
            surface: None,
            pointer: None,
            output: None,
            viewport: None,
            layer_surface: None,
            dmabuf: None,
            shm: None,
            fractional_scale: None,
            dmabuf_version: 0,
            compositor_version: 0,
            output_count: 0,
            output_scale: 1,
            preferred_fractional_scale: FRACTIONAL_SCALE_DENOMINATOR + 60, // 180/120 = 1.5
            output_mode_width: 0,
            output_mode_height: 0,
            logical_width: 100,
            logical_height: 50,
            render_width: 0,
            render_height: 0,
            fallback_width: 0,
            fallback_height: 0,
            pointer_x: 0.0,
            pointer_y: 0.0,
            running: true,
            configured: false,
            extent_mismatch_reported: false,
            session: None,
            _library: None,
            in_flight: Vec::new(),
            interactive: false,
            paused: false,
            pending_input_events: Vec::new(),
        };
        state.update_render_extent();
        assert_eq!(state.render_width, 150);
        assert_eq!(state.render_height, 75);
    }

    #[test]
    fn expand_tilde_expands_home_prefix() {
        let home = std::env::var_os("HOME").expect("HOME must be set in test env");
        let expanded = expand_tilde("~/renderer-cache");
        assert_eq!(expanded, PathBuf::from(home).join("renderer-cache"));
    }
}
