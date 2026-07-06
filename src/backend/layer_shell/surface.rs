use std::os::fd::{AsFd, OwnedFd};

use anyhow::{anyhow, Result};
use wayland_client::{
    delegate_noop,
    protocol::{
        wl_buffer::WlBuffer,
        wl_callback::{Event as CallbackEvent, WlCallback},
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
    fractional_scale::v1::client::wp_fractional_scale_v1::{
        Event as FractionalScaleEvent, WpFractionalScaleV1,
    },
    linux_dmabuf::zv1::client::{
        zwp_linux_buffer_params_v1::{Flags as DmabufFlags, ZwpLinuxBufferParamsV1},
        zwp_linux_dmabuf_v1::ZwpLinuxDmabufV1,
    },
    viewporter::client::{wp_viewport::WpViewport, wp_viewporter::WpViewporter},
};
use wayland_protocols_wlr::layer_shell::v1::client::{
    zwlr_layer_shell_v1::ZwlrLayerShellV1,
    zwlr_layer_surface_v1::{Event as LayerSurfaceEvent, ZwlrLayerSurfaceV1},
};
use we_renderer::Frame;

use crate::backend::{
    layer_shell::state::{LayerShellState, WaylandBuffer},
    wayland_common::output::FRACTIONAL_SCALE_DENOMINATOR,
};

// ---------------------------------------------------------------------------
// DRM format helper
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

// ---------------------------------------------------------------------------
// Dispatch impls
// ---------------------------------------------------------------------------

impl Dispatch<ZwlrLayerSurfaceV1, ()> for LayerShellState {
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
                state.output.logical_width =
                    if width > 0 { width } else { state.output.fallback_width };
                state.output.logical_height =
                    if height > 0 { height } else { state.output.fallback_height };
                state.update_render_extent();
                state.update_viewport_destination();
            }
            LayerSurfaceEvent::Closed => state.running = false,
            _ => {}
        }
    }
}

impl Dispatch<WlOutput, ()> for LayerShellState {
    fn event(
        state: &mut Self,
        _proxy: &WlOutput,
        event: wl_output::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        match event {
            wl_output::Event::Mode { flags: WEnum::Value(value), width, height, .. }
                if value.contains(wl_output::Mode::Current) =>
            {
                state.output.output_mode_width = width.max(0) as u32;
                state.output.output_mode_height = height.max(0) as u32;
                state.update_render_extent();
            }
            wl_output::Event::Scale { factor } => {
                state.output.output_scale = factor.max(1) as u32;
                state.update_render_extent();
            }
            _ => {}
        }
    }
}

impl Dispatch<WpFractionalScaleV1, ()> for LayerShellState {
    fn event(
        state: &mut Self,
        _proxy: &WpFractionalScaleV1,
        event: FractionalScaleEvent,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        if let FractionalScaleEvent::PreferredScale { scale } = event {
            state.output.preferred_fractional_scale = scale.max(FRACTIONAL_SCALE_DENOMINATOR);
            state.update_render_extent();
        }
    }
}

impl Dispatch<WlSeat, ()> for LayerShellState {
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
            if has_pointer && state.objects.pointer.is_none() {
                state.objects.pointer = Some(seat.get_pointer(qh, ()));
            } else if !has_pointer {
                state.objects.pointer = None;
            }
        }
    }
}

impl Dispatch<WlPointer, ()> for LayerShellState {
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
                state.output.pointer_x = surface_x;
                state.output.pointer_y = surface_y;
                if let Some((x, y)) = state.normalized_pointer() {
                    state.pending_input_events.push(we_renderer::InputEvent::PointerMove { x, y });
                }
            }
            wl_pointer::Event::Leave { .. } => {}
            wl_pointer::Event::Motion { surface_x, surface_y, .. } => {
                state.output.pointer_x = surface_x;
                state.output.pointer_y = surface_y;
                if let Some((x, y)) = state.normalized_pointer() {
                    state.pending_input_events.push(we_renderer::InputEvent::PointerMove { x, y });
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
                        state.pending_input_events.push(we_renderer::InputEvent::PointerDown {
                            x,
                            y,
                            button: mapped_button,
                        });
                    }
                    WEnum::Value(wl_pointer::ButtonState::Released) => {
                        state.pending_input_events.push(we_renderer::InputEvent::PointerUp {
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
                    state.pending_input_events.push(we_renderer::InputEvent::PointerWheel {
                        x,
                        y,
                        delta_x,
                        delta_y,
                    });
                }
            }
            _ => {}
        }
    }
}

impl Dispatch<WlCallback, ()> for LayerShellState {
    fn event(
        state: &mut Self,
        callback: &WlCallback,
        event: CallbackEvent,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        if let CallbackEvent::Done { callback_data } = event {
            state.frame_callback.pending = false;
            state.frame_callback.ready_for_next_frame = true;
            state.frame_callback.last_done_msec = Some(callback_data);
            if state
                .objects
                .frame_callback
                .as_ref()
                .map(|current| current.id() == callback.id())
                .unwrap_or(false)
            {
                state.objects.frame_callback = None;
            }
        }
    }
}

