use iced::{
    alignment::Vertical,
    widget::{button, column, container, image, responsive, row, scrollable, stack, text, text_input},
    Background, Border, Color, ContentFit, Element, Fill, Theme,
};
use we_core::wallpaper::{WallpaperEntry, WallpaperType};

use crate::{
    app::{App, Message},
    domain::ui_state::AnimatedPreview,
};

pub(crate) fn view(app: &App) -> Element<'_, Message> {
    let matches = app.entries.iter().enumerate().filter(|(_, entry)| {
        app.type_filter.is_none_or(|ty| entry.ty == ty)
            && entry.title.to_lowercase().contains(&app.search_query.to_lowercase())
    });
    let entries = matches.collect::<Vec<_>>();
    let grid = responsive(move |size| {
        build_wallpaper_grid(
            entries.iter().copied(),
            app.selected_id.as_ref(),
            size.width,
            &app.animated_previews,
        )
    });
    let filters = row![
        filter_chip("All", app.type_filter.is_none(), None),
        filter_chip("Web", app.type_filter == Some(WallpaperType::Web), Some(WallpaperType::Web)),
        filter_chip("Scene", app.type_filter == Some(WallpaperType::Scene), Some(WallpaperType::Scene)),
        filter_chip("Video", app.type_filter == Some(WallpaperType::Video), Some(WallpaperType::Video)),
    ]
    .spacing(8);
    let toolbar = row![
        column![text("Wallpapers").size(28), text(format!("{} items", app.entries.len())).size(13)]
            .spacing(2)
            .width(Fill),
        button(text("⚙").size(20))
            .on_press(Message::SettingsPressed)
            .style(top_bar_button_style),
    ]
    .align_y(Vertical::Center);
    container(
        column![
            toolbar,
            row![
                text_input("Search wallpapers", &app.search_query)
                    .on_input(Message::SearchChanged)
                    .padding(12)
                    .style(search_style)
                    .width(Fill),
                filters,
            ]
            .spacing(12)
            .align_y(Vertical::Center),
            scrollable(grid).width(Fill).height(Fill),
        ]
        .spacing(16),
    )
    .padding(24)
    .width(Fill)
    .height(Fill)
    .style(library_style)
    .into()
}

fn build_wallpaper_grid<'a>(
    entries: impl Iterator<Item = (usize, &'a WallpaperEntry)>,
    selected_id: Option<&String>,
    width: f32,
    animated_previews: &'a std::collections::HashMap<std::path::PathBuf, AnimatedPreview>,
) -> Element<'a, Message> {
    let spacing = 12.0;
    let target_card_width = 360.0;
    let cols = ((width + spacing) / (target_card_width + spacing)).floor().max(1.0) as usize;
    let card_width = ((width - spacing * (cols.saturating_sub(1) as f32)) / cols as f32).max(180.0);
    let mut root = column!().spacing(spacing).padding(spacing);
    let entries = entries.collect::<Vec<_>>();

    for chunk in entries.chunks(cols) {
        let mut row = row!().spacing(spacing);
        for (index, entry) in chunk.iter() {
            let selected = selected_id.is_some_and(|id| id == &entry.id);
            row = row.push(make_wallpaper_card(entry, *index, card_width, selected, animated_previews));
        }
        root = root.push(row);
    }
    root.into()
}

