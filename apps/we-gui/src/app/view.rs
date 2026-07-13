use iced::{
    alignment::Horizontal,
    widget::{container, pane_grid, stack, text},
    window, Color, Element, Fill,
};

use crate::{domain::ui_state::{Pane, Sidebar}, ui::sidebar::{detail, settings}};

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

    if app.layerd_available {
        content
    } else {
        stack![
            content,
            container(text("we-layerd not found in PATH").size(18).color(Color::from_rgb8(255, 180, 171)))
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
        Sidebar::Settings => settings::build_settings_overlay(&app.ui_settings),
        Sidebar::Detail => match app.selected_id.as_deref().and_then(|id| app.entries.iter().find(|entry| entry.id == id)) {
            Some(entry) => detail::view(
                entry,
                app.launch_settings.wallpapers.get(&entry.id).expect("selected wallpaper must have a profile"),
                &app.selected_schema,
                &app.resolution_width,
                &app.resolution_height,
                app.detail_tab,
                app.playback_running,
                app.playback_paused,
            )
            .map(Message::Detail),
            None => container(text("Select a wallpaper to view its details.")).padding(24).into(),
        },
    }
}
