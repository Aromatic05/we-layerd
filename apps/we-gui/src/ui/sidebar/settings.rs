use crate::{
    app::Message,
    domain::{
        i18n::{Language, Localized, Text},
        runtime_status::RuntimeStatus,
        settings::{ScaleModeOption, UiSettings},
    },
    ui::theme::scrollbar,
};
use iced::{
    widget::{button, checkbox, column, container, pick_list, row, scrollable, text, text_input},
    Background, Border, Color, Element, Fill, Theme,
};

pub fn build_settings_overlay<'a>(
    ui_settings: &'a UiSettings,
    language: Language,
    runtime_status: &'a RuntimeStatus,
) -> Element<'a, Message> {
    let assets_path_display = format_path_for_display(&ui_settings.assets_path, 64);
    let workshop_path_display = format_path_for_display(&ui_settings.workshop_path, 64);
    let library_path_display = format_path_for_display(&ui_settings.renderer_library_path, 64);
    let cache_path_display = format_path_for_display(&ui_settings.renderer_cache_path, 64);
    let scale_options = vec![
        Localized::new(ScaleModeOption::Fit, language.text(Text::ScaleFit)),
        Localized::new(ScaleModeOption::Cover, language.text(Text::ScaleCover)),
        Localized::new(ScaleModeOption::Stretch, language.text(Text::ScaleStretch)),
    ];
    let selected_scale =
        scale_options.iter().find(|option| option.value == ui_settings.scale_mode).cloned();

    let content = column![
        row![
            column![
                text(language.text(Text::Settings)).size(26),
                text(language.text(Text::SettingsSubtitle)).size(13),
            ]
            .spacing(3)
            .width(Fill),
            container(
                button(text(language.text(Text::CloseSettings)).size(14))
                    .on_press(Message::SettingsPressed)
                    .style(outlined_button_style),
            )
            .id("settings.close"),
        ]
        .align_y(iced::Alignment::Center),
        section_title(language.text(Text::Language)),
        container(
            pick_list(Language::ALL.to_vec(), Some(language), Message::LanguageSelected,)
                .padding([14, 10])
                .style(md_pick_list_style)
                .menu_style(md_menu_style),
        )
        .id("settings.language"),
        section_title(language.text(Text::WallpaperEngine)),
        text(language.text(Text::AssetsPath)).size(14),
        row![
            text_input("/path/to/wallpaper_engine", &ui_settings.assets_path)
                .id("settings.assets-path")
                .on_input(Message::AssetsPathChanged)
                .padding([14, 10])
                .style(md_text_input_style)
                .on_submit(Message::AutoScan)
                .width(Fill),
            container(
                button(text(language.text(Text::Browse)).size(14))
                    .on_press(Message::PickAssetsPath)
                    .style(outlined_button_style),
            )
            .id("settings.assets-path.browse"),
        ]
        .spacing(10),
        text(assets_path_display).size(12),
        text(language.text(Text::WorkshopPath)).size(14),
        row![
            text_input("/path/to/workshop/content/431960", &ui_settings.workshop_path)
                .id("settings.workshop-path")
                .on_input(Message::WorkshopPathChanged)
                .padding([14, 10])
                .style(md_text_input_style)
                .on_submit(Message::AutoScan)
                .width(Fill),
            container(
                button(text(language.text(Text::Browse)).size(14))
                    .on_press(Message::PickWorkshopPath)
                    .style(outlined_button_style),
            )
            .id("settings.workshop-path.browse"),
        ]
        .spacing(10),
        text(workshop_path_display).size(12),
        section_title(language.text(Text::Renderer)),
        text(language.text(Text::RendererLibrary)).size(14),
        text_input(language.text(Text::AutomaticSearch), &ui_settings.renderer_library_path)
            .id("settings.renderer-library")
            .on_input(Message::RendererLibraryPathChanged)
            .padding([14, 10])
            .style(md_text_input_style),
        text(library_path_display).size(12),
        text(language.text(Text::RendererCachePath)).size(14),
        text_input("~/.cache/we-layerd/renderer", &ui_settings.renderer_cache_path)
            .id("settings.renderer-cache")
            .on_input(Message::RendererCachePathChanged)
            .padding([14, 10])
            .style(md_text_input_style),
        text(cache_path_display).size(12),
        section_title(language.text(Text::Presentation)),
        text(language.text(Text::FrameRateLimit)).size(14),
        text_input("60", &ui_settings.fps_limit)
            .id("settings.fps-limit")
            .on_input(Message::FpsLimitChanged)
            .padding([14, 10])
            .style(md_text_input_style),
        text(language.text(Text::ScaleMode)).size(14),
        container(
            pick_list(scale_options, selected_scale, |option| Message::ScaleModeSelected(
                option.value
            ),)
            .padding([14, 10])
            .style(md_pick_list_style)
            .menu_style(md_menu_style),
        )
        .id("settings.scale-mode"),
        section_title(language.text(Text::Behaviour)),
        container(
            checkbox(ui_settings.interactive)
                .label(language.text(Text::EnableWallpaperInput))
                .on_toggle(Message::InteractiveToggled)
                .style(md_checkbox_style),
        )
        .id("settings.wallpaper-input"),
        container(
            checkbox(ui_settings.force_scene_audio_loop)
                .label(language.text(Text::ForceSceneAudioLoop))
                .on_toggle(Message::ForceSceneAudioLoopToggled)
                .style(md_checkbox_style),
        )
        .id("settings.audio.force-loop"),
        text(language.text(Text::ForceSceneAudioLoopDescription)).size(12),
        container(
            checkbox(ui_settings.show_fps)
                .label(language.text(Text::ShowRealtimeFps))
                .on_toggle(Message::ShowFpsToggled)
                .style(md_checkbox_style),
        )
        .id("settings.show-fps"),
        container(
            checkbox(ui_settings.prefer_dmabuf)
                .label(language.text(Text::PreferDmabuf))
                .on_toggle(Message::PreferDmabufToggled)
                .style(md_checkbox_style),
        )
        .id("settings.prefer-dmabuf"),
        container(
            checkbox(ui_settings.allow_shm_fallback)
                .label(language.text(Text::AllowShmFallback))
                .on_toggle(Message::AllowShmFallbackToggled)
                .style(md_checkbox_style),
        )
        .id("settings.allow-shm-fallback"),
        section_title(language.text(Text::RendererDiagnostics)),
        renderer_diagnostics_view(runtime_status, language),
        section_title(language.text(Text::RuntimeStatus)),
        container(text(language.runtime_status(runtime_status)).size(14))
            .id("settings.runtime-status")
            .padding(12)
            .width(Fill)
            .style(status_box_style),
    ]
    .spacing(10);

    container(
        scrollable(container(content).padding(iced::Padding {
            top: 0.0,
            right: 18.0,
            bottom: 0.0,
            left: 0.0,
        }))
        .height(Fill)
        .direction(iced::widget::scrollable::Direction::Vertical(
            iced::widget::scrollable::Scrollbar::new().width(12).margin(6).scroller_width(6),
        ))
        .style(scrollbar::md_style),
    )
    .width(Fill)
    .height(Fill)
    .padding(18)
    .style(settings_overlay_style)
    .into()
}

