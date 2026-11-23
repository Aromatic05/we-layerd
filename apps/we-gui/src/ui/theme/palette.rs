//! Colors that follow the desktop appearance.
//!
//! On GNOME the colour scheme (light/dark) and the accent colour come from the
//! freedesktop settings portal (`org.freedesktop.appearance`), the same source
//! libadwaita apps use. The palette below mirrors libadwaita's named colours so
//! the window looks like a native GNOME app and follows the user's accent.

use std::sync::RwLock;

use iced::{theme::Palette, Color, Theme};

/// What the desktop asked for.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Appearance {
    pub dark: bool,
    /// Accent background colour; `None` means the desktop did not set one.
    pub accent: Option<Color>,
}

/// Semantic colours used by every widget style in the app.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Colors {
    pub dark: bool,
    /// Accent used as a fill (buttons, checkboxes, slider, selected menu row).
    pub accent_bg: Color,
    pub accent_bg_hover: Color,
    /// Text/icons drawn on `accent_bg`.
    pub on_accent: Color,
    /// Accent used as a foreground (links, headings, focus rings) — adjusted
    /// for contrast against the window background, like libadwaita's
    /// standalone accent colour.
    pub accent: Color,
    /// Selected tab / chip / list row background.
    pub accent_container: Color,
    pub on_accent_container: Color,
    /// Text selection highlight.
    pub selection: Color,

    pub window_bg: Color,
    pub view_bg: Color,
    pub sidebar_bg: Color,
    pub card_bg: Color,
    pub popover_bg: Color,
    pub field_bg: Color,
    pub hover: Color,
    pub track: Color,

    pub fg: Color,
    pub fg_dim: Color,
    pub border: Color,
    pub outline: Color,

    pub error: Color,
    pub warning: Color,
    pub success: Color,
}

/// libadwaita's default accent (blue 3).
const ADWAITA_BLUE: Color = rgb(0x35, 0x84, 0xe4);

const fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color { r: r as f32 / 255.0, g: g as f32 / 255.0, b: b as f32 / 255.0, a: 1.0 }
}

/// `amount` of `top` over `base` (0 = base, 1 = top).
fn mix(base: Color, top: Color, amount: f32) -> Color {
    let t = amount.clamp(0.0, 1.0);
    Color {
        r: base.r + (top.r - base.r) * t,
        g: base.g + (top.g - base.g) * t,
        b: base.b + (top.b - base.b) * t,
        a: 1.0,
    }
}

fn luminance(color: Color) -> f32 {
    fn channel(c: f32) -> f32 {
        if c <= 0.03928 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    }
    0.2126 * channel(color.r) + 0.7152 * channel(color.g) + 0.0722 * channel(color.b)
}

fn contrast(a: Color, b: Color) -> f32 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

impl Colors {
    pub fn from_appearance(appearance: Appearance) -> Self {
        let dark = appearance.dark;
        let accent_bg = appearance.accent.unwrap_or(ADWAITA_BLUE);

        // libadwaita 1.6 named colours.
        let (window_bg, view_bg, sidebar_bg, popover_bg, fg) = if dark {
            (rgb(0x22, 0x22, 0x26), rgb(0x1d, 0x1d, 0x20), rgb(0x2e, 0x2e, 0x32), rgb(0x36, 0x36, 0x3a), rgb(0xff, 0xff, 0xff))
        } else {
            (rgb(0xfa, 0xfa, 0xfb), rgb(0xff, 0xff, 0xff), rgb(0xeb, 0xeb, 0xed), rgb(0xff, 0xff, 0xff), rgb(0x32, 0x32, 0x35))
        };
        // Adwaita draws these as translucent currentColor over the window.
        let card_bg = if dark { mix(window_bg, fg, 0.08) } else { view_bg };
        let field_bg = mix(window_bg, fg, 0.10);
        let hover = mix(window_bg, fg, 0.10);
        let track = mix(window_bg, fg, 0.20);
        let border = mix(window_bg, fg, if dark { 0.15 } else { 0.15 });
        let outline = mix(window_bg, fg, 0.40);
        let fg_dim = mix(window_bg, fg, 0.70);

        let white = rgb(0xff, 0xff, 0xff);
        let black = rgb(0, 0, 0);
        let on_accent = if contrast(accent_bg, white) >= 3.0 { white } else { mix(white, black, 0.8) };
        let accent_bg_hover = mix(accent_bg, on_accent, 0.10);

        // Standalone accent: nudge towards fg until it reads on the window.
        let toward = if dark { white } else { black };
        let mut accent = accent_bg;
        for step in 1..=20 {
            if contrast(accent, window_bg) >= 4.5 {
                break;
            }
            accent = mix(accent_bg, toward, step as f32 * 0.05);
        }

        let accent_container = mix(window_bg, accent_bg, if dark { 0.35 } else { 0.22 });
        let selection = mix(field_bg, accent_bg, 0.45);

        let (error, warning, success) = if dark {
            (rgb(0xff, 0x7b, 0x63), rgb(0xf8, 0xe4, 0x5c), rgb(0x78, 0xe9, 0xab))
        } else {
            (rgb(0xc3, 0x00, 0x00), rgb(0x9c, 0x6e, 0x03), rgb(0x1b, 0x85, 0x53))
        };

        Self {
            dark,
            accent_bg,
            accent_bg_hover,
            on_accent,
            accent,
            accent_container,
            on_accent_container: fg,
            selection,
            window_bg,
            view_bg,
            sidebar_bg,
            card_bg,
            popover_bg,
            field_bg,
            hover,
            track,
            fg,
            fg_dim,
            border,
            outline,
            error,
            warning,
            success,
        }
    }

