use iced::{
    widget::{button, column, container, row, scrollable, text, text_input},
    Element, Fill,
};

use crate::{
    app::{App, Message},
    domain::i18n::Text,
};

pub(crate) fn view(app: &App) -> Element<'_, Message> {
    let language = app.language;
    let profile_names =
        app.launch_settings.profiles.definitions.keys().cloned().collect::<Vec<_>>();

    let mut profiles = column![
        text(language.text(Text::Profiles)).size(24),
        text(language.text(Text::ProfilesSubtitle)).size(13),
        row![
            text_input(language.text(Text::NewProfile), &app.profile_new_name_input)
                .on_input(Message::ProfileNewNameChanged)
                .width(Fill),
            button(text(language.text(Text::CreateProfile))).on_press(Message::ProfileCreate),
        ]
        .spacing(8),
    ]
    .spacing(10);

    for name in profile_names {
        let active = app.launch_settings.profiles.active.as_deref() == Some(name.as_str());
        let selected = app.profile_selected.as_deref() == Some(name.as_str());
        let label = match (active, selected) {
            (true, _) => format!("● {name}"),
            (false, true) => format!("› {name}"),
            _ => name.clone(),
        };
        profiles =
            profiles.push(button(text(label)).on_press(Message::ProfileSelect(name)).width(Fill));
    }

    let detail: Element<'_, Message> = match app.profile_selected.as_deref() {
        Some(name) => {
            let profile = app.launch_settings.profiles.definitions.get(name);
            let mut outputs =
                column![text(language.text(Text::ProfileOutputs)).size(14)].spacing(6);
            if let Some(profile) = profile {
                if profile.outputs.is_empty() {
                    outputs = outputs.push(text(language.text(Text::ProfileEmpty)).size(13));
                } else {
                    for (output, binding) in &profile.outputs {
                        let target = if let Some(playlist) = binding.playlist.as_deref() {
                            format!("playlist · {playlist}")
                        } else if let Some(wallpaper) = binding.wallpaper_id.as_deref() {
                            format!("wallpaper · {wallpaper}")
                        } else {
                            "invalid binding".to_string()
                        };
                        outputs = outputs.push(text(format!("{output}  →  {target}")).size(13));
                    }
                }
            }

            column![
                text_input(language.text(Text::ProfileName), &app.profile_name_input)
                    .on_input(Message::ProfileNameChanged),
                row![
                    button(text(language.text(Text::RenameProfile)))
                        .on_press(Message::ProfileRename),
                    button(text(language.text(Text::DeleteProfile)))
                        .on_press(Message::ProfileDelete),
                ]
                .spacing(8),
                outputs,
                row![
                    button(text(language.text(Text::SaveCurrentProfile)))
                        .on_press(Message::ProfileSaveCurrent),
                    button(text(language.text(Text::ApplyProfile))).on_press(Message::ProfileApply),
                ]
                .spacing(8),
            ]
            .spacing(10)
            .into()
        }
        None => column![text(language.text(Text::ProfileEmpty)).size(13)].into(),
    };

    container(scrollable(column![profiles, detail].spacing(16)))
        .width(Fill)
        .height(Fill)
        .padding(18)
        .into()
}
