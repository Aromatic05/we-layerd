use crate::config::ScaleMode;

use super::state::FRACTIONAL_SCALE_DENOMINATOR;

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
        ScaleMode::Fit => PresentationGeometry {
            render_width,
            render_height,
            viewport_width,
            viewport_height,
            viewport_source: None,
        },
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
    Some(ViewportSource {
        x: 0.0,
        y,
        width: render_width as f64,
        height: cropped_height,
    })
}