    pub fn theme(&self) -> Theme {
        Theme::custom(
            if self.dark { "GNOME Dark" } else { "GNOME Light" },
            Palette {
                background: self.window_bg,
                text: self.fg,
                primary: self.accent_bg,
                success: self.success,
                warning: self.warning,
                danger: self.error,
            },
        )
    }
}

static CURRENT: RwLock<Option<Colors>> = RwLock::new(None);

/// The colours for the current desktop appearance.
pub fn colors() -> Colors {
    if let Some(colors) = *CURRENT.read().unwrap_or_else(|e| e.into_inner()) {
        return colors;
    }
    Colors::from_appearance(Appearance { dark: true, accent: None })
}

/// Re-read the desktop appearance on this thread and store it.
///
/// Blocking: reads the settings portal synchronously, so only startup may call it. The periodic
/// check runs [`detect`] on a worker thread and applies the result through [`store`].
pub fn refresh() -> Option<Theme> {
    store(detect_appearance())
}

/// Store an appearance that was read off the UI thread and return the theme if it changed.
pub fn store(appearance: Appearance) -> Option<Theme> {
    let colors = Colors::from_appearance(appearance);
    let mut current = CURRENT.write().unwrap_or_else(|e| e.into_inner());
    if *current == Some(colors) {
        return None;
    }
    *current = Some(colors);
    Some(colors.theme())
}

/// Read the desktop appearance. Blocking, so run it on a worker thread.
pub fn detect() -> Appearance {
    detect_appearance()
}

/// The theme for the current appearance, detecting it if needed.
pub fn current_theme() -> Theme {
    refresh().unwrap_or_else(|| colors().theme())
}

fn detect_appearance() -> Appearance {
    #[cfg(target_os = "linux")]
    if let Some(appearance) = portal::read() {
        return appearance;
    }
    let dark = !matches!(dark_light::detect(), dark_light::Mode::Light);
    Appearance { dark, accent: None }
}

#[cfg(target_os = "linux")]
mod portal {
    use gtk::gio;
    use gtk::glib::Variant;
    use iced::Color;

    use super::Appearance;

    const NAMESPACE: &str = "org.freedesktop.appearance";

    pub(super) fn read() -> Option<Appearance> {
        let connection = gio::bus_get_sync(gio::BusType::Session, None::<&gio::Cancellable>).ok()?;
        // 1 = prefer dark, 2 = prefer light, 0 = no preference (GNOME: light).
        let scheme = read_setting(&connection, "color-scheme")?.get::<u32>()?;
        let accent = read_setting(&connection, "accent-color")
            .and_then(|value| value.get::<(f64, f64, f64)>())
            .filter(|(r, g, b)| [r, g, b].iter().all(|c| (0.0..=1.0).contains(*c)))
            .map(|(r, g, b)| Color::from_rgb(r as f32, g as f32, b as f32));
        Some(Appearance { dark: scheme == 1, accent })
    }

    fn read_setting(connection: &gio::DBusConnection, key: &str) -> Option<Variant> {
        let call = |method: &str| {
            connection
                .call_sync(
                    Some("org.freedesktop.portal.Desktop"),
                    "/org/freedesktop/portal/desktop",
                    "org.freedesktop.portal.Settings",
                    method,
                    Some(&(NAMESPACE, key).into()),
                    None,
                    gio::DBusCallFlags::NONE,
                    500,
                    None::<&gio::Cancellable>,
                )
                .ok()
        };
        // ReadOne returns (v); the deprecated Read returns (v) wrapping another v.
        if let Some(reply) = call("ReadOne") {
            return reply.child_value(0).as_variant();
        }
        let outer = call("Read")?.child_value(0).as_variant()?;
        Some(outer.as_variant().unwrap_or(outer))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accent_follows_desktop_and_stays_readable() {
        let red = rgb(0xe6, 0x2d, 0x42);
        for dark in [true, false] {
            let colors = Colors::from_appearance(Appearance { dark, accent: Some(red) });
            assert_eq!(colors.accent_bg, red);
            assert!(contrast(colors.accent, colors.window_bg) >= 4.5);
            assert!(contrast(colors.on_accent, colors.accent_bg) >= 3.0);
            assert!(contrast(colors.fg, colors.window_bg) >= 7.0);
        }
    }

    #[test]
    fn defaults_to_adwaita_blue() {
        let colors = Colors::from_appearance(Appearance { dark: false, accent: None });
        assert_eq!(colors.accent_bg, ADWAITA_BLUE);
        assert!(!colors.dark);
    }
}