fn renderer_diagnostics_view<'a>(
    runtime_status: &'a RuntimeStatus,
    language: Language,
) -> Element<'a, Message> {
    let diagnostics = renderer_diagnostics(runtime_status);
    if diagnostics.is_empty() {
        let message = renderer_diagnostics_error(runtime_status)
            .unwrap_or_else(|| language.text(Text::NoRendererDiagnostics).to_string());
        return container(text(message).size(13))
            .padding(12)
            .width(Fill)
            .style(status_box_style)
            .into();
    }

    let mut content = column!().spacing(8);
    for diagnostic in diagnostics {
        let severity = diagnostic.severity.to_ascii_uppercase();
        content = content.push(
            container(
                column![
                    text(format!("{severity} · {}", diagnostic.source)).size(12),
                    text(diagnostic.message).size(13),
                ]
                .spacing(4),
            )
            .padding(10)
            .width(Fill)
            .style(status_box_style),
        );
    }
    content.into()
}

#[derive(Debug)]
struct RendererDiagnosticView {
    severity: String,
    source: String,
    message: String,
}

fn renderer_diagnostics(runtime_status: &RuntimeStatus) -> Vec<RendererDiagnosticView> {
    let RuntimeStatus::Raw(raw) = runtime_status else {
        return Vec::new();
    };
    let Ok(document) = toml::from_str::<toml::Value>(raw) else {
        return Vec::new();
    };
    let mut diagnostics = Vec::new();
    if let Some(runtime) = document.get("runtime").and_then(toml::Value::as_table) {
        append_renderer_diagnostics(&mut diagnostics, runtime, None);
    }
    if let Some(outputs) = document.get("output_runtime").and_then(toml::Value::as_table) {
        for (output, value) in outputs {
            if let Some(runtime) = value.get("runtime").and_then(toml::Value::as_table) {
                append_renderer_diagnostics(&mut diagnostics, runtime, Some(output));
            }
        }
    }
    diagnostics
}

fn append_renderer_diagnostics(
    diagnostics: &mut Vec<RendererDiagnosticView>,
    runtime: &toml::value::Table,
    output: Option<&str>,
) {
    let Some(json) = runtime.get("renderer_diagnostics_json").and_then(toml::Value::as_str) else {
        return;
    };
    let Ok(payload) = serde_json::from_str::<serde_json::Value>(json) else {
        return;
    };
    let Some(entries) = payload.get("entries").and_then(serde_json::Value::as_array) else {
        return;
    };
    diagnostics.extend(entries.iter().filter_map(|entry| {
        let source = entry.get("source")?.as_str()?;
        Some(RendererDiagnosticView {
            severity: entry.get("severity")?.as_str()?.to_string(),
            source: output
                .map_or_else(|| source.to_string(), |output| format!("{output} · {source}")),
            message: entry.get("message")?.as_str()?.to_string(),
        })
    }));
}

