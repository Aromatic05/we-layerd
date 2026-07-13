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
            button(text("×").size(22)).on_press(Message::SettingsPressed).style(outlined_button_style),
        ].align_y(iced::Alignment::Center),
        section_title("Wallpaper Engine"),
        text("Wallpaper Engine Assets Path").size(14),
        row![
            text_input("/path/to/wallpaper_engine/assets", &ui_settings.assets_path)
                .on_input(Message::AssetsPathChanged)
                .padding(10).style(md_text_input_style)
                .on_submit(Message::AutoScan)
                .width(Fill),
            button(text("…").size(20)).on_press(Message::PickAssetsPath).style(outlined_button_style),
        ]
        .spacing(10),
        text(assets_path_display).size(12),
        text("Workshop Path").size(14),
        row![
            text_input("/path/to/workshop/content/431960", &ui_settings.workshop_path)
                .on_input(Message::WorkshopPathChanged)
                .padding(10).style(md_text_input_style)
                .on_submit(Message::AutoScan)
                .width(Fill),
            button(text("…").size(20)).on_press(Message::PickWorkshopPath).style(outlined_button_style),
        ]
        .spacing(10),
        text(workshop_path_display).size(12),
        section_title("Renderer"),
        text("Renderer Library").size(14),
        text_input("Leave blank for automatic search", &ui_settings.renderer_library_path)
            .on_input(Message::RendererLibraryPathChanged)
            .padding(10).style(md_text_input_style),
        text(library_path_display).size(12),
        text("Renderer Cache Path").size(14),
        text_input("~/.cache/we-layerd/renderer", &ui_settings.renderer_cache_path)
            .on_input(Message::RendererCachePathChanged)
            .padding(10).style(md_text_input_style),
        text(cache_path_display).size(12),
        section_title("Presentation"),
        text("Frame Rate Limit (FPS)").size(14),
        text_input("60", &ui_settings.fps_limit).on_input(Message::FpsLimitChanged).padding(10).style(md_text_input_style),
        text("Scale Mode").size(14),
        pick_list(
            vec![ScaleModeOption::Fit, ScaleModeOption::Cover, ScaleModeOption::Stretch],
            Some(ui_settings.scale_mode),
            Message::ScaleModeSelected,
        )
        .padding(10).style(md_pick_list_style).menu_style(md_menu_style),
        section_title("Behaviour"),
        checkbox(ui_settings.interactive)
            .label("Enable wallpaper input")
            .on_toggle(Message::InteractiveToggled).style(md_checkbox_style),
        checkbox(ui_settings.show_fps)
            .label("Show realtime FPS")
            .on_toggle(Message::ShowFpsToggled).style(md_checkbox_style),
        checkbox(ui_settings.prefer_dmabuf)
            .label("Prefer DMA-BUF presentation")
            .on_toggle(Message::PreferDmabufToggled).style(md_checkbox_style),
        checkbox(ui_settings.allow_shm_fallback)
            .label("Allow SHM fallback")
            .on_toggle(Message::AllowShmFallbackToggled).style(md_checkbox_style),
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

fn md_text_input_style(_theme: &Theme, status: iced::widget::text_input::Status) -> iced::widget::text_input::Style {
    let border = if matches!(status, iced::widget::text_input::Status::Focused { .. }) { Color::from_rgb8(174, 198, 255) } else { Color::from_rgb8(143, 147, 156) };
    iced::widget::text_input::Style {
        background: Background::Color(Color::from_rgb8(43, 44, 48)),
        border: Border { radius: 8.0.into(), width: 1.0, color: border },
        icon: Color::from_rgb8(196, 199, 204),
        placeholder: Color::from_rgb8(196, 199, 204),
        value: Color::from_rgb8(230, 225, 229),
        selection: Color::from_rgb8(78, 99, 139),
    }
}

fn md_checkbox_style(_theme: &Theme, status: iced::widget::checkbox::Status) -> iced::widget::checkbox::Style {
    let checked = matches!(status, iced::widget::checkbox::Status::Active { is_checked: true } | iced::widget::checkbox::Status::Hovered { is_checked: true } | iced::widget::checkbox::Status::Disabled { is_checked: true });
    iced::widget::checkbox::Style {
        background: Background::Color(if checked { Color::from_rgb8(174, 198, 255) } else { Color::from_rgb8(43, 44, 48) }),
        icon_color: Color::from_rgb8(26, 39, 64),
        border: Border { radius: 2.0.into(), width: if checked { 0.0 } else { 2.0 }, color: Color::from_rgb8(196, 199, 204) },
        text_color: Some(Color::from_rgb8(230, 225, 229)),
    }
}

fn md_pick_list_style(_theme: &Theme, status: iced::widget::pick_list::Status) -> iced::widget::pick_list::Style {
    iced::widget::pick_list::Style {
        text_color: Color::from_rgb8(230, 225, 229),
        placeholder_color: Color::from_rgb8(196, 199, 204),
        handle_color: Color::from_rgb8(196, 199, 204),
        background: Background::Color(Color::from_rgb8(43, 44, 48)),
        border: Border { radius: 8.0.into(), width: 1.0, color: if matches!(status, iced::widget::pick_list::Status::Opened { .. }) { Color::from_rgb8(174, 198, 255) } else { Color::from_rgb8(143, 147, 156) } },
    }
}

fn md_menu_style(_theme: &Theme) -> iced::overlay::menu::Style {
    iced::overlay::menu::Style {
        background: Background::Color(Color::from_rgb8(48, 49, 53)),
        border: Border { radius: 8.0.into(), width: 1.0, color: Color::from_rgb8(72, 74, 80) },
        text_color: Color::from_rgb8(230, 225, 229),
        selected_text_color: Color::from_rgb8(26, 39, 64),
        selected_background: Background::Color(Color::from_rgb8(174, 198, 255)),
        shadow: iced::Shadow { color: Color::from_rgba8(0, 0, 0, 0.35), blur_radius: 12.0, offset: iced::Vector::new(0.0, 4.0) },
    }
}
