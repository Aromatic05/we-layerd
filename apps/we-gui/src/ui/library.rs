use crate::ui::theme::colors;
use iced::{
    alignment::Vertical,
    widget::{
        button, column, container, image, responsive, row, scrollable, stack, text, text_input, Row,
    },
    Background, Border, Color, ContentFit, Element, Fill, Length, Theme,
};
use we_core::wallpaper::{WallpaperEntry, WallpaperType};

use crate::{
    app::{App, Message},
    domain::{
        i18n::{Language, Text},
        library_grid::{grid_window, GridWindow},
        ui_state::AnimatedPreview,
    },
};

pub(crate) fn view(app: &App) -> Element<'_, Message> {
    let language = app.language;
    let entries = &app.entries;
    let filtered_entry_indices = &app.filtered_entry_indices;
    let selected_id = app.selected_id.as_ref();
    let animated_previews = &app.animated_previews;
    let playlist_selected = app.playlist_selected.as_deref();
    let scroll_y = app.library_scroll_y;
    let viewport_height = app.library_viewport_height;
    let item_count = app.entries.len();
    let grid = responsive(move |size| {
        if filtered_entry_indices.is_empty() {
            return container(text(language.text(Text::NoMatchingWallpapers)).size(16))
                .width(Fill)
                .align_x(iced::alignment::Horizontal::Center)
                .padding(32)
                .into();
        }
        let layout = grid_window(
            filtered_entry_indices.len(),
            size.width,
            scroll_y,
            viewport_height,
            playlist_selected.is_some(),
        );
        build_wallpaper_grid(
            filtered_entry_indices[layout.start_item..layout.end_item]
                .iter()
                .copied()
                .filter_map(|index| entries.get(index).map(|entry| (index, entry))),
            layout,
            selected_id,
            animated_previews,
            playlist_selected,
            language,
        )
    });
    let search_query = app.search_query.as_str();
    let type_filter = app.type_filter;
    let toolbar = responsive(move |size| {
        let title = column![
            text(language.text(Text::Wallpapers)).size(28),
            text(language.item_count(item_count)).size(13)
        ]
        .spacing(2);
        if size.width >= 760.0 {
            row![title.width(Fill), top_bar_buttons(language)]
                .width(Fill)
                .align_y(Vertical::Center)
                .into()
        } else {
            column![title, top_bar_buttons(language).wrap().vertical_spacing(8)]
                .spacing(12)
                .into()
        }
    })
    .height(Length::Shrink);
    let search = responsive(move |size| {
        let input = text_input(language.text(Text::SearchWallpapers), search_query)
            .id("library.search")
            .on_input(Message::SearchChanged)
            .padding(12)
            .style(search_style);
        let filters = filter_chips(type_filter, language);
        if size.width >= 620.0 {
            row![input.width(Fill), filters].spacing(12).align_y(Vertical::Center).into()
        } else {
            column![input, filters.wrap().vertical_spacing(8)].spacing(8).into()
        }
    })
    .height(Length::Shrink);
    container(
        column![
            toolbar,
            search,
            scrollable(grid)
                .id("library.scroll")
                .on_scroll(|viewport| {
                    let bounds = viewport.bounds();
                    Message::LibraryScrolled {
                        offset_y: viewport.absolute_offset().y,
                        viewport_width: bounds.width,
                        viewport_height: bounds.height,
                    }
                })
                .width(Fill)
                .height(Fill),
        ]
        .spacing(16),
    )
    .padding(24)
    .width(Fill)
    .height(Fill)
    .style(library_style)
    .into()
}

fn top_bar_buttons(language: Language) -> Row<'static, Message> {
    row![
        container(
            button(text(format!("↻  {}", language.text(Text::RefreshLibrary))).size(16))
                .on_press(Message::RefreshLibrary)
                .style(top_bar_button_style),
        )
        .id("library.refresh"),
        container(
            button(text(format!("☷  {}", language.text(Text::Playlists))).size(16))
                .on_press(Message::PlaylistsPressed)
                .style(top_bar_button_style),
        )
        .id("library.playlists"),
        container(
            button(text(format!("▦  {}", language.text(Text::Profiles))).size(16))
                .on_press(Message::ProfilesPressed)
                .style(top_bar_button_style),
        )
        .id("library.profiles"),
        container(
            button(text(format!("◷  {}", language.text(Text::SmartScenes))).size(16))
                .on_press(Message::ScenesPressed)
                .style(top_bar_button_style),
        )
        .id("library.scenes"),
        container(
            button(text(format!("⚙  {}", language.text(Text::OpenSettings))).size(16))
                .on_press(Message::SettingsPressed)
                .style(top_bar_button_style),
        )
        .id("library.settings"),
    ]
}