fn renderer_diagnostics_error(runtime_status: &RuntimeStatus) -> Option<String> {
    let RuntimeStatus::Raw(raw) = runtime_status else {
        return None;
    };
    let document = toml::from_str::<toml::Value>(raw).ok()?;
    let mut errors = Vec::new();
    if let Some(error) = document
        .get("runtime")
        .and_then(|runtime| runtime.get("renderer_diagnostics_error"))
        .and_then(toml::Value::as_str)
        .filter(|value| !value.is_empty())
    {
        errors.push(error.to_string());
    }
    if let Some(outputs) = document.get("output_runtime").and_then(toml::Value::as_table) {
        for (output, value) in outputs {
            if let Some(error) = value
                .get("runtime")
                .and_then(|runtime| runtime.get("renderer_diagnostics_error"))
                .and_then(toml::Value::as_str)
                .filter(|value| !value.is_empty())
            {
                errors.push(format!("{output}: {error}"));
            }
        }
    }
    (!errors.is_empty()).then(|| errors.join("\n"))
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
        background: Some(Background::Color(if matches!(theme, Theme::Light) {
            Color::from_rgb8(247, 245, 250)
        } else {
            Color::from_rgb8(30, 31, 34)
        })),
        border: Border { radius: 0.0.into(), width: 0.0, color: Color::TRANSPARENT },
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

fn md_text_input_style(
    _theme: &Theme,
    status: iced::widget::text_input::Status,
) -> iced::widget::text_input::Style {
    let border = if matches!(status, iced::widget::text_input::Status::Focused { .. }) {
        Color::from_rgb8(174, 198, 255)
    } else {
        Color::from_rgb8(143, 147, 156)
    };
    iced::widget::text_input::Style {
        background: Background::Color(Color::from_rgb8(43, 44, 48)),
        border: Border { radius: 8.0.into(), width: 1.0, color: border },
        icon: Color::from_rgb8(196, 199, 204),
        placeholder: Color::from_rgb8(196, 199, 204),
        value: Color::from_rgb8(230, 225, 229),
        selection: Color::from_rgb8(78, 99, 139),
    }
}

fn md_checkbox_style(
    _theme: &Theme,
    status: iced::widget::checkbox::Status,
) -> iced::widget::checkbox::Style {
    let checked = matches!(
        status,
        iced::widget::checkbox::Status::Active { is_checked: true }
            | iced::widget::checkbox::Status::Hovered { is_checked: true }
            | iced::widget::checkbox::Status::Disabled { is_checked: true }
    );
    iced::widget::checkbox::Style {
        background: Background::Color(if checked {
            Color::from_rgb8(174, 198, 255)
        } else {
            Color::from_rgb8(43, 44, 48)
        }),
        icon_color: Color::from_rgb8(26, 39, 64),
        border: Border {
            radius: 2.0.into(),
            width: if checked { 0.0 } else { 2.0 },
            color: Color::from_rgb8(196, 199, 204),
        },
        text_color: Some(Color::from_rgb8(230, 225, 229)),
    }
}

fn md_pick_list_style(
    _theme: &Theme,
    status: iced::widget::pick_list::Status,
) -> iced::widget::pick_list::Style {
    iced::widget::pick_list::Style {
        text_color: Color::from_rgb8(230, 225, 229),
        placeholder_color: Color::from_rgb8(196, 199, 204),
        handle_color: Color::from_rgb8(196, 199, 204),
        background: Background::Color(Color::from_rgb8(43, 44, 48)),
        border: Border {
            radius: 8.0.into(),
            width: 1.0,
            color: if matches!(status, iced::widget::pick_list::Status::Opened { .. }) {
                Color::from_rgb8(174, 198, 255)
            } else {
                Color::from_rgb8(143, 147, 156)
            },
        },
    }
}

fn md_menu_style(_theme: &Theme) -> iced::overlay::menu::Style {
    iced::overlay::menu::Style {
        background: Background::Color(Color::from_rgb8(48, 49, 53)),
        border: Border { radius: 8.0.into(), width: 1.0, color: Color::from_rgb8(72, 74, 80) },
        text_color: Color::from_rgb8(230, 225, 229),
        selected_text_color: Color::from_rgb8(26, 39, 64),
        selected_background: Background::Color(Color::from_rgb8(174, 198, 255)),
        shadow: iced::Shadow {
            color: Color::from_rgba8(0, 0, 0, 0.35),
            blur_radius: 12.0,
            offset: iced::Vector::new(0.0, 4.0),
        },
    }
}