impl Dispatch<WlBuffer, std::sync::Arc<std::sync::atomic::AtomicBool>> for LayerShellState {
    fn event(
        _state: &mut Self,
        _proxy: &WlBuffer,
        event: wayland_client::protocol::wl_buffer::Event,
        released: &std::sync::Arc<std::sync::atomic::AtomicBool>,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        if let wayland_client::protocol::wl_buffer::Event::Release = event {
            released.store(true, std::sync::atomic::Ordering::SeqCst);
        }
    }
}

impl Dispatch<wl_registry::WlRegistry, wayland_client::globals::GlobalListContents>
    for LayerShellState
{
    fn event(
        _state: &mut Self,
        _proxy: &wl_registry::WlRegistry,
        _event: wl_registry::Event,
        _data: &wayland_client::globals::GlobalListContents,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
    }
}

delegate_noop!(LayerShellState: ignore WlCompositor);
delegate_noop!(LayerShellState: ignore WlSurface);
delegate_noop!(LayerShellState: ignore WlRegion);
delegate_noop!(LayerShellState: ignore ZwlrLayerShellV1);
delegate_noop!(LayerShellState: ignore WlShm);
delegate_noop!(LayerShellState: ignore WlShmPool);
delegate_noop!(LayerShellState: ignore ZwpLinuxDmabufV1);
delegate_noop!(LayerShellState: ignore ZwpLinuxBufferParamsV1);
delegate_noop!(LayerShellState: ignore WpViewporter);
delegate_noop!(LayerShellState: ignore WpViewport);
delegate_noop!(
    LayerShellState: ignore wayland_protocols::wp::fractional_scale::v1::client::wp_fractional_scale_manager_v1::WpFractionalScaleManagerV1
);

// ---------------------------------------------------------------------------
// Buffer creation
// ---------------------------------------------------------------------------

pub(super) fn create_buffer_for_frame(
    state: &LayerShellState,
    qh: &QueueHandle<LayerShellState>,
    frame: Frame,
) -> Result<WaylandBuffer> {
    match frame {
        Frame::Shm(shm) => {
            let shm_obj =
                state.objects.shm.as_ref().ok_or_else(|| anyhow!("wl_shm unavailable"))?;
            let released = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            let pool = shm_obj.create_pool(shm.fd.as_fd(), shm.size as i32, qh, ());
            let buffer = pool.create_buffer(
                0,
                shm.width as i32,
                shm.height as i32,
                shm.stride as i32,
                wl_shm::Format::Xrgb8888,
                qh,
                std::sync::Arc::clone(&released),
            );
            pool.destroy();
            Ok(WaylandBuffer { buffer, released, pending_fds: vec![shm.fd] })
        }
        Frame::Dmabuf(dmabuf) => {
            let dmabuf_obj = state
                .objects
                .dmabuf
                .as_ref()
                .ok_or_else(|| anyhow!("zwp_linux_dmabuf_v1 unavailable"))?;
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
            let released = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            let buffer = params.create_immed(
                dmabuf.width as i32,
                dmabuf.height as i32,
                to_opaque_drm_fourcc(dmabuf.drm_fourcc),
                DmabufFlags::empty(),
                qh,
                std::sync::Arc::clone(&released),
            );
            params.destroy();
            let pending_fds: Vec<OwnedFd> = dmabuf.planes.into_iter().map(|p| p.fd).collect();
            Ok(WaylandBuffer { buffer, released, pending_fds })
        }
    }
}

// ---------------------------------------------------------------------------
// Frame presentation
// ---------------------------------------------------------------------------

pub(super) fn present_frame(
    state: &mut LayerShellState,
    qh: &QueueHandle<LayerShellState>,
    frame: Frame,
) -> Result<()> {
    let entry = create_buffer_for_frame(state, qh, frame)?;
    state.update_viewport_destination();
    let surface = state.objects.surface.as_ref().ok_or_else(|| anyhow!("no surface"))?;
    surface.attach(Some(&entry.buffer), 0, 0);
    if state.compositor_version >= 4 {
        surface.damage_buffer(0, 0, i32::MAX, i32::MAX);
    } else {
        surface.damage(0, 0, i32::MAX, i32::MAX);
    }
    let callback = surface.frame(qh, ());
    surface.commit();
    state.objects.frame_callback = Some(callback);
    state.frame_callback.pending = true;
    state.frame_callback.ready_for_next_frame = false;
    state.frame_stats.presented = state.frame_stats.presented.saturating_add(1);
    state.frame_stats.in_flight_count = state.buffers.in_flight.len() + 1;
    state.buffers.in_flight.push(entry);
    Ok(())
}