fn filter_chips(type_filter: Option<WallpaperType>, language: Language) -> Row<'static, Message> {
    row![
        filter_chip(
            language.text(Text::FilterAll),
            "library.filter.all",
            type_filter.is_none(),
            None
        ),
        filter_chip(
            language.text(Text::FilterWeb),
            "library.filter.web",
            type_filter == Some(WallpaperType::Web),
            Some(WallpaperType::Web)
        ),
        filter_chip(
            language.text(Text::FilterScene),
            "library.filter.scene",
            type_filter == Some(WallpaperType::Scene),
            Some(WallpaperType::Scene)
        ),
        filter_chip(
            language.text(Text::FilterVideo),
            "library.filter.video",
            type_filter == Some(WallpaperType::Video),
            Some(WallpaperType::Video)
        ),
    ]
    .spacing(8)
}

fn build_wallpaper_grid<'a>(
    entries: impl Iterator<Item = (usize, &'a WallpaperEntry)>,
    layout: GridWindow,
    selected_id: Option<&String>,
    animated_previews: &'a std::collections::HashMap<std::path::PathBuf, AnimatedPreview>,
    playlist_selected: Option<&'a str>,
    language: Language,
) -> Element<'a, Message> {
    let spacing = 12.0;
    let mut root = column!().width(Fill);
    if layout.top_spacer > 0.0 {
        root = root.push(container(text("")).width(Fill).height(layout.top_spacer));
    }
    let entries = entries.collect::<Vec<_>>();

    for chunk in entries.chunks(layout.cols) {
        let mut row = row!().spacing(spacing);
        for (index, entry) in chunk.iter() {
            let selected = selected_id.is_some_and(|id| id == &entry.id);
            row = row.push(make_wallpaper_card(
                entry,
                *index,
                layout.card_width,
                selected,
                animated_previews,
                playlist_selected,
                language,
            ));
        }
        root = root.push(container(row).width(Fill).height(layout.row_pitch));
    }
    if layout.bottom_spacer > 0.0 {
        root = root.push(container(text("")).width(Fill).height(layout.bottom_spacer));
    }
    container(root).padding([0, 12]).width(Fill).into()
}

