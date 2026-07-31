use we_renderer::InputEvent;

use super::output::{PresentationGeometry, ViewportSource};

const BTN_LEFT: u32 = 0x110;
const BTN_RIGHT: u32 = 0x111;
const BTN_MIDDLE: u32 = 0x112;

#[derive(Debug, Clone, Copy)]
pub(crate) enum PointerAxis {
    Horizontal,
    Vertical,
}

pub(crate) fn map_surface_position(
    surface_x: f64,
    surface_y: f64,
    geometry: PresentationGeometry,
) -> Option<(f32, f32)> {
    if !surface_x.is_finite()
        || !surface_y.is_finite()
        || geometry.render_width == 0
        || geometry.render_height == 0
        || geometry.viewport_width == 0
        || geometry.viewport_height == 0
    {
        return None;
    }

    let source = geometry.viewport_source.unwrap_or(ViewportSource {
        x: 0.0,
        y: 0.0,
        width: geometry.render_width as f64,
        height: geometry.render_height as f64,
    });
    if !source.x.is_finite()
        || !source.y.is_finite()
        || !source.width.is_finite()
        || !source.height.is_finite()
        || source.width <= 0.0
        || source.height <= 0.0
    {
        return None;
    }

    let render_x = source.x + surface_x / geometry.viewport_width as f64 * source.width;
    let render_y = source.y + surface_y / geometry.viewport_height as f64 * source.height;
    let normalized_x = (render_x / geometry.render_width as f64).clamp(0.0, 1.0);
    let normalized_y = (render_y / geometry.render_height as f64).clamp(0.0, 1.0);
    if !normalized_x.is_finite() || !normalized_y.is_finite() {
        return None;
    }

    Some((normalized_x as f32, normalized_y as f32))
}

fn map_button(button: u32) -> Option<usize> {
    match button {
        BTN_LEFT => Some(0),
        BTN_MIDDLE => Some(1),
        BTN_RIGHT => Some(2),
        _ => None,
    }
}

#[derive(Debug, Default)]
struct AxisFrame {
    continuous_x: f64,
    continuous_y: f64,
    discrete_x: i32,
    discrete_y: i32,
    value120_x: i32,
    value120_y: i32,
    has_discrete_x: bool,
    has_discrete_y: bool,
    has_value120_x: bool,
    has_value120_y: bool,
    stopped_x: bool,
    stopped_y: bool,
}

impl AxisFrame {
    fn continuous(&mut self, axis: PointerAxis, value: f64) {
        if !value.is_finite() {
            return;
        }
        match axis {
            PointerAxis::Horizontal => self.continuous_x += value,
            PointerAxis::Vertical => self.continuous_y += value,
        }
    }

    fn discrete(&mut self, axis: PointerAxis, steps: i32) {
        match axis {
            PointerAxis::Horizontal => {
                self.discrete_x = self.discrete_x.saturating_add(steps);
                self.has_discrete_x = true;
            }
            PointerAxis::Vertical => {
                self.discrete_y = self.discrete_y.saturating_add(steps);
                self.has_discrete_y = true;
            }
        }
    }

    fn value120(&mut self, axis: PointerAxis, value: i32) {
        match axis {
            PointerAxis::Horizontal => {
                self.value120_x = self.value120_x.saturating_add(value);
                self.has_value120_x = true;
            }
            PointerAxis::Vertical => {
                self.value120_y = self.value120_y.saturating_add(value);
                self.has_value120_y = true;
            }
        }
    }

    fn stop(&mut self, axis: PointerAxis) {
        match axis {
            PointerAxis::Horizontal => self.stopped_x = true,
            PointerAxis::Vertical => self.stopped_y = true,
        }
    }
}

#[derive(Debug, Default)]
pub(crate) struct PointerInputState {
    focused: bool,
    surface_position: Option<(f64, f64)>,
    renderer_position: Option<(f32, f32)>,
    pressed_buttons: [bool; 3],
    axis_frame: AxisFrame,
    smooth_remainder_x: f64,
    smooth_remainder_y: f64,
}

