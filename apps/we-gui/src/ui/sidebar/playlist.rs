use iced::{
    widget::{button, column, container, pick_list, row, scrollable, text, text_input},
    Background, Border, Color, Element, Fill, Theme,
};
use we_core::playlist::PlaylistMode;

use crate::{
    app::{App, Message},
    domain::{
        i18n::{Localized, Text},
        playlist_editor::MoveDirection,
        runtime_status::RuntimeStatus,
    },
    ui::theme::scrollbar,
};

pub(crate) fn view(app: &App) -> Element<'_, Message> {
    let language = app.language;
    let feedback: Element<'_, Message> = match &app.runtime_status {
        RuntimeStatus::PlaylistError(_) | RuntimeStatus::ConfigSaveFailed(_) => container(
            text(language.runtime_status(&app.runtime_status))
                .size(12)
                .color(Color::from_rgb8(255, 180, 171)),
        )
        .padding(8)
        .style(entry_style)
        .into(),
        _ => text("").into(),
    };
    let mut playlist_list = column!().spacing(6);
    for name in app.launch_settings.playlists.definitions.keys() {
        let selected = app.playlist_selected.as_deref() == Some(name.as_str());
        playlist_list = playlist_list.push(
            button(text(if selected { format!("✓ {name}") } else { name.clone() }).size(14))
                .on_press(Message::PlaylistSelect(name.clone()))
                .width(Fill)
                .style(move |_theme, status| playlist_button_style(selected, status)),
        );
    }

    let create = row![
        text_input(language.text(Text::NewPlaylist), &app.playlist_new_name_input)
            .on_input(Message::PlaylistNewNameChanged)
            .on_submit(Message::PlaylistCreate)
            .padding([10, 10])
            .width(Fill),
        button(text(language.text(Text::CreatePlaylist)))
            .on_press(Message::PlaylistCreate)
            .style(super::detail::outlined_button_style),
    ]
    .spacing(8);

    let editor: Element<'_, Message> = match app.playlist_selected.as_deref().and_then(|name| {
        app.launch_settings.playlists.definitions.get(name).map(|playlist| (name, playlist))
    }) {
        Some((name, playlist)) => {
            let mode_options = vec![
                Localized::new(PlaylistMode::Sequential, language.text(Text::PlaylistSequential)),
                Localized::new(PlaylistMode::Repeat, language.text(Text::PlaylistRepeat)),
                Localized::new(PlaylistMode::Shuffle, language.text(Text::PlaylistShuffle)),
                Localized::new(PlaylistMode::Manual, language.text(Text::PlaylistManual)),
            ];
            let selected_mode =
                mode_options.iter().find(|option| option.value == playlist.mode).cloned();

            let mut entries = column!().spacing(8);
            if playlist.items.is_empty() {
                entries = entries.push(text(language.text(Text::PlaylistEmpty)).size(13));
            } else {
                for (index, item) in playlist.items.iter().enumerate() {
                    let title = app
                        .entries
                        .iter()
                        .find(|entry| entry.id == item.wallpaper_id)
                        .map(|entry| entry.title.as_str())
                        .unwrap_or(item.wallpaper_id.as_str());
                    let current = app.runtime_outputs.values().any(|runtime| {
                        runtime.playlist_active.as_deref() == Some(name)
                            && runtime.playlist_index == Some(index)
                    }) || (app.runtime_playlist_active.as_deref() == Some(name)
                        && app.runtime_playlist_index == Some(index));
                    let duration = app
                        .playlist_entry_duration_inputs
                        .get(index)
                        .map(String::as_str)
                        .unwrap_or("");

                    let mut up = button(text("↑"))
                        .style(super::detail::outlined_button_style)
                        .padding([6, 10]);
                    if index > 0 {
                        up = up.on_press(Message::PlaylistEntryMove {
                            index,
                            direction: MoveDirection::Up,
                        });
                    }
                    let mut down = button(text("↓"))
                        .style(super::detail::outlined_button_style)
                        .padding([6, 10]);
                    if index + 1 < playlist.items.len() {
                        down = down.on_press(Message::PlaylistEntryMove {
                            index,
                            direction: MoveDirection::Down,
                        });
                    }

                    entries = entries.push(
                        container(
                            column![
                                row![
                                    text(if current {
                                        format!("▶ {title}")
                                    } else {
                                        title.to_string()
                                    })
                                    .size(14)
                                    .width(Fill),
                                    up,
                                    down,
                                    button(text("×"))
                                        .on_press(Message::PlaylistEntryRemove(index))
                                        .style(super::detail::outlined_button_style)
                                        .padding([6, 10]),
                                ]
                                .spacing(6)
                                .align_y(iced::Alignment::Center),
                                row![
                                    text_input(language.text(Text::EntryDuration), duration)
                                        .on_input(move |value| {
                                            Message::PlaylistEntryDurationChanged { index, value }
                                        })
                                        .on_submit(Message::PlaylistEntryDurationApply(index))
                                        .padding([8, 8])
                                        .width(Fill),
                                    button(text(language.text(Text::Apply)).size(12))
                                        .on_press(Message::PlaylistEntryDurationApply(index))
                                        .style(super::detail::outlined_button_style),
                                    button(text(language.text(Text::UseDefaultDuration)).size(12))
                                        .on_press(Message::PlaylistEntryDurationClear(index))
                                        .style(super::detail::outlined_button_style),
                                ]
                                .spacing(6),
                            ]
                            .spacing(8),
                        )
                        .padding(10)
                        .style(entry_style),
                    );
                }
            }

            let output_statuses = app
                .runtime_outputs
                .iter()
                .filter_map(|(output, runtime)| {
                    (runtime.playlist_active.as_deref() == Some(name)).then(|| {
                        runtime
                            .playlist_index
                            .map(|index| format!("{output}: {}", index + 1))
                            .unwrap_or_else(|| output.clone())
                    })
                })
                .collect::<Vec<_>>();
            let active_status = if !output_statuses.is_empty() {
                text(format!(
                    "{} · {}",
                    language.text(Text::ActivePlaylist),
                    output_statuses.join(" · ")
                ))
                .size(12)
                .color(Color::from_rgb8(174, 198, 255))
            } else if app.runtime_playlist_active.as_deref() == Some(name) {
                let current = app
                    .runtime_playlist_index
                    .map(|index| {
                        format!("{}: {}", language.text(Text::CurrentPlaylistEntry), index + 1)
                    })
                    .unwrap_or_else(|| language.text(Text::ActivePlaylist).to_string());
                text(format!("{} · {current}", language.text(Text::ActivePlaylist)))
                    .size(12)
                    .color(Color::from_rgb8(174, 198, 255))
            } else {
                text("").size(12)
            };

            let mut output_targets = row!().spacing(6);
            for output in &app.outputs {
                let active = app.selected_outputs.contains(output);
                let output_name = output.clone();
                output_targets = output_targets.push(
                    button(text(if active { format!("✓ {output}") } else { output.clone() }))
                        .on_press(Message::ToggleOutput(output_name))
                        .style(move |_theme, status| playlist_button_style(active, status)),
                );
            }

            column![
                active_status,
                text(language.text(Text::PlaylistName)).size(13),
                row![
                    text_input(language.text(Text::PlaylistName), &app.playlist_name_input)
                        .on_input(Message::PlaylistNameChanged)
                        .on_submit(Message::PlaylistRename)
                        .padding([10, 10])
                        .width(Fill),
                    button(text(language.text(Text::RenamePlaylist)))
                        .on_press(Message::PlaylistRename)
                        .style(super::detail::outlined_button_style),
                    button(text(language.text(Text::DeletePlaylist)))
                        .on_press(Message::PlaylistDelete)
                        .style(super::detail::outlined_button_style),
                ]
                .spacing(6),
                text(language.text(Text::PlaylistMode)).size(13),
                pick_list(mode_options, selected_mode, |option| Message::PlaylistModeSelected(
                    option.value
                ))
                .padding([10, 10]),
                text(language.text(Text::PlaylistDefaultDuration)).size(13),
                row![
                    text_input("1800000", &app.playlist_default_duration_input)
                        .on_input(Message::PlaylistDefaultDurationChanged)
                        .on_submit(Message::PlaylistDefaultDurationApply)
                        .padding([10, 10])
                        .width(Fill),
                    button(text(language.text(Text::Apply)))
                        .on_press(Message::PlaylistDefaultDurationApply)
                        .style(super::detail::outlined_button_style),
                ]
                .spacing(6),
                text(language.text(Text::ApplyToDisplays)).size(13),
                output_targets,
                column![
                    row![
                        button(text(language.text(Text::PlayPlaylist)))
                            .on_press(Message::PlaylistPlay)
                            .style(super::detail::outlined_button_style),
                        button(text(language.text(Text::StopPlaylist)))
                            .on_press(Message::PlaylistStop)
                            .style(super::detail::outlined_button_style),
                    ]
                    .spacing(6),
                    row![
                        button(text(language.text(Text::PreviousItem)))
                            .on_press(Message::PlaylistPrevious)
                            .style(super::detail::outlined_button_style),
                        button(text(language.text(Text::NextItem)))
                            .on_press(Message::PlaylistNext)
                            .style(super::detail::outlined_button_style),
                    ]
                    .spacing(6),
                ]
                .spacing(6),
                text(format!(
                    "{} ({})",
                    language.text(Text::PlaylistEntries),
                    playlist.items.len()
                ))
                .size(16),
                entries,
            ]
            .spacing(10)
            .into()
        }
        None => text(language.text(Text::PlaylistEmpty)).size(13).into(),
    };

    let content = column![
        row![
            column![
                text(language.text(Text::Playlists)).size(26),
                text(language.text(Text::PlaylistsSubtitle)).size(12),
            ]
            .spacing(3)
            .width(Fill),
            button(text("×"))
                .on_press(Message::PlaylistsPressed)
                .style(super::detail::outlined_button_style),
        ]
        .align_y(iced::Alignment::Center),
        feedback,
        create,
        text(language.text(Text::Playlists)).size(15),
        playlist_list,
        editor,
    ]
    .spacing(12);

    container(
        scrollable(container(content).padding([4, 12]))
            .height(Fill)
            .direction(iced::widget::scrollable::Direction::Vertical(
                iced::widget::scrollable::Scrollbar::new().width(12).margin(6).scroller_width(6),
            ))
            .style(scrollbar::md_style),
    )
    .padding(18)
    .width(Fill)
    .height(Fill)
    .style(sidebar_style)
    .into()
}

fn playlist_button_style(selected: bool, status: button::Status) -> button::Style {
    let background = if selected {
        Color::from_rgb8(65, 83, 116)
    } else if matches!(status, button::Status::Hovered) {
        Color::from_rgb8(54, 56, 62)
    } else {
        Color::TRANSPARENT
    };
    button::Style {
        background: Some(Background::Color(background)),
        text_color: Color::from_rgb8(225, 228, 235),
        border: Border { radius: 10.0.into(), ..Default::default() },
        ..Default::default()
    }
}

fn entry_style(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(Color::from_rgb8(36, 37, 42))),
        border: Border { radius: 12.0.into(), width: 1.0, color: Color::from_rgb8(72, 74, 82) },
        ..Default::default()
    }
}

fn sidebar_style(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(Color::from_rgb8(29, 30, 34))),
        ..Default::default()
    }
}