pub(super) fn begin_stop_teardown(state: &mut LayerShellState) -> Result<()> {
    state.stopping = true;
    state.paused = true;
    state.frame_callback.pending = false;
    state.frame_callback.ready_for_next_frame = false;
    state.objects.frame_callback = None;
    state.pending_input_events.clear();

    let surface = state.objects.surface.as_ref().ok_or_else(|| anyhow!("no surface"))?;
    surface.attach(None, 0, 0);
    surface.commit();
    Ok(())
}

// ---------------------------------------------------------------------------
// Wayland init
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
pub(super) fn init_wayland(
    _conn: &Connection,
    qh: &QueueHandle<LayerShellState>,
    state: &mut LayerShellState,
    globals: &wayland_client::globals::GlobalList,
    compositor: WlCompositor,
    layer_shell: Option<ZwlrLayerShellV1>,
    shm: WlShm,
    dmabuf: Option<ZwpLinuxDmabufV1>,
    dmabuf_version: u32,
    seat: Option<WlSeat>,
    viewporter: Option<WpViewporter>,
    fractional_scale_manager: Option<wayland_protocols::wp::fractional_scale::v1::client::wp_fractional_scale_manager_v1::WpFractionalScaleManagerV1>,
) -> Result<()> {
    state.objects.compositor = Some(compositor.clone());
    state.compositor_version = compositor.version();
    state.objects.shm = Some(shm);
    state.objects.dmabuf = dmabuf;
    state.dmabuf_version = dmabuf_version;
    state.diagnostics.wayland_connected = true;
    state.diagnostics.dmabuf_global_available = state.objects.dmabuf.is_some();
    state.diagnostics.dmabuf_global_version = dmabuf_version;
    state.diagnostics.shm_available = true;
    state.diagnostics.viewporter_available = viewporter.is_some();
    state.diagnostics.fractional_scale_available = fractional_scale_manager.is_some();

    if state.objects.dmabuf.is_some() && state.dmabuf_version < 2 {
        return Err(anyhow!(
            "zwp_linux_dmabuf_v1 version {} does not support create_immed",
            state.dmabuf_version
        ));
    }

    let output_globals: Vec<_> = globals
        .contents()
        .clone_list()
        .into_iter()
        .filter(|g| g.interface == "wl_output")
        .collect();
    state.output_count = output_globals.len() as u32;
    if let Some(first_output) = output_globals.first() {
        let version = first_output.version.min(4);
        state.objects.output = Some(globals.registry().bind(first_output.name, version, qh, ()));
    }

    if state.output_count > 1 {
        tracing::info!("compositor exposed {} outputs, using the first one", state.output_count);
    }

    state.objects.surface = Some(compositor.create_surface(qh, ()));
    let surface = state.objects.surface.as_ref().unwrap();
    surface.set_buffer_scale(1);

    if let Some(ref vp) = viewporter {
        state.objects.viewport = Some(vp.get_viewport(surface, qh, ()));
    }
    if viewporter.is_none() {
        tracing::info!(
            "wp_viewporter unavailable, fractional high-DPI buffers will not map correctly"
        );
    }

    if let Some(ref fsm) = fractional_scale_manager {
        state.objects.fractional_scale = Some(fsm.get_fractional_scale(surface, qh, ()));
    }
    if fractional_scale_manager.is_none() {
        tracing::info!("fractional-scale-v1 unavailable, falling back to wl_output integer scale");
    }

    let layer_shell = layer_shell.ok_or_else(|| anyhow!("zwlr_layer_shell_v1 unavailable"))?;
    let layer_surface = layer_shell.get_layer_surface(
        surface,
        state.objects.output.as_ref(),
        wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_shell_v1::Layer::Background,
        "wallpaper-engine-renderer".to_string(),
        qh,
        (),
    );
    layer_surface.set_anchor(
        wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_surface_v1::Anchor::Top
            | wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_surface_v1::Anchor::Bottom
            | wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_surface_v1::Anchor::Left
            | wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_surface_v1::Anchor::Right,
    );
    layer_surface.set_size(0, 0);
    layer_surface.set_exclusive_zone(-1);
    layer_surface.set_margin(0, 0, 0, 0);
    layer_surface.set_keyboard_interactivity(
        wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_surface_v1::KeyboardInteractivity::None,
    );
    state.objects.layer_surface = Some(layer_surface);

    if seat.is_some() {
        state.objects.pointer = None;
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Input region helper
// ---------------------------------------------------------------------------

pub(super) fn update_input_region(state: &LayerShellState, qh: &QueueHandle<LayerShellState>) {
    if let (Some(ref compositor), Some(ref surface)) =
        (&state.objects.compositor, &state.objects.surface)
    {
        let region = compositor.create_region(qh, ());
        if state.interactive && state.output.logical_width > 0 && state.output.logical_height > 0 {
            region.add(0, 0, state.output.logical_width as i32, state.output.logical_height as i32);
        }
        surface.set_input_region(Some(&region));
        region.destroy();
    }
}
