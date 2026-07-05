use std::{
    os::fd::OwnedFd,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use wayland_client::protocol::{
    wl_buffer::WlBuffer, wl_callback::WlCallback, wl_compositor::WlCompositor, wl_output::WlOutput,
    wl_pointer::WlPointer, wl_shm::WlShm, wl_surface::WlSurface,
};
use wayland_protocols::wp::{
    fractional_scale::v1::client::wp_fractional_scale_v1::WpFractionalScaleV1,
    linux_dmabuf::zv1::client::zwp_linux_dmabuf_v1::ZwpLinuxDmabufV1,
    viewporter::client::wp_viewport::WpViewport,
};
use wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_surface_v1::ZwlrLayerSurfaceV1;
use we_renderer::{InputEvent, RendererLibrary, Session};

use crate::config::ScaleMode;

use super::{
    diagnostics::{FrameStats, RuntimeDiagnostics, RuntimeStatusSnapshot},
    geometry::{compute_geometry, GeometryInput, PresentationGeometry},
};

pub(super) const FRACTIONAL_SCALE_DENOMINATOR: u32 = 120;
pub(super) const MAX_IN_FLIGHT_BUFFERS: usize = 3;

// ---------------------------------------------------------------------------
// Buffer bookkeeping
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub(super) struct WaylandBuffer {
    pub(super) buffer: WlBuffer,
    pub(super) released: Arc<AtomicBool>,
    pub(super) pending_fds: Vec<OwnedFd>,
}

impl Drop for WaylandBuffer {
    fn drop(&mut self) {
        self.pending_fds.clear();
        self.buffer.destroy();
    }
}

#[derive(Default)]
pub(super) struct WaylandObjects {
    pub(super) compositor: Option<WlCompositor>,
    pub(super) surface: Option<WlSurface>,
    pub(super) pointer: Option<WlPointer>,
    pub(super) output: Option<WlOutput>,
    pub(super) viewport: Option<WpViewport>,
    pub(super) layer_surface: Option<ZwlrLayerSurfaceV1>,
    pub(super) dmabuf: Option<ZwpLinuxDmabufV1>,
    pub(super) shm: Option<WlShm>,
    pub(super) fractional_scale: Option<WpFractionalScaleV1>,
    pub(super) frame_callback: Option<WlCallback>,
}

pub(super) struct OutputState {
    pub(super) output_scale: u32,
    pub(super) preferred_fractional_scale: u32,
    pub(super) output_mode_width: u32,
    pub(super) output_mode_height: u32,
    pub(super) logical_width: u32,
    pub(super) logical_height: u32,
    pub(super) fallback_width: u32,
    pub(super) fallback_height: u32,
    pub(super) scale_mode: ScaleMode,
    pub(super) geometry: PresentationGeometry,
    pub(super) pointer_x: f64,
    pub(super) pointer_y: f64,
}

impl OutputState {
    pub(super) fn new(scale_mode: ScaleMode) -> Self {
        let mut output = Self {
            output_scale: 1,
            preferred_fractional_scale: 0,
            output_mode_width: 0,
            output_mode_height: 0,
            logical_width: 0,
            logical_height: 0,
            fallback_width: 1920,
            fallback_height: 1080,
            scale_mode,
            geometry: PresentationGeometry {
                render_width: 1920,
                render_height: 1080,
                viewport_width: 1920,
                viewport_height: 1080,
                viewport_source: None,
            },
            pointer_x: 0.0,
            pointer_y: 0.0,
        };
        output.recompute_geometry();
        output
    }

    pub(super) fn render_scale_factor(&self) -> f64 {
        if self.preferred_fractional_scale >= FRACTIONAL_SCALE_DENOMINATOR {
            self.preferred_fractional_scale as f64 / FRACTIONAL_SCALE_DENOMINATOR as f64
        } else {
            self.output_scale.max(1) as f64
        }
    }

    pub(super) fn recompute_geometry(&mut self) {
        self.geometry = compute_geometry(GeometryInput {
            logical_width: self.logical_width,
            logical_height: self.logical_height,
            output_mode_width: self.output_mode_width,
            output_mode_height: self.output_mode_height,
            fallback_width: self.fallback_width,
            fallback_height: self.fallback_height,
            output_scale: self.output_scale,
            preferred_fractional_scale: self.preferred_fractional_scale,
            scale_mode: self.scale_mode,
        });
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
}

#[derive(Default)]
pub(super) struct FrameCallbackState {
    pub(super) pending: bool,
    pub(super) ready_for_next_frame: bool,
    pub(super) last_done_msec: Option<u32>,
}

pub(super) struct BufferBookkeeping {
    pub(super) in_flight: Vec<WaylandBuffer>,
    pub(super) max_in_flight: usize,
}

impl Default for BufferBookkeeping {
    fn default() -> Self {
        Self { in_flight: Vec::new(), max_in_flight: MAX_IN_FLIGHT_BUFFERS }
    }
}

pub(crate) struct LayerState {
    pub(super) objects: WaylandObjects,
    pub(super) output: OutputState,
    pub(super) buffers: BufferBookkeeping,
    pub(super) frame_callback: FrameCallbackState,
    pub(super) frame_stats: FrameStats,
    pub(super) diagnostics: RuntimeDiagnostics,
    pub(super) dmabuf_version: u32,
    pub(super) compositor_version: u32,
    pub(super) output_count: u32,
    pub(super) running: bool,
    pub(super) configured: bool,
    pub(super) session: Option<Session>,
    pub(super) _library: Option<RendererLibrary>,
    pub(super) interactive: bool,
    pub(super) paused: bool,
    pub(super) stopping: bool,
    pub(super) pending_input_events: Vec<InputEvent>,
}

impl LayerState {
    pub(super) fn update_render_extent(&mut self) {
        self.output.recompute_geometry();
    }

    pub(super) fn update_viewport_destination(&self) {
        if let Some(viewport) = &self.objects.viewport {
            if self.output.geometry.viewport_width > 0 && self.output.geometry.viewport_height > 0 {
                viewport.set_destination(
                    self.output.geometry.viewport_width as i32,
                    self.output.geometry.viewport_height as i32,
                );
                if let Some(source) = self.output.geometry.viewport_source {
                    viewport.set_source(source.x, source.y, source.width, source.height);
                } else {
                    viewport.set_source(
                        0.0,
                        0.0,
                        self.output.geometry.render_width as f64,
                        self.output.geometry.render_height as f64,
                    );
                }
            }
        }
    }

    pub(super) fn normalized_pointer(&self) -> Option<(f32, f32)> {
        self.output.normalized_pointer()
    }

    pub(super) fn snapshot(&self) -> RuntimeStatusSnapshot {
        RuntimeStatusSnapshot {
            runtime: self.diagnostics.clone(),
            presentation: super::diagnostics::PresentationStatus {
                configured: self.configured,
                logical_width: self.output.logical_width,
                logical_height: self.output.logical_height,
                render_width: self.output.geometry.render_width,
                render_height: self.output.geometry.render_height,
                output_mode_width: self.output.output_mode_width,
                output_mode_height: self.output.output_mode_height,
                output_scale: self.output.output_scale,
                fractional_scale: self.output.render_scale_factor(),
                scale_mode: self.output.scale_mode,
                paused: self.paused,
                viewport_width: self.output.geometry.viewport_width,
                viewport_height: self.output.geometry.viewport_height,
                viewport_source: self
                    .output
                    .geometry
                    .viewport_source
                    .map(|source| (source.x, source.y, source.width, source.height)),
            },
            frame_stats: self.frame_stats.clone(),
        }
    }

    pub(super) fn release_pending_send_fds(&mut self) {
        for entry in &mut self.buffers.in_flight {
            entry.pending_fds.clear();
        }
    }

    pub(super) fn collect_released_buffers(&mut self) {
        let before = self.buffers.in_flight.len();
        self.buffers.in_flight.retain(|entry| {
            !(entry.pending_fds.is_empty() && entry.released.load(Ordering::SeqCst))
        });
        let released = before.saturating_sub(self.buffers.in_flight.len());
        self.frame_stats.released_buffers =
            self.frame_stats.released_buffers.saturating_add(released as u64);
        self.frame_stats.in_flight_count = self.buffers.in_flight.len();
    }

    pub(super) fn clear_in_flight_buffers(&mut self) {
        self.buffers.in_flight.clear();
        self.frame_stats.in_flight_count = 0;
    }

    #[cfg(test)]
    pub(crate) fn test_default(scale_mode: ScaleMode) -> Self {
        Self {
            objects: WaylandObjects::default(),
            output: OutputState::new(scale_mode),
            buffers: BufferBookkeeping::default(),
            frame_callback: FrameCallbackState::default(),
            frame_stats: FrameStats::default(),
            diagnostics: RuntimeDiagnostics::default(),
            dmabuf_version: 0,
            compositor_version: 0,
            output_count: 0,
            running: true,
            configured: false,
            session: None,
            _library: None,
            interactive: false,
            paused: false,
            stopping: false,
            pending_input_events: Vec::new(),
        }
    }
}
