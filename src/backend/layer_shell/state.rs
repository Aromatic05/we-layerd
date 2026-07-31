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
    linux_dmabuf::zv1::client::{
        zwp_linux_dmabuf_feedback_v1::ZwpLinuxDmabufFeedbackV1,
        zwp_linux_dmabuf_v1::ZwpLinuxDmabufV1,
    },
    viewporter::client::wp_viewport::WpViewport,
};
use wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_surface_v1::ZwlrLayerSurfaceV1;

use crate::{
    backend::wayland_common::{
        dmabuf::DmabufFeedbackState,
        input::{PointerAxis, PointerInputState},
        output::{OutputState, PresentationGeometry},
    },
    runtime::status::{FrameStats, RuntimeDiagnostics, RuntimeStatusSnapshot},
    runtime::{input::PendingInput, renderer_session::RendererSession},
};

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
    pub(super) dmabuf_feedback: Option<ZwpLinuxDmabufFeedbackV1>,
    pub(super) shm: Option<WlShm>,
    pub(super) fractional_scale: Option<WpFractionalScaleV1>,
    pub(super) frame_callback: Option<WlCallback>,
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

pub(crate) struct LayerShellState {
    pub(super) objects: WaylandObjects,
    pub(super) output: OutputState,
    pub(super) presentation_geometry: PresentationGeometry,
    pub(super) pointer_input: PointerInputState,
    pub(super) last_input_region: Option<(u32, u32)>,
    pub(super) buffers: BufferBookkeeping,
    pub(super) frame_callback: FrameCallbackState,
    pub(super) frame_stats: FrameStats,
    pub(super) diagnostics: RuntimeDiagnostics,
    pub(super) dmabuf_feedback: DmabufFeedbackState,
    pub(super) dmabuf_version: u32,
    pub(super) compositor_version: u32,
    pub(super) output_count: u32,
    pub(super) running: bool,
    pub(super) configured: bool,
    pub(super) session: Option<RendererSession>,
    pub(super) interactive: bool,
    pub(super) render_resolution_follows_output: bool,
    pub(super) paused: bool,
    pub(super) stopping: bool,
    pub(super) pending_input_events: PendingInput,
}

impl LayerShellState {
    pub(super) fn update_render_extent(&mut self) {
        let old_size = (self.output.geometry.render_width, self.output.geometry.render_height);
        self.output.recompute_geometry();
        let new_size = (self.output.geometry.render_width, self.output.geometry.render_height);
        if self.render_resolution_follows_output && old_size != new_size {
            if let Some(session) = &mut self.session {
                if let Err(error) = session.resize_output(new_size.0, new_size.1) {
                    tracing::warn!(%error, width = new_size.0, height = new_size.1, "failed to resize renderer output");
                }
            }
        }
    }

    pub(super) fn update_viewport_destination(&mut self) {
        let geometry = self.output.geometry;
        self.apply_viewport_geometry(geometry);
        self.presentation_geometry = geometry;
    }

    pub(super) fn update_viewport_destination_for_frame(
        &mut self,
        frame_width: u32,
        frame_height: u32,
    ) {
        let geometry = self.output.geometry_for_frame(frame_width, frame_height);
        self.apply_viewport_geometry(geometry);
        self.presentation_geometry = geometry;
    }

    fn apply_viewport_geometry(
        &self,
        geometry: crate::backend::wayland_common::output::PresentationGeometry,
    ) {
        if let Some(viewport) = &self.objects.viewport {
            if geometry.viewport_width > 0 && geometry.viewport_height > 0 {
                viewport.set_destination(
                    geometry.viewport_width as i32,
                    geometry.viewport_height as i32,
                );
                if let Some(source) = geometry.viewport_source {
                    viewport.set_source(source.x, source.y, source.width, source.height);
                } else {
                    viewport.set_source(
                        0.0,
                        0.0,
                        geometry.render_width as f64,
                        geometry.render_height as f64,
                    );
                }
            }
        }
    }