impl PointerInputState {
    pub(crate) fn enter(
        &mut self,
        surface_x: f64,
        surface_y: f64,
        geometry: PresentationGeometry,
    ) -> Vec<InputEvent> {
        let mut events = Vec::with_capacity(2);
        if !self.focused {
            self.focused = true;
            events.push(InputEvent::Focus { focused: true });
        }
        if let Some(event) = self.move_to(surface_x, surface_y, geometry) {
            events.push(event);
        }
        events
    }

    pub(crate) fn move_to(
        &mut self,
        surface_x: f64,
        surface_y: f64,
        geometry: PresentationGeometry,
    ) -> Option<InputEvent> {
        if !self.focused || !surface_x.is_finite() || !surface_y.is_finite() {
            return None;
        }

        self.surface_position = Some((surface_x, surface_y));
        let (x, y) = map_surface_position(surface_x, surface_y, geometry)?;
        self.renderer_position = Some((x, y));
        Some(InputEvent::PointerMove { x, y })
    }

    pub(crate) fn button(
        &mut self,
        linux_button: u32,
        pressed: bool,
        geometry: PresentationGeometry,
    ) -> Option<InputEvent> {
        if !self.focused {
            return None;
        }
        let button = map_button(linux_button)?;
        if self.pressed_buttons[button] == pressed {
            return None;
        }

        if pressed {
            let (x, y) = self.current_position(geometry)?;
            self.pressed_buttons[button] = true;
            return Some(InputEvent::PointerDown { x, y, button: button as i32 });
        }

        let position = self.current_position(geometry).or(self.renderer_position);
        self.pressed_buttons[button] = false;
        position.map(|(x, y)| InputEvent::PointerUp { x, y, button: button as i32 })
    }

    pub(crate) fn axis(&mut self, axis: PointerAxis, value: f64) {
        self.axis_frame.continuous(axis, value);
    }

    pub(crate) fn axis_discrete(&mut self, axis: PointerAxis, steps: i32) {
        self.axis_frame.discrete(axis, steps);
    }

    pub(crate) fn axis_value120(&mut self, axis: PointerAxis, value: i32) {
        self.axis_frame.value120(axis, value);
    }

    pub(crate) fn axis_stop(&mut self, axis: PointerAxis) {
        self.axis_frame.stop(axis);
    }

    pub(crate) fn finish_axis_frame(
        &mut self,
        geometry: PresentationGeometry,
    ) -> Option<InputEvent> {
        let frame = std::mem::take(&mut self.axis_frame);
        let delta_x = axis_delta(
            frame.continuous_x,
            frame.discrete_x,
            frame.has_discrete_x,
            frame.value120_x,
            frame.has_value120_x,
            &mut self.smooth_remainder_x,
        );
        let delta_y = axis_delta(
            frame.continuous_y,
            frame.discrete_y,
            frame.has_discrete_y,
            frame.value120_y,
            frame.has_value120_y,
            &mut self.smooth_remainder_y,
        );
        if frame.stopped_x {
            self.smooth_remainder_x = 0.0;
        }
        if frame.stopped_y {
            self.smooth_remainder_y = 0.0;
        }
        if !self.focused || (delta_x == 0 && delta_y == 0) {
            return None;
        }
        let (x, y) = self.current_position(geometry)?;
        Some(InputEvent::PointerWheel { x, y, delta_x, delta_y })
    }

    pub(crate) fn leave(&mut self, geometry: PresentationGeometry) -> Vec<InputEvent> {
        let position = self.current_position(geometry).or(self.renderer_position);
        let pressed_buttons = std::mem::take(&mut self.pressed_buttons);
        let was_focused = self.focused;
        self.focused = false;
        self.surface_position = None;
        self.renderer_position = None;
        self.axis_frame = AxisFrame::default();
        self.smooth_remainder_x = 0.0;
        self.smooth_remainder_y = 0.0;

        let mut events = Vec::with_capacity(4);
        if let Some((x, y)) = position {
            events.extend(pressed_buttons.into_iter().enumerate().filter_map(
                |(button, pressed)| {
                    pressed.then_some(InputEvent::PointerUp { x, y, button: button as i32 })
                },
            ));
        }
        if was_focused {
            events.push(InputEvent::Focus { focused: false });
        }
        events
    }