fn make_wallpaper_card<'a>(
    entry: &'a WallpaperEntry,
    index: usize,
    card_width: f32,
    selected: bool,
    animated_previews: &'a std::collections::HashMap<std::path::PathBuf, AnimatedPreview>,
) -> Element<'a, Message> {
    let card_height = (card_width * 9.0 / 16.0).round();
    let media: Element<'a, Message> = if let Some(path) = &entry.preview {
        let handle = animated_previews
            .get(path)
            .map(|preview| {
                let frame = &preview.frames[preview.current];
                image::Handle::from_rgba(frame.width, frame.height, frame.pixels.clone())
            })
            .unwrap_or_else(|| image::Handle::from_path(path));
        image(handle)
            .width(card_width)
            .height(card_height)
            .content_fit(ContentFit::Cover)
            .into()
    } else {
        container(text(""))
            .width(card_width)
            .height(card_height)
            .style(|_theme: &Theme| container::Style {
                background: Some(Background::Color(Color::from_rgb8(18, 18, 18))),
                ..Default::default()
            })
            .into()
    };

    let chip = container(text(wallpaper_type_name(entry.ty)).size(12))
        .padding([3, 8])
        .style(|_theme: &Theme| container::Style {
            text_color: Some(Color::WHITE),
            background: Some(Background::Color(Color { r: 0.0, g: 0.0, b: 0.0, a: 0.45 })),
            border: Border { radius: 10.0.into(), ..Default::default() },
            ..Default::default()
        });
    let overlay = container(chip)
        .width(Fill)
        .height(Fill)
        .align_x(iced::alignment::Horizontal::Right)
        .align_y(Vertical::Bottom)
        .padding(8);
    let border_color = if selected { Color::from_rgb8(174, 198, 255) } else { Color::from_rgb8(70, 72, 78) };
    let frame = container(stack![media, overlay])
        .width(card_width)
        .height(card_height)
        .style(move |_theme: &Theme| container::Style {
            border: Border { radius: 16.0.into(), width: if selected { 2.0 } else { 1.0 }, color: border_color },
            shadow: if selected {
                iced::Shadow { color: Color::from_rgba8(0, 0, 0, 0.35), blur_radius: 8.0, offset: iced::Vector::new(0.0, 2.0) }
            } else {
                iced::Shadow::default()
            },
            ..Default::default()
        });
    button(frame)
        .on_press(Message::SelectWallpaper(index))
        .style(image_card_button_style)
        .into()
}

fn wallpaper_type_name(ty: WallpaperType) -> &'static str {
    match ty {
        WallpaperType::Video => "video",
        WallpaperType::Scene => "scene",
        WallpaperType::Web => "web",
        WallpaperType::Unknown => "unknown",
    }
}

fn image_card_button_style(_theme: &Theme, _status: button::Status) -> button::Style {
    button::Style { background: None, text_color: Color::WHITE, border: Border::default(), shadow: iced::Shadow::default(), ..Default::default() }
}

fn library_style(_theme: &Theme) -> container::Style {
    container::Style { background: Some(Background::Color(Color::from_rgb8(24, 25, 28))), ..Default::default() }
}

fn search_style(_theme: &Theme, status: text_input::Status) -> text_input::Style {
    let border = if matches!(status, text_input::Status::Focused { .. }) { Color::from_rgb8(174, 198, 255) } else { Color::from_rgb8(140, 144, 153) };
    text_input::Style {
        background: Background::Color(Color::from_rgb8(43, 44, 48)),
        border: Border { radius: 28.0.into(), width: 1.0, color: border },
        icon: Color::from_rgb8(196, 199, 204),
        placeholder: Color::from_rgb8(196, 199, 204),
        value: Color::from_rgb8(230, 225, 229),
        selection: Color::from_rgb8(78, 99, 139),
    }
}

fn filter_chip<'a>(label: &'a str, selected: bool, value: Option<WallpaperType>) -> iced::widget::Button<'a, Message> {
    button(text(if selected { format!("✓ {label}") } else { label.to_string() }).size(14))
        .on_press(Message::TypeFilterSelected(value))
        .padding([8, 14])
        .style(move |_theme, status| {
            let background = if selected { Color::from_rgb8(70, 91, 129) } else if matches!(status, button::Status::Hovered) { Color::from_rgb8(54, 56, 62) } else { Color::TRANSPARENT };
            button::Style {
                background: Some(Background::Color(background)),
                text_color: if selected { Color::from_rgb8(222, 231, 255) } else { Color::from_rgb8(201, 203, 209) },
                border: Border { radius: 20.0.into(), width: if selected { 0.0 } else { 1.0 }, color: Color::from_rgb8(143, 147, 156) },
                ..Default::default()
            }
        })
}

fn top_bar_button_style(_theme: &Theme, status: button::Status) -> button::Style {
    let background = if matches!(status, button::Status::Hovered) { Color::from_rgb8(56, 58, 63) } else { Color::TRANSPARENT };
    button::Style { background: Some(Background::Color(background)), text_color: Color::from_rgb8(220, 225, 235), border: Border { radius: 20.0.into(), ..Default::default() }, ..Default::default() }
}