    pub(super) fn pointer_entered(&mut self, surface_x: f64, surface_y: f64) {
        let events = self.pointer_input.enter(surface_x, surface_y, self.presentation_geometry);
        for event in events {
            self.pending_input_events.push(event);
        }
    }

    pub(super) fn pointer_moved(&mut self, surface_x: f64, surface_y: f64) {
        if let Some(event) =
            self.pointer_input.move_to(surface_x, surface_y, self.presentation_geometry)
        {
            self.pending_input_events.push(event);
        }
    }

    pub(super) fn pointer_button(&mut self, linux_button: u32, pressed: bool) {
        if let Some(event) =
            self.pointer_input.button(linux_button, pressed, self.presentation_geometry)
        {
            self.pending_input_events.push(event);
        }
    }

    pub(super) fn pointer_axis(&mut self, axis: PointerAxis, value: f64) {
        self.pointer_input.axis(axis, value);
    }

    pub(super) fn pointer_axis_discrete(&mut self, axis: PointerAxis, steps: i32) {
        self.pointer_input.axis_discrete(axis, steps);
    }

    pub(super) fn pointer_axis_value120(&mut self, axis: PointerAxis, value: i32) {
        self.pointer_input.axis_value120(axis, value);
    }

    pub(super) fn pointer_axis_stopped(&mut self, axis: PointerAxis) {
        self.pointer_input.axis_stop(axis);
    }

    pub(super) fn pointer_axis_frame(&mut self) {
        if let Some(event) = self.pointer_input.finish_axis_frame(self.presentation_geometry) {
            self.pending_input_events.push(event);
        }
    }

    pub(super) fn pointer_left(&mut self) {
        for event in self.pointer_input.leave(self.presentation_geometry) {
            self.pending_input_events.push(event);
        }
    }

    pub(super) fn clear_pointer_input(&mut self) {
        self.pointer_input.clear();
    }

    pub(super) fn snapshot(&self) -> RuntimeStatusSnapshot {
        RuntimeStatusSnapshot {
            runtime: self.diagnostics.clone(),
            presentation: crate::runtime::status::PresentationStatus {
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
    pub(crate) fn test_default(scale_mode: crate::config::ScaleMode) -> Self {
        let output = OutputState::new(scale_mode);
        let presentation_geometry = output.geometry;
        Self {
            objects: WaylandObjects::default(),
            output,
            presentation_geometry,
            pointer_input: PointerInputState::default(),
            last_input_region: None,
            buffers: BufferBookkeeping::default(),
            frame_callback: FrameCallbackState::default(),
            frame_stats: FrameStats::default(),
            diagnostics: RuntimeDiagnostics::default(),
            dmabuf_feedback: DmabufFeedbackState::default(),
            dmabuf_version: 0,
            compositor_version: 0,
            output_count: 0,
            running: true,
            configured: false,
            session: None,
            interactive: false,
            render_resolution_follows_output: true,
            paused: false,
            stopping: false,
            pending_input_events: PendingInput::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::LayerShellState;
    use crate::config::ScaleMode;
    use we_renderer::InputEvent;

    #[test]
    fn snapshot_reflects_runtime_geometry_without_layer_shell_protocol_objects() {
        let mut state = LayerShellState::test_default(ScaleMode::Stretch);
        state.output.logical_width = 1280;
        state.output.logical_height = 720;
        state.update_render_extent();

        let snapshot = state.snapshot();
        assert_eq!(snapshot.presentation.render_width, 1280);
        assert_eq!(snapshot.presentation.render_height, 720);
    }

    #[test]
    fn pointer_mapping_uses_the_geometry_of_the_last_presented_frame() {
        let mut state = LayerShellState::test_default(ScaleMode::Cover);
        state.output.logical_width = 100;
        state.output.logical_height = 100;
        state.update_render_extent();
        state.update_viewport_destination_for_frame(200, 100);

        state.pointer_entered(0.0, 50.0);

        assert_eq!(
            state.pending_input_events.drain(),
            vec![InputEvent::Focus { focused: true }, InputEvent::PointerMove { x: 0.25, y: 0.5 },]
        );
    }
}
