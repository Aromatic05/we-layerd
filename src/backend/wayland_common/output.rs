use crate::config::ScaleMode;

pub(crate) const FRACTIONAL_SCALE_DENOMINATOR: u32 = 120;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ViewportSource {
    pub(crate) x: f64,
    pub(crate) y: f64,
    pub(crate) width: f64,
    pub(crate) height: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PresentationGeometry {
    pub(crate) render_width: u32,
    pub(crate) render_height: u32,
    pub(crate) viewport_width: u32,
    pub(crate) viewport_height: u32,
    pub(crate) viewport_source: Option<ViewportSource>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct GeometryInput {
    pub(crate) logical_width: u32,
    pub(crate) logical_height: u32,
    pub(crate) output_mode_width: u32,
    pub(crate) output_mode_height: u32,
    pub(crate) fallback_width: u32,
    pub(crate) fallback_height: u32,
    pub(crate) output_scale: u32,
    pub(crate) preferred_fractional_scale: u32,
    pub(crate) scale_mode: ScaleMode,
    pub(crate) render_size_override: Option<(u32, u32)>,
}

fn render_scale_factor(input: GeometryInput) -> f64 {
    if input.preferred_fractional_scale >= FRACTIONAL_SCALE_DENOMINATOR {
        input.preferred_fractional_scale as f64 / FRACTIONAL_SCALE_DENOMINATOR as f64
    } else {
        input.output_scale.max(1) as f64
    }
}

pub(crate) fn compute_geometry(input: GeometryInput) -> PresentationGeometry {
    let viewport_width =
        if input.logical_width > 0 { input.logical_width } else { input.fallback_width }.max(1);
    let viewport_height =
        if input.logical_height > 0 { input.logical_height } else { input.fallback_height }.max(1);

    if let Some((render_width, render_height)) = input.render_size_override {
        return geometry_with_render_extent(
            input.scale_mode,
            render_width.max(1),
            render_height.max(1),
            viewport_width,
            viewport_height,
        );
    }

    if input.output_mode_width > 0 && input.output_mode_height > 0 {
        return geometry_with_render_extent(
            input.scale_mode,
            input.output_mode_width,
            input.output_mode_height,
            viewport_width,
            viewport_height,
        );
    }

    let scale = render_scale_factor(input);
    let render_width = (viewport_width as f64 * scale).round().max(1.0) as u32;
    let render_height = (viewport_height as f64 * scale).round().max(1.0) as u32;
    geometry_with_render_extent(
        input.scale_mode,
        render_width,
        render_height,
        viewport_width,
        viewport_height,
    )
}

fn geometry_with_render_extent(
    scale_mode: ScaleMode,
    render_width: u32,
    render_height: u32,
    viewport_width: u32,
    viewport_height: u32,
) -> PresentationGeometry {
    match scale_mode {
        ScaleMode::Stretch => PresentationGeometry {
            render_width,
            render_height,
            viewport_width,
            viewport_height,
            viewport_source: None,
        },
        ScaleMode::Cover => {
            let source = cover_source(render_width, render_height, viewport_width, viewport_height);
            PresentationGeometry {
                render_width,
                render_height,
                viewport_width,
                viewport_height,
                viewport_source: source,
            }
        }
        ScaleMode::Fit => {
            // A single layer-surface + wp_viewporter destination cannot center letterboxing.
            // We still preserve aspect ratio here so fit is observable in status and rendering,
            // with the current limitation that any empty area stays on the bottom/right edges.
            let (fit_width, fit_height) =
                fit_destination(render_width, render_height, viewport_width, viewport_height);
            PresentationGeometry {
                render_width,
                render_height,
                viewport_width: fit_width,
                viewport_height: fit_height,
                viewport_source: None,
            }
        }
    }
}

fn cover_source(
    render_width: u32,
    render_height: u32,
    viewport_width: u32,
    viewport_height: u32,
) -> Option<ViewportSource> {
    if render_width == 0 || render_height == 0 || viewport_width == 0 || viewport_height == 0 {
        return None;
    }

    let render_aspect = render_width as f64 / render_height as f64;
    let viewport_aspect = viewport_width as f64 / viewport_height as f64;
    if (render_aspect - viewport_aspect).abs() < f64::EPSILON {
        return None;
    }

    if render_aspect > viewport_aspect {
        let cropped_width = render_height as f64 * viewport_aspect;
        let x = ((render_width as f64 - cropped_width) / 2.0).max(0.0);
        return Some(ViewportSource {
            x,
            y: 0.0,
            width: cropped_width,
            height: render_height as f64,
        });
    }

    let cropped_height = render_width as f64 / viewport_aspect;
    let y = ((render_height as f64 - cropped_height) / 2.0).max(0.0);
    Some(ViewportSource { x: 0.0, y, width: render_width as f64, height: cropped_height })
}

fn fit_destination(
    render_width: u32,
    render_height: u32,
    viewport_width: u32,
    viewport_height: u32,
) -> (u32, u32) {
    if render_width == 0 || render_height == 0 || viewport_width == 0 || viewport_height == 0 {
        return (viewport_width, viewport_height);
    }

    let width_scale = viewport_width as f64 / render_width as f64;
    let height_scale = viewport_height as f64 / render_height as f64;
    let scale = width_scale.min(height_scale);
    let width = (render_width as f64 * scale).round().max(1.0) as u32;
    let height = (render_height as f64 * scale).round().max(1.0) as u32;
    (width, height)
}

pub(crate) struct OutputState {
    pub(crate) output_scale: u32,
    pub(crate) preferred_fractional_scale: u32,
    pub(crate) output_mode_width: u32,
    pub(crate) output_mode_height: u32,
    pub(crate) logical_width: u32,
    pub(crate) logical_height: u32,
    pub(crate) fallback_width: u32,
    pub(crate) fallback_height: u32,
    pub(crate) scale_mode: ScaleMode,
    pub(crate) render_size_override: Option<(u32, u32)>,
    pub(crate) geometry: PresentationGeometry,
}

impl OutputState {
    pub(crate) fn new(scale_mode: ScaleMode) -> Self {
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
            render_size_override: None,
            geometry: PresentationGeometry {
                render_width: 1920,
                render_height: 1080,
                viewport_width: 1920,
                viewport_height: 1080,
                viewport_source: None,
            },
        };
        output.recompute_geometry();
        output
    }

    pub(crate) fn render_scale_factor(&self) -> f64 {
        if self.preferred_fractional_scale >= FRACTIONAL_SCALE_DENOMINATOR {
            self.preferred_fractional_scale as f64 / FRACTIONAL_SCALE_DENOMINATOR as f64
        } else {
            self.output_scale.max(1) as f64
        }
    }

    pub(crate) fn recompute_geometry(&mut self) {
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
            render_size_override: self.render_size_override,
        });
    }

    pub(crate) fn geometry_for_frame(
        &self,
        frame_width: u32,
        frame_height: u32,
    ) -> PresentationGeometry {
        let viewport_width =
            if self.logical_width > 0 { self.logical_width } else { self.fallback_width }.max(1);
        let viewport_height =
            if self.logical_height > 0 { self.logical_height } else { self.fallback_height }.max(1);
        geometry_with_render_extent(
            self.scale_mode,
            frame_width.max(1),
            frame_height.max(1),
            viewport_width,
            viewport_height,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{OutputState, FRACTIONAL_SCALE_DENOMINATOR};
    use crate::config::ScaleMode;

    #[test]
    fn render_extent_prefers_output_mode() {
        let mut output = OutputState::new(ScaleMode::Stretch);
        output.output_mode_width = 2560;
        output.output_mode_height = 1440;
        output.logical_width = 1920;
        output.logical_height = 1080;
        output.recompute_geometry();
        assert_eq!(output.geometry.render_width, 2560);
        assert_eq!(output.geometry.render_height, 1440);
    }

    #[test]
    fn render_extent_uses_logical_when_no_output_mode() {
        let mut output = OutputState::new(ScaleMode::Stretch);
        output.logical_width = 100;
        output.logical_height = 50;
        output.recompute_geometry();
        assert_eq!(output.geometry.render_width, 100);
        assert_eq!(output.geometry.render_height, 50);
    }

    #[test]
    fn render_extent_falls_back_when_logical_is_zero() {
        let output = OutputState::new(ScaleMode::Stretch);
        assert_eq!(output.geometry.render_width, 1920);
        assert_eq!(output.geometry.render_height, 1080);
    }

    #[test]
    fn render_extent_uses_fractional_scale() {
        let mut output = OutputState::new(ScaleMode::Stretch);
        output.preferred_fractional_scale = FRACTIONAL_SCALE_DENOMINATOR + 60;
        output.logical_width = 100;
        output.logical_height = 50;
        output.recompute_geometry();
        assert_eq!(output.geometry.render_width, 150);
        assert_eq!(output.geometry.render_height, 75);
    }

    #[test]
    fn fixed_render_resolution_is_presented_at_its_configured_extent() {
        let mut output = OutputState::new(ScaleMode::Cover);
        output.logical_width = 1920;
        output.logical_height = 1080;
        output.output_mode_width = 3840;
        output.output_mode_height = 2160;
        output.render_size_override = Some((1280, 720));
        output.recompute_geometry();

        assert_eq!(output.geometry.render_width, 1280);
        assert_eq!(output.geometry.render_height, 720);
        assert_eq!(output.geometry.viewport_width, 1920);
        assert_eq!(output.geometry.viewport_height, 1080);
    }

    #[test]
    fn fit_geometry_uses_letterboxed_destination_for_16_by_9_to_16_by_10() {
        let mut output = OutputState::new(ScaleMode::Fit);
        output.output_mode_width = 2560;
        output.output_mode_height = 1440;
        output.logical_width = 1920;
        output.logical_height = 1200;
        output.recompute_geometry();

        assert_eq!(output.geometry.viewport_width, 1920);
        assert_eq!(output.geometry.viewport_height, 1080);
        assert!(output.geometry.viewport_source.is_none());
    }

    #[test]
    fn cover_geometry_crops_width_for_16_by_9_to_16_by_10() {
        let mut output = OutputState::new(ScaleMode::Cover);
        output.output_mode_width = 2560;
        output.output_mode_height = 1440;
        output.logical_width = 1920;
        output.logical_height = 1200;
        output.recompute_geometry();

        let source = output.geometry.viewport_source.expect("cover should crop");
        assert_eq!(output.geometry.viewport_width, 1920);
        assert_eq!(output.geometry.viewport_height, 1200);
        assert!(source.x > 0.0);
        assert_eq!(source.y, 0.0);
    }

    #[test]
    fn frame_geometry_uses_actual_buffer_extent_for_cover() {
        let mut output = OutputState::new(ScaleMode::Cover);
        output.logical_width = 1707;
        output.logical_height = 1067;
        output.output_mode_width = 2560;
        output.output_mode_height = 1600;
        output.recompute_geometry();

        let geometry = output.geometry_for_frame(1920, 1080);
        let source = geometry.viewport_source.expect("cover should crop the actual frame");
        assert_eq!(geometry.render_width, 1920);
        assert_eq!(geometry.render_height, 1080);
        assert!(source.x >= 0.0);
        assert!(source.y >= 0.0);
        assert!(source.x + source.width <= 1920.0);
        assert!(source.y + source.height <= 1080.0);
    }

    #[test]
    fn stretch_geometry_keeps_full_destination_for_16_by_9_to_16_by_10() {
        let mut output = OutputState::new(ScaleMode::Stretch);
        output.output_mode_width = 2560;
        output.output_mode_height = 1440;
        output.logical_width = 1920;
        output.logical_height = 1200;
        output.recompute_geometry();

        assert_eq!(output.geometry.viewport_width, 1920);
        assert_eq!(output.geometry.viewport_height, 1200);
        assert!(output.geometry.viewport_source.is_none());
    }
}
