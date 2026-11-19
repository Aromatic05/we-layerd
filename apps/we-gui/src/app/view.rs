use crate::ui::theme::colors;
use iced::{
    alignment::Horizontal,
    widget::{container, pane_grid, stack, text},
    window, Element, Fill,
};

use crate::{
    domain::{
        i18n::Text,
        ui_state::{Pane, Sidebar},
    },
    ui::sidebar::{detail, playlist, profile, scenes, settings},
};

use super::{App, Message};

pub(crate) fn view(app: &App) -> Element<'_, Message> {
    let library = crate::ui::library::view(app);
    let content = if let Some(sidebar) = app.sidebar {
        pane_grid(&app.panes, |_pane, pane, _| {
            let content: Element<'_, Message> = match pane {
                Pane::Library => crate::ui::library::view(app),
                Pane::Sidebar => sidebar_view(app, sidebar),
            };
            pane_grid::Content::new(content)
        })
        .on_resize(8, Message::PaneResized)
        .spacing(1)
        .into()
    } else {
        library
    };

    let content: Element<'_, Message> = match &app.aspect_confirm {
        Some(warnings) => stack![content, aspect_confirm_dialog(app, warnings)].into(),
        None => content,
    };

    if app.layerd_available {
        content
    } else {
        stack![
            content,
            container(
                text(
                    app.language.runtime_status(
                        &crate::domain::runtime_status::RuntimeStatus::DaemonNotFound,
                    )
                )
                .size(18)
                .color(colors().error),
            )
            .width(Fill)
            .align_x(Horizontal::Center)
            .padding(16),
        ]
        .into()
    }
}

pub(crate) fn daemon_view(app: &App, _window: window::Id) -> Element<'_, Message> {
    view(app)
}

fn sidebar_view(app: &App, sidebar: Sidebar) -> Element<'_, Message> {
    match sidebar {
        Sidebar::Settings => settings::build_settings_overlay(
            &app.ui_settings,
            app.language,
            &app.runtime_status,
            app.autostart_enabled,
            app.autostart_system_managed,
            app.autostart_pending,
            app.autostart_error.as_deref(),
        ),
        Sidebar::Playlist => playlist::view(app),
        Sidebar::Profile => profile::view(app),
        Sidebar::Scenes => scenes::view(app),
        Sidebar::Detail => match app
            .selected_id
            .as_deref()
            .and_then(|id| app.entries.iter().find(|entry| entry.id == id))
        {
            Some(entry) => detail::view(detail::DetailViewState {
                entry,
                settings: app
                    .launch_settings
                    .wallpapers
                    .get(&entry.id)
                    .expect("selected wallpaper must have a profile"),
                schema: &app.selected_schema,
                property_filter: &app.property_filter,
                resolution_width: &app.resolution_width,
                resolution_height: &app.resolution_height,
                active_tab: app.detail_tab,
                is_running: app.selected_wallpaper_is_running(),
                is_paused: app.playback_paused,
                outputs: &app.outputs,
                selected_outputs: &app.selected_outputs,
                language: app.language,
            })
            .map(Message::Detail),
            None => {
                container(text(app.language.text(Text::SelectWallpaperDetails))).padding(24).into()
            }
        },
    }
}


/// Modal shown before applying a wallpaper whose shape doesn't match a target
/// display (e.g. a portrait wallpaper on a landscape monitor).
fn aspect_confirm_dialog<'a>(app: &'a App, warnings: &'a [String]) -> Element<'a, Message> {
    use crate::ui::sidebar::detail::{outlined_button_style, section_style};
    use iced::widget::{button, center, column, opaque, row};

    let language = app.language;
    let card = container(
        column![
            text(language.text(Text::AspectConfirmTitle)).size(18),
            column(warnings.iter().map(|warning| {
                text(warning.as_str()).size(13).color(colors().warning).into()
            }))
            .spacing(6),
            row![
                button(text(language.text(Text::AspectConfirmCancel)).size(14))
                    .on_press(Message::AspectConfirmCancel)
                    .padding([8, 16])
                    .style(outlined_button_style),
                button(text(language.text(Text::AspectConfirmApply)).size(14))
                    .on_press(Message::AspectConfirmApply)
                    .padding([8, 16])
                    .style(|theme, status| iced::widget::button::primary(theme, status)),
            ]
            .spacing(10),
        ]
        .spacing(14)
        .max_width(520),
    )
    .padding(20)
    .style(section_style);

    opaque(
        center(card).style(|_theme| container::Style {
            background: Some(iced::Background::Color(iced::Color { a: 0.55, ..iced::Color::BLACK })),
            ..Default::default()
        }),
    )
}