    pub(crate) fn clear(&mut self) {
        *self = Self::default();
    }

    fn current_position(&mut self, geometry: PresentationGeometry) -> Option<(f32, f32)> {
        let (surface_x, surface_y) = self.surface_position?;
        let position = map_surface_position(surface_x, surface_y, geometry)?;
        self.renderer_position = Some(position);
        Some(position)
    }
}

fn axis_delta(
    continuous: f64,
    discrete: i32,
    has_discrete: bool,
    value120: i32,
    has_value120: bool,
    smooth_remainder: &mut f64,
) -> i32 {
    // Wayland's positive axes point down/right. The renderer ABI follows the
    // CEF convention where positive wheel deltas point up/left.
    if has_value120 {
        return value120.saturating_neg();
    }
    if has_discrete {
        return discrete.saturating_mul(120).saturating_neg();
    }

    *smooth_remainder -= continuous;
    let integral = smooth_remainder.trunc().clamp(i32::MIN as f64, i32::MAX as f64) as i32;
    *smooth_remainder -= integral as f64;
    integral
}

#[cfg(test)]
mod tests {
    use super::{
        map_button, map_surface_position, PointerAxis, PointerInputState, BTN_LEFT, BTN_MIDDLE,
        BTN_RIGHT,
    };
    use crate::backend::wayland_common::output::{PresentationGeometry, ViewportSource};
    use we_renderer::InputEvent;

    fn geometry(
        render_width: u32,
        render_height: u32,
        viewport_width: u32,
        viewport_height: u32,
        viewport_source: Option<ViewportSource>,
    ) -> PresentationGeometry {
        PresentationGeometry {
            render_width,
            render_height,
            viewport_width,
            viewport_height,
            viewport_source,
        }
    }

    #[test]
    fn stretch_coordinates_are_normalized_and_clamped() {
        let stretch = geometry(200, 100, 100, 50, None);
        assert_eq!(map_surface_position(25.0, 37.5, stretch), Some((0.25, 0.75)));
        assert_eq!(map_surface_position(-10.0, 80.0, stretch), Some((0.0, 1.0)));
    }

    #[test]
    fn cover_coordinates_include_the_cropped_source_offset() {
        let cover = geometry(
            200,
            100,
            100,
            100,
            Some(ViewportSource { x: 50.0, y: 0.0, width: 100.0, height: 100.0 }),
        );
        assert_eq!(map_surface_position(0.0, 50.0, cover), Some((0.25, 0.5)));
        assert_eq!(map_surface_position(100.0, 50.0, cover), Some((0.75, 0.5)));
    }

    #[test]
    fn fit_coordinates_use_the_smaller_destination() {
        let fit = geometry(200, 100, 100, 50, None);
        assert_eq!(map_surface_position(50.0, 25.0, fit), Some((0.5, 0.5)));
        assert_eq!(map_surface_position(75.0, 75.0, fit), Some((0.75, 1.0)));
    }

    #[test]
    fn linux_mouse_buttons_use_renderer_indices() {
        assert_eq!(map_button(BTN_LEFT), Some(0));
        assert_eq!(map_button(BTN_MIDDLE), Some(1));
        assert_eq!(map_button(BTN_RIGHT), Some(2));
        assert_eq!(map_button(0x113), None);
    }

