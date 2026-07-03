use super::renderer::expand_tilde;
use super::state::*;

#[test]
fn render_extent_prefers_output_mode() {
    let mut state = LayerState::test_default();
    state.output_mode_width = 2560;
    state.output_mode_height = 1440;
    state.logical_width = 1920;
    state.logical_height = 1080;
    state.update_render_extent();
    assert_eq!(state.render_width, 2560);
    assert_eq!(state.render_height, 1440);
}

#[test]
fn render_extent_uses_logical_when_no_output_mode() {
    let mut state = LayerState::test_default();
    state.logical_width = 100;
    state.logical_height = 50;
    state.update_render_extent();
    assert_eq!(state.render_width, 100);
    assert_eq!(state.render_height, 50);
}

#[test]
fn render_extent_falls_back_when_logical_is_zero() {
    let mut state = LayerState::test_default();
    state.update_render_extent();
    assert_eq!(state.render_width, 1920);
    assert_eq!(state.render_height, 1080);
}

#[test]
fn render_extent_uses_fractional_scale() {
    let mut state = LayerState::test_default();
    state.preferred_fractional_scale = FRACTIONAL_SCALE_DENOMINATOR + 60; // 180/120 = 1.5
    state.logical_width = 100;
    state.logical_height = 50;
    state.update_render_extent();
    assert_eq!(state.render_width, 150);
    assert_eq!(state.render_height, 75);
}

#[test]
fn expand_tilde_expands_home_prefix() {
    let home = std::env::var_os("HOME").expect("HOME must be set in test env");
    let expanded = expand_tilde("~/renderer-cache");
    assert_eq!(expanded, std::path::PathBuf::from(home).join("renderer-cache"));
}
