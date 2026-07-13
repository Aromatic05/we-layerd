use std::fmt;

use iced::{
    widget::{button, checkbox, column, container, pick_list, row, scrollable, text, text_input},
    Background, Border, Color, Element, Fill, Theme,
};
use we_core::config::ScaleMode;

use crate::Message;

#[derive(Debug, Clone)]
pub struct UiSettings {
    pub assets_path: String,
    pub workshop_path: String,
    pub renderer_library_path: String,
    pub renderer_cache_path: String,
    pub prefer_dmabuf: bool,
    pub allow_shm_fallback: bool,
    pub interactive: bool,
    pub fps_limit: String,
    pub show_fps: bool,
    pub scale_mode: ScaleModeOption,
    pub status_text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScaleModeOption {
    Fit,
    Cover,
    Stretch,
}

impl fmt::Display for ScaleModeOption {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Fit => write!(f, "Fit"),
            Self::Cover => write!(f, "Cover"),
            Self::Stretch => write!(f, "Stretch"),
        }
    }
}

impl From<ScaleMode> for ScaleModeOption {
    fn from(value: ScaleMode) -> Self {
        match value {
            ScaleMode::Fit => Self::Fit,
            ScaleMode::Cover => Self::Cover,
            ScaleMode::Stretch => Self::Stretch,
        }
    }
}

impl From<ScaleModeOption> for ScaleMode {
    fn from(value: ScaleModeOption) -> Self {
        match value {
            ScaleModeOption::Fit => Self::Fit,
            ScaleModeOption::Cover => Self::Cover,
            ScaleModeOption::Stretch => Self::Stretch,
        }
    }
}

pub fn build_settings_overlay<'a>(ui_settings: &'a UiSettings) -> Element<'a, Message> {
    let assets_path_display = format_path_for_display(&ui_settings.assets_path, 64);
    let workshop_path_display = format_path_for_display(&ui_settings.workshop_path, 64);
    let library_path_display = format_path_for_display(&ui_settings.renderer_library_path, 64);
    let cache_path_display = format_path_for_display(&ui_settings.renderer_cache_path, 64);

    let content = column![
        row![
            column![text("Settings").size(26), text("Renderer and library preferences").size(13)].spacing(3).width(Fill),
            button(text("Close")).on_press(Message::SettingsPressed).style(outlined_button_style),
        ].align_y(iced::Alignment::Center),
        section_title("Wallpaper Engine"),
        text("Wallpaper Engine Assets Path").size(14),
        row![
            text_input("/path/to/wallpaper_engine/assets", &ui_settings.assets_path)
                .on_input(Message::AssetsPathChanged)
                .padding(10)
                .on_submit(Message::AutoScan)
                .width(Fill),
            button(text("Browse")).on_press(Message::PickAssetsPath),
        ]
        .spacing(10),
        text(assets_path_display).size(12),
        text("Workshop Path").size(14),
        row![
            text_input("/path/to/workshop/content/431960", &ui_settings.workshop_path)
                .on_input(Message::WorkshopPathChanged)
                .padding(10)
                .on_submit(Message::AutoScan)
                .width(Fill),
            button(text("Browse")).on_press(Message::PickWorkshopPath),
        ]
        .spacing(10),
        text(workshop_path_display).size(12),
        section_title("Renderer"),
        text("Renderer Library").size(14),
        text_input("Leave blank for automatic search", &ui_settings.renderer_library_path)
            .on_input(Message::RendererLibraryPathChanged)
            .padding(10),
        text(library_path_display).size(12),
        text("Renderer Cache Path").size(14),
        text_input("~/.cache/we-layerd/renderer", &ui_settings.renderer_cache_path)
            .on_input(Message::RendererCachePathChanged)
            .padding(10),
        text(cache_path_display).size(12),
        section_title("Presentation"),
        text("Frame Rate Limit (FPS)").size(14),
        text_input("60", &ui_settings.fps_limit).on_input(Message::FpsLimitChanged).padding(10),
        text("Scale Mode").size(14),
        pick_list(
            vec![ScaleModeOption::Fit, ScaleModeOption::Cover, ScaleModeOption::Stretch],
            Some(ui_settings.scale_mode),
            Message::ScaleModeSelected,
        )
        .padding(10),
        section_title("Behaviour"),
        checkbox(ui_settings.interactive)
            .label("Enable wallpaper input")
            .on_toggle(Message::InteractiveToggled),
        checkbox(ui_settings.show_fps)
            .label("Show realtime FPS")
            .on_toggle(Message::ShowFpsToggled),
        checkbox(ui_settings.prefer_dmabuf)
            .label("Prefer DMA-BUF presentation")
            .on_toggle(Message::PreferDmabufToggled),
        checkbox(ui_settings.allow_shm_fallback)
            .label("Allow SHM fallback")
            .on_toggle(Message::AllowShmFallbackToggled),
        section_title("Runtime status"),
        container(text(&ui_settings.status_text).size(14))
            .padding(12)
            .width(Fill)
            .style(status_box_style),
    ]
    .spacing(10);

    container(scrollable(content).height(Fill))
        .width(Fill)
        .height(Fill)
        .padding(18)
        .style(settings_overlay_style)
        .into()
}

fn format_path_for_display(path: &str, limit: usize) -> String {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    if trimmed.chars().count() <= limit {
        return trimmed.to_string();
    }

    let head_len = limit / 2;
    let tail_len = limit.saturating_sub(head_len + 1);
    let head = trimmed.chars().take(head_len).collect::<String>();
    let tail =
        trimmed.chars().rev().take(tail_len).collect::<String>().chars().rev().collect::<String>();
    format!("{head}…{tail}")
}

fn settings_overlay_style(theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(if matches!(theme, Theme::Light) { Color::from_rgb8(247, 245, 250) } else { Color::from_rgb8(30, 31, 34) })),
        border: Border {
            radius: 0.0.into(),
            width: 0.0,
            color: Color::TRANSPARENT,
        },
        ..Default::default()
    }
}

fn status_box_style(theme: &Theme) -> container::Style {
    let is_light = matches!(theme, Theme::Light);
    container::Style {
        background: Some(Background::Color(if is_light {
            Color::from_rgba(0.95, 0.97, 0.99, 1.0)
        } else {
            Color::from_rgba(0.12, 0.14, 0.16, 1.0)
        })),
        border: Border {
            radius: 16.0.into(),
            width: 1.0,
            color: if is_light {
                Color::from_rgba(0.0, 0.0, 0.0, 0.06)
            } else {
                Color::from_rgba(1.0, 1.0, 1.0, 0.06)
            },
        },
        ..Default::default()
    }
}

fn section_title<'a>(title: &'a str) -> iced::widget::Text<'a> {
    text(title).size(16).color(Color::from_rgb8(178, 198, 255))
}

fn outlined_button_style(_theme: &Theme, _status: button::Status) -> button::Style {
    button::Style {
        text_color: Color::from_rgb8(198, 210, 242),
        border: Border { radius: 20.0.into(), width: 1.0, color: Color::from_rgb8(143, 147, 156) },
        ..Default::default()
    }
}