fn make_wallpaper_card<'a>(
    entry: &'a WallpaperEntry,
    index: usize,
    card_width: f32,
    selected: bool,
    animated_previews: &'a std::collections::HashMap<std::path::PathBuf, AnimatedPreview>,
    playlist_selected: Option<&'a str>,
    language: Language,
) -> Element<'a, Message> {
    let card_height = (card_width * 9.0 / 16.0).round();
    let media: Element<'a, Message> = if let Some(path) = &entry.preview {
        let handle = animated_previews
            .get(path)
            .map(|preview| preview.frames[preview.current].handle.clone())
            .unwrap_or_else(|| image::Handle::from_path(path));
        image(handle).width(card_width).height(card_height).content_fit(ContentFit::Cover).into()
    } else {
        container(text(""))
            .width(card_width)
            .height(card_height)
            .style(|_theme: &Theme| container::Style {
                background: Some(Background::Color(colors().card_bg)),
                ..Default::default()
            })
            .into()
    };

    let chip = container(text(wallpaper_type_name(entry.ty, language)).size(12))
        .padding([3, 8])
        .style(|_theme: &Theme| container::Style {
            text_color: Some(Color::WHITE),
            background: Some(Background::Color(Color { r: 0.0, g: 0.0, b: 0.0, a: 0.45 })),
            border: Border { radius: 10.0.into(), ..Default::default() },
            ..Default::default()
        });
    let title = container(text(&entry.title).size(13).color(Color::WHITE))
        .width(Fill)
        .padding([3, 8])
        .style(|_theme: &Theme| container::Style {
            background: Some(Background::Color(Color { r: 0.0, g: 0.0, b: 0.0, a: 0.45 })),
            border: Border { radius: 10.0.into(), ..Default::default() },
            ..Default::default()
        });
    let overlay = container(row![title, chip].spacing(8).align_y(Vertical::Center))
        .width(Fill)
        .height(Fill)
        .align_x(iced::alignment::Horizontal::Right)
        .align_y(Vertical::Bottom)
        .padding(8);
    let border_color =
        if selected { colors().accent_bg } else { colors().border };
    let frame = container(stack![media, overlay]).width(card_width).height(card_height).style(
        move |_theme: &Theme| container::Style {
            border: Border {
                radius: 16.0.into(),
                width: if selected { 2.0 } else { 1.0 },
                color: border_color,
            },
            shadow: if selected {
                iced::Shadow {
                    color: Color::from_rgba8(0, 0, 0, 0.35),
                    blur_radius: 8.0,
                    offset: iced::Vector::new(0.0, 2.0),
                }
            } else {
                iced::Shadow::default()
            },
            ..Default::default()
        },
    );
    let card =
        button(frame).on_press(Message::SelectWallpaper(index)).style(image_card_button_style);
    let mut content = column![card].spacing(6).width(card_width);
    if let Some(playlist_name) = playlist_selected {
        content = content.push(
            button(
                text(format!("+ {} · {}", language.text(Text::AddToPlaylist), playlist_name))
                    .size(12),
            )
            .on_press(Message::AddWallpaperToSelectedPlaylist(index))
            .width(Fill)
            .style(top_bar_button_style),
        );
    }
    container(content).id(format!("library.wallpaper.{}", entry.id)).into()
}

fn wallpaper_type_name(ty: WallpaperType, language: Language) -> &'static str {
    match ty {
        WallpaperType::Video => language.text(Text::TypeVideo),
        WallpaperType::Scene => language.text(Text::TypeScene),
        WallpaperType::Web => language.text(Text::TypeWeb),
        WallpaperType::Unknown => language.text(Text::TypeUnknown),
    }
}

fn image_card_button_style(_theme: &Theme, _status: button::Status) -> button::Style {
    button::Style {
        background: None,
        text_color: Color::WHITE,
        border: Border::default(),
        shadow: iced::Shadow::default(),
        ..Default::default()
    }
}

fn library_style(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(colors().view_bg)),
        ..Default::default()
    }
}

fn search_style(_theme: &Theme, status: text_input::Status) -> text_input::Style {
    let border = if matches!(status, text_input::Status::Focused { .. }) {
        colors().accent_bg
    } else {
        colors().outline
    };
    text_input::Style {
        background: Background::Color(colors().field_bg),
        border: Border { radius: 28.0.into(), width: 1.0, color: border },
        icon: colors().fg_dim,
        placeholder: colors().fg_dim,
        value: colors().fg,
        selection: colors().selection,
    }
}

fn filter_chip<'a>(
    label: &'a str,
    id: &'static str,
    selected: bool,
    value: Option<WallpaperType>,
) -> Element<'a, Message> {
    container(
        button(text(if selected { format!("✓ {label}") } else { label.to_string() }).size(14))
            .on_press(Message::TypeFilterSelected(value))
            .padding([8, 14])
            .style(move |_theme, status| {
                let background = if selected {
                    colors().accent_container
                } else if matches!(status, button::Status::Hovered) {
                    colors().hover
                } else {
                    Color::TRANSPARENT
                };
                button::Style {
                    background: Some(Background::Color(background)),
                    text_color: if selected {
                        colors().on_accent_container
                    } else {
                        colors().fg_dim
                    },
                    border: Border {
                        radius: 20.0.into(),
                        width: if selected { 0.0 } else { 1.0 },
                        color: colors().outline,
                    },
                    ..Default::default()
                }
            }),
    )
    .id(id)
    .into()
}

fn top_bar_button_style(_theme: &Theme, status: button::Status) -> button::Style {
    let background = if matches!(status, button::Status::Hovered) {
        colors().hover
    } else {
        Color::TRANSPARENT
    };
    button::Style {
        background: Some(Background::Color(background)),
        text_color: colors().fg,
        border: Border { radius: 20.0.into(), ..Default::default() },
        ..Default::default()
    }
}