    #[test]
    fn enter_and_leave_preserve_focus_and_release_every_pressed_button() {
        let geometry = geometry(100, 100, 100, 100, None);
        let mut pointer = PointerInputState::default();
        assert_eq!(
            pointer.enter(20.0, 30.0, geometry),
            vec![InputEvent::Focus { focused: true }, InputEvent::PointerMove { x: 0.2, y: 0.3 },]
        );
        assert_eq!(
            pointer.button(BTN_LEFT, true, geometry),
            Some(InputEvent::PointerDown { x: 0.2, y: 0.3, button: 0 })
        );
        assert_eq!(
            pointer.button(BTN_RIGHT, true, geometry),
            Some(InputEvent::PointerDown { x: 0.2, y: 0.3, button: 2 })
        );
        assert_eq!(
            pointer.leave(geometry),
            vec![
                InputEvent::PointerUp { x: 0.2, y: 0.3, button: 0 },
                InputEvent::PointerUp { x: 0.2, y: 0.3, button: 2 },
                InputEvent::Focus { focused: false },
            ]
        );
    }

    #[test]
    fn a_pointer_frame_combines_axes_and_prefers_value120() {
        let geometry = geometry(100, 100, 100, 100, None);
        let mut pointer = PointerInputState::default();
        pointer.enter(50.0, 50.0, geometry);
        pointer.axis(PointerAxis::Horizontal, 2.0);
        pointer.axis(PointerAxis::Vertical, -3.0);
        pointer.axis_value120(PointerAxis::Horizontal, 120);
        pointer.axis_value120(PointerAxis::Vertical, -240);
        assert_eq!(
            pointer.finish_axis_frame(geometry),
            Some(InputEvent::PointerWheel { x: 0.5, y: 0.5, delta_x: -120, delta_y: 240 })
        );
    }

    #[test]
    fn discrete_scroll_steps_use_120_units_and_wayland_signs_are_inverted() {
        let geometry = geometry(100, 100, 100, 100, None);
        let mut pointer = PointerInputState::default();
        pointer.enter(50.0, 50.0, geometry);
        pointer.axis(PointerAxis::Horizontal, 9.0);
        pointer.axis(PointerAxis::Vertical, -9.0);
        pointer.axis_discrete(PointerAxis::Horizontal, 1);
        pointer.axis_discrete(PointerAxis::Vertical, -2);

        assert_eq!(
            pointer.finish_axis_frame(geometry),
            Some(InputEvent::PointerWheel { x: 0.5, y: 0.5, delta_x: -120, delta_y: 240 })
        );
    }

    #[test]
    fn smooth_scroll_keeps_sub_unit_remainders() {
        let geometry = geometry(100, 100, 100, 100, None);
        let mut pointer = PointerInputState::default();
        pointer.enter(50.0, 50.0, geometry);
        pointer.axis(PointerAxis::Vertical, 0.6);
        assert_eq!(pointer.finish_axis_frame(geometry), None);
        pointer.axis(PointerAxis::Vertical, 0.6);
        assert_eq!(
            pointer.finish_axis_frame(geometry),
            Some(InputEvent::PointerWheel { x: 0.5, y: 0.5, delta_x: 0, delta_y: -1 })
        );
    }

    #[test]
    fn axis_stop_does_not_carry_a_fraction_into_the_next_gesture() {
        let geometry = geometry(100, 100, 100, 100, None);
        let mut pointer = PointerInputState::default();
        pointer.enter(50.0, 50.0, geometry);
        pointer.axis(PointerAxis::Vertical, 0.6);
        pointer.axis_stop(PointerAxis::Vertical);
        assert_eq!(pointer.finish_axis_frame(geometry), None);

        pointer.axis(PointerAxis::Vertical, 0.6);
        assert_eq!(pointer.finish_axis_frame(geometry), None);
    }

    #[test]
    fn invalid_geometry_and_positions_are_ignored() {
        assert_eq!(map_surface_position(1.0, 1.0, geometry(0, 100, 100, 100, None)), None);
        assert_eq!(map_surface_position(f64::NAN, 1.0, geometry(100, 100, 100, 100, None)), None);
    }
}
