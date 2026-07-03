use std::{
    os::fd::OwnedFd,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use wayland_client::protocol::{
    wl_buffer::WlBuffer, wl_compositor::WlCompositor, wl_output::WlOutput, wl_pointer::WlPointer,
    wl_shm::WlShm, wl_surface::WlSurface,
};
use wayland_protocols::wp::{
    fractional_scale::v1::client::wp_fractional_scale_v1::WpFractionalScaleV1,
    linux_dmabuf::zv1::client::zwp_linux_dmabuf_v1::ZwpLinuxDmabufV1,
    viewporter::client::wp_viewport::WpViewport,
};
use wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_surface_v1::ZwlrLayerSurfaceV1;
use we_renderer::{InputEvent, RendererLibrary, Session};

pub(super) const FRACTIONAL_SCALE_DENOMINATOR: u32 = 120;

// ---------------------------------------------------------------------------
// Buffer bookkeeping
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub(super) struct WaylandBuffer {
    pub(super) buffer: WlBuffer,
    pub(super) released: Arc<AtomicBool>,
    pub(super) pending_fds: Vec<OwnedFd>,
}

// ---------------------------------------------------------------------------
// Single-output state
// ---------------------------------------------------------------------------

pub(super) struct LayerState {
    pub(super) compositor: Option<WlCompositor>,
    pub(super) surface: Option<WlSurface>,
    pub(super) pointer: Option<WlPointer>,
    pub(super) output: Option<WlOutput>,
    pub(super) viewport: Option<WpViewport>,
    pub(super) layer_surface: Option<ZwlrLayerSurfaceV1>,
    pub(super) dmabuf: Option<ZwpLinuxDmabufV1>,
    pub(super) shm: Option<WlShm>,
    pub(super) fractional_scale: Option<WpFractionalScaleV1>,

    pub(super) dmabuf_version: u32,
    pub(super) compositor_version: u32,
    pub(super) output_count: u32,

    pub(super) output_scale: u32,
    pub(super) preferred_fractional_scale: u32,
    pub(super) output_mode_width: u32,
    pub(super) output_mode_height: u32,
    pub(super) logical_width: u32,
    pub(super) logical_height: u32,
    pub(super) render_width: u32,
    pub(super) render_height: u32,
    pub(super) fallback_width: u32,
    pub(super) fallback_height: u32,

    pub(super) pointer_x: f64,
    pub(super) pointer_y: f64,

    pub(super) running: bool,
    pub(super) configured: bool,
    pub(super) extent_mismatch_reported: bool,

    pub(super) session: Option<Session>,
    pub(super) _library: Option<RendererLibrary>,

    pub(super) in_flight: Vec<WaylandBuffer>,

    pub(super) interactive: bool,
    pub(super) paused: bool,
    pub(super) pending_input_events: Vec<InputEvent>,
}

impl LayerState {
    pub(super) fn render_scale_factor(&self) -> f64 {
        if self.preferred_fractional_scale >= FRACTIONAL_SCALE_DENOMINATOR {
            self.preferred_fractional_scale as f64 / FRACTIONAL_SCALE_DENOMINATOR as f64
        } else {
            self.output_scale.max(1) as f64
        }
    }

    pub(super) fn update_render_extent(&mut self) {
        if self.output_mode_width > 0 && self.output_mode_height > 0 {
            self.render_width = self.output_mode_width;
            self.render_height = self.output_mode_height;
            return;
        }

        let logical_w =
            if self.logical_width > 0 { self.logical_width } else { self.fallback_width };
        let logical_h =
            if self.logical_height > 0 { self.logical_height } else { self.fallback_height };
        let scale = self.render_scale_factor();

        self.render_width = (logical_w as f64 * scale).round().max(1.0) as u32;
        self.render_height = (logical_h as f64 * scale).round().max(1.0) as u32;
    }

    pub(super) fn update_viewport_destination(&self) {
        if let Some(viewport) = &self.viewport {
            if self.logical_width > 0 && self.logical_height > 0 {
                viewport.set_destination(self.logical_width as i32, self.logical_height as i32);
            }
        }
    }

    pub(super) fn normalized_pointer(&self) -> Option<(f32, f32)> {
        if self.logical_width == 0 || self.logical_height == 0 {
            return None;
        }
        Some((
            (self.pointer_x / self.logical_width as f64) as f32,
            (self.pointer_y / self.logical_height as f64) as f32,
        ))
    }

    pub(super) fn release_pending_send_fds(&mut self) {
        for entry in &mut self.in_flight {
            entry.pending_fds.clear();
        }
    }

    pub(super) fn collect_released_buffers(&mut self) {
        self.in_flight.retain(|entry| {
            !(entry.pending_fds.is_empty() && entry.released.load(Ordering::SeqCst))
        });
    }

    #[cfg(test)]
    pub(crate) fn test_default() -> Self {
        Self {
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
        }
    }
}
