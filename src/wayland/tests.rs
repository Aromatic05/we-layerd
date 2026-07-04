use we_core::install_layout::expand_tilde;

use crate::config::ScaleMode;

use super::state::*;

#[test]
fn render_extent_prefers_output_mode() {
    let mut state = LayerState::test_default(ScaleMode::Stretch);
    state.output.output_mode_width = 2560;
    state.output.output_mode_height = 1440;
    state.output.logical_width = 1920;
    state.output.logical_height = 1080;
    state.update_render_extent();
    assert_eq!(state.output.geometry.render_width, 2560);
    assert_eq!(state.output.geometry.render_height, 1440);
}

#[test]
fn render_extent_uses_logical_when_no_output_mode() {
    let mut state = LayerState::test_default(ScaleMode::Stretch);
    state.output.logical_width = 100;
    state.output.logical_height = 50;
    state.update_render_extent();
    assert_eq!(state.output.geometry.render_width, 100);
    assert_eq!(state.output.geometry.render_height, 50);
}

#[test]
fn render_extent_falls_back_when_logical_is_zero() {
    let mut state = LayerState::test_default(ScaleMode::Stretch);
    state.update_render_extent();
    assert_eq!(state.output.geometry.render_width, 1920);
    assert_eq!(state.output.geometry.render_height, 1080);
}

#[test]
fn render_extent_uses_fractional_scale() {
    let mut state = LayerState::test_default(ScaleMode::Stretch);
    state.output.preferred_fractional_scale = FRACTIONAL_SCALE_DENOMINATOR + 60;
    state.output.logical_width = 100;
    state.output.logical_height = 50;
    state.update_render_extent();
    assert_eq!(state.output.geometry.render_width, 150);
    assert_eq!(state.output.geometry.render_height, 75);
}

#[test]
fn fit_geometry_uses_letterboxed_destination_for_16_by_9_to_16_by_10() {
    let mut state = LayerState::test_default(ScaleMode::Fit);
    state.output.output_mode_width = 2560;
    state.output.output_mode_height = 1440;
    state.output.logical_width = 1920;
    state.output.logical_height = 1200;
    state.update_render_extent();

    assert_eq!(state.output.geometry.viewport_width, 1920);
    assert_eq!(state.output.geometry.viewport_height, 1080);
    assert!(state.output.geometry.viewport_source.is_none());
}

#[test]
fn cover_geometry_crops_width_for_16_by_9_to_16_by_10() {
    let mut state = LayerState::test_default(ScaleMode::Cover);
    state.output.output_mode_width = 2560;
    state.output.output_mode_height = 1440;
    state.output.logical_width = 1920;
    state.output.logical_height = 1200;
    state.update_render_extent();

    let source = state.output.geometry.viewport_source.expect("cover should crop");
    assert_eq!(state.output.geometry.viewport_width, 1920);
    assert_eq!(state.output.geometry.viewport_height, 1200);
    assert!(source.x > 0.0);
    assert_eq!(source.y, 0.0);
}

#[test]
fn stretch_geometry_keeps_full_destination_for_16_by_9_to_16_by_10() {
    let mut state = LayerState::test_default(ScaleMode::Stretch);
    state.output.output_mode_width = 2560;
    state.output.output_mode_height = 1440;
    state.output.logical_width = 1920;
    state.output.logical_height = 1200;
    state.update_render_extent();

    assert_eq!(state.output.geometry.viewport_width, 1920);
    assert_eq!(state.output.geometry.viewport_height, 1200);
    assert!(state.output.geometry.viewport_source.is_none());
}

#[test]
fn expand_tilde_expands_home_prefix() {
    let home = std::env::var_os("HOME").expect("HOME must be set in test env");
    let expanded = expand_tilde("~/renderer-cache");
    assert_eq!(expanded, std::path::PathBuf::from(home).join("renderer-cache"));
}
