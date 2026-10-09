use std::collections::BTreeSet;

use iced::{
    widget::{button, checkbox, column, container, pick_list, row, scrollable, text, text_input},
    Background, Border, Color, Element, Fill, Theme,
};
use we_core::config::{RuntimeRuleAction, SceneDay, ScenePower};

use crate::{
    app::{App, Message},
    domain::{
        i18n::{Language, Localized, Text},
        playlist_editor::MoveDirection,
        runtime_status::RuntimeStatus,
    },
};

const DAYS: [SceneDay; 7] = [
    SceneDay::Mon,
    SceneDay::Tue,
    SceneDay::Wed,
    SceneDay::Thu,
    SceneDay::Fri,
    SceneDay::Sat,
    SceneDay::Sun,
];

fn power_options(language: Language) -> Vec<Localized<Option<ScenePower>>> {
    vec![
        Localized::new(None, language.text(Text::SceneAny)),
        Localized::new(Some(ScenePower::Ac), language.text(Text::SceneAc)),
        Localized::new(Some(ScenePower::Battery), language.text(Text::SceneBattery)),
    ]
}

fn known_outputs(app: &App) -> Vec<String> {
    let mut names = app.outputs.iter().cloned().collect::<BTreeSet<_>>();
    names.extend(app.launch_settings.outputs.keys().cloned());
    names.extend(app.scene_editor.preview_outputs.iter().cloned());
    for profile in app.launch_settings.profiles.definitions.values() {
        names.extend(profile.outputs.keys().cloned());
    }
    for rule in &app.scene_editor.rules.rules {
        names.extend(rule.outputs.iter().cloned());
    }
    names.into_iter().collect()
}

pub(crate) fn view(app: &App) -> Element<'_, Message> {
    let language = app.language;
    let editor = &app.scene_editor;
    let dirty = editor.is_dirty(&app.launch_settings);
    let save_button = button(text(language.text(Text::SceneSave)).size(13));
    let discard_button = button(text(language.text(Text::SceneDiscard)).size(13));
    let save_button = if dirty { save_button.on_press(Message::SceneSave) } else { save_button };
    let discard_button =
        if dirty { discard_button.on_press(Message::SceneDiscard) } else { discard_button };

    let header = column![
        row![
            column![
                text(language.text(Text::SmartScenes)).size(25),
                text(language.text(Text::ScenesSubtitle)).size(12),
            ]
            .spacing(4)
            .width(Fill),
            button(text("×").size(20)).on_press(Message::ScenesPressed),
        ]
        .align_y(iced::Alignment::Center),
        row![save_button, discard_button].spacing(8),
        checkbox(editor.rules.enabled)
            .label(language.text(Text::SceneEnabled))
            .on_toggle(Message::SceneEnabledToggled),
    ]
    .spacing(10);

    let mut rules = column![text(language.text(Text::ScenePriority)).size(12),].spacing(7);
    let profiles = app.launch_settings.profiles.definitions.keys().cloned().collect::<Vec<_>>();
    if profiles.is_empty() {
        rules = rules.push(text(language.text(Text::SceneNoProfiles)).size(13));
        rules = rules.push(
            button(text(language.text(Text::CreateProfile)).size(13))
                .on_press(Message::ProfilesPressed),
        );
    } else {
        rules = rules.push(
            button(text(format!("+  {}", language.text(Text::SceneAddRule))))
                .on_press(Message::SceneAdd),
        );
    }

    for (index, rule) in editor.rules.rules.iter().enumerate() {
        let selected = editor.selected == Some(index);
        let title =
            format!("{:02}   {}{}", index + 1, rule.profile, if selected { "  ◀" } else { "" });
        rules = rules.push(
            button(text(title).size(13))
                .on_press(Message::SceneSelect(index))
                .width(Fill)
                .style(if selected { button::primary } else { button::secondary }),
        );
    }
    if editor.rules.rules.is_empty() {
        rules = rules.push(text(language.text(Text::SceneNoRules)).size(13));
    }

    let mut content =
        column![header, divider(), current_status(app), divider(), rules,].spacing(14);

    if let Some(index) = editor.selected {
        if let Some(rule) = editor.rules.rules.get(index) {
            let options = profiles;
            let selected_profile = options.iter().find(|name| *name == &rule.profile).cloned();
            let mut days = column![text(language.text(Text::SceneDays)).size(12)].spacing(6);
            for chunk in DAYS.chunks(4) {
                let mut group = row!().spacing(8);
                for &day in chunk {
                    let active = rule.days.contains(&day);
                    group = group.push(
                        checkbox(active)
                            .label(language.scene_day(day))
                            .on_toggle(move |value| Message::SceneDayToggled(day, value)),
                    );
                }
                days = days.push(group);
            }
            if rule.days.is_empty() {
                days = days.push(text(language.text(Text::SceneEveryDay)).size(12));
            }

            let outputs = known_outputs(app);
            let mut display_conditions =
                column![text(language.text(Text::SceneDisplays)).size(13)].spacing(7);
            if outputs.is_empty() {
                display_conditions =
                    display_conditions.push(text(language.text(Text::SceneAnyDisplay)).size(12));
            }
            for output in outputs {
                let checked = rule.outputs.contains(&output);
                display_conditions =
                    display_conditions.push(checkbox(checked).label(output.clone()).on_toggle(
                        move |value| Message::SceneOutputToggled(output.clone(), value),
                    ));
            }
            let output_name = text_input("DP-1", &editor.output_name_input)
                .on_input(Message::SceneOutputNameChanged)
                .on_submit(Message::SceneAddOutput)
                .width(Fill);
            display_conditions = display_conditions.push(
                row![
                    output_name,
                    button(text(language.text(Text::SceneAddDisplay)).size(12))
                        .on_press(Message::SceneAddOutput),
                ]
                .spacing(8),
            );

            let powers = power_options(language);
            let selected_power = powers.iter().find(|option| option.value == rule.power).cloned();
            let mut editor_panel = column![
                row![
                    text(language.text(Text::SceneProfile)).size(15).width(Fill),
                    button(text(language.text(Text::SceneMoveUp)).size(12))
                        .on_press(Message::SceneMove(MoveDirection::Up)),
                    button(text(language.text(Text::SceneMoveDown)).size(12))
                        .on_press(Message::SceneMove(MoveDirection::Down)),
                ]
                .spacing(5)
                .align_y(iced::Alignment::Center),
                pick_list(options, selected_profile, Message::SceneProfileSelected).width(Fill),
                text(language.text(Text::SceneTime)).size(15),
                row![
                    column![
                        text(language.text(Text::SceneStart)).size(12),
                        text_input("09:00", rule.start.as_deref().unwrap_or(""))
                            .on_input(Message::SceneStartChanged)
                            .width(Fill),
                    ]
                    .spacing(3),
                    column![
                        text(language.text(Text::SceneEnd)).size(12),
                        text_input("18:00", rule.end.as_deref().unwrap_or(""))
                            .on_input(Message::SceneEndChanged)
                            .width(Fill),
                    ]
                    .spacing(3),
                ]
                .spacing(9),
                button(text(language.text(Text::SceneAllDay)).size(12))
                    .on_press(Message::SceneAllDay),
                days,
                text(language.text(Text::ScenePower)).size(13),
                pick_list(powers, selected_power, |option| Message::ScenePowerSelected(
                    option.value
                ))
                .width(Fill),
                display_conditions,
                row![
                    button(text(language.text(Text::SceneDuplicate)).size(13))
                        .on_press(Message::SceneDuplicate),
                    button(text(language.text(Text::SceneDelete)).size(13))
                        .on_press(Message::SceneDelete)
                        .style(button::danger),
                ]
                .spacing(8),
            ]
            .spacing(9);
            if let Err(error) = editor.rules.validate() {
                editor_panel =
                    editor_panel.push(text(error).size(12).color(Color::from_rgb8(255, 171, 164)));
            }
            content = content.push(container(editor_panel).padding(14).style(panel_style));
        }
    }

    let actions = vec![
        Localized::new(RuntimeRuleAction::Keep, language.text(Text::RuleKeep)),
        Localized::new(RuntimeRuleAction::Mute, language.text(Text::RuleMute)),
        Localized::new(RuntimeRuleAction::Pause, language.text(Text::RulePause)),
    ];
    let selected_action =
        actions.iter().find(|option| option.value == editor.adaptive.on_battery).cloned();
    content = content.push(
        container(
            column![
                text(language.text(Text::SceneBattery)).size(17),
                text(language.text(Text::SceneBatteryFps)).size(13),
                text_input("24", &editor.battery_fps)
                    .on_input(Message::SceneBatteryFpsChanged)
                    .width(Fill),
                text(language.text(Text::SceneBatteryAction)).size(13),
                pick_list(actions, selected_action, |option| Message::SceneBatteryActionSelected(
                    option.value
                ))
                .width(Fill),
            ]
            .spacing(9),
        )
        .padding(14)
        .style(panel_style),
    );

    content = content.push(divider()).push(preview(app));
    if dirty {
        content = content.push(
            text(language.text(Text::SceneUnsaved)).size(13).color(Color::from_rgb8(255, 210, 137)),
        );
    }
    if let Some(error) = &editor.error {
        content = content.push(text(error.clone()).size(13).color(Color::from_rgb8(255, 171, 164)));
    }
    if let Some(notice) = &editor.notice {
        content =
            content.push(text(notice.clone()).size(13).color(Color::from_rgb8(146, 214, 180)));
    }

    container(scrollable(content.spacing(14)).height(Fill))
        .width(Fill)
        .height(Fill)
        .padding(18)
        .into()
}

fn preview(app: &App) -> Element<'_, Message> {
    let editor = &app.scene_editor;
    let language = app.language;
    let days =
        DAYS.iter().map(|&day| Localized::new(day, language.scene_day(day))).collect::<Vec<_>>();
    let selected_day = days.iter().find(|day| day.value == editor.preview_day).cloned();
    let powers = power_options(language);
    let selected_power = powers.iter().find(|p| p.value == editor.preview_power).cloned();
    let matching = match editor.preview() {
        Some((index, profile)) => {
            format!("{} #{:02} · {profile}", language.text(Text::SceneMatch), index + 1)
        }
        None if we_core::config::parse_clock_time(&editor.preview_time).is_none() => {
            language.text(Text::SceneInvalidTime).to_string()
        }
        None => language.text(Text::SceneNoMatch).to_string(),
    };
    let why = editor
        .preview()
        .and_then(|(index, _)| editor.rules.rules.get(index))
        .map(|rule| {
            let time = match (&rule.start, &rule.end) {
                (Some(start), Some(end)) => format!("{start}–{end}"),
                _ => language.text(Text::SceneAllDay).to_string(),
            };
            let weekdays = if rule.days.is_empty() {
                language.text(Text::SceneEveryDay).to_string()
            } else {
                rule.days.iter().map(|day| language.scene_day(*day)).collect::<Vec<_>>().join(" · ")
            };
            let power = match rule.power {
                None => language.text(Text::SceneAny),
                Some(ScenePower::Ac) => language.text(Text::SceneAc),
                Some(ScenePower::Battery) => language.text(Text::SceneBattery),
            };
            let displays = if rule.outputs.is_empty() {
                language.text(Text::SceneAnyDisplay).to_string()
            } else {
                rule.outputs.join(", ")
            };
            format!("{time} · {weekdays} · {power} · {displays}")
        })
        .unwrap_or_default();
    let forecast = match editor.next_transition() {
        Some((offset, minute, index)) => {
            let day = DAYS[(editor.preview_day as usize + offset as usize) % DAYS.len()];
            let day_label =
                if offset == 0 { language.text(Text::SceneToday) } else { language.scene_day(day) };
            let target = index
                .and_then(|index| editor.rules.rules.get(index))
                .map(|rule| rule.profile.as_str())
                .unwrap_or(language.text(Text::SceneNoMatch));
            format!(
                "{}: {day_label} {:02}:{:02} → {target}",
                language.text(Text::SceneNextSwitch),
                minute / 60,
                minute % 60
            )
        }
        None => language.text(Text::SceneNoUpcomingSwitch).to_string(),
    };
    let mut outputs = column![text(language.text(Text::SceneDisplays)).size(12)].spacing(6);
    for output in known_outputs(app) {
        let checked = editor.preview_outputs.contains(&output);
        outputs = outputs.push(
            checkbox(checked)
                .label(output.clone())
                .on_toggle(move |value| Message::ScenePreviewOutputToggled(output.clone(), value)),
        );
    }
    container(
        column![
            text(language.text(Text::ScenePreview)).size(17),
            text(language.text(Text::ScenePreviewHint)).size(12),
            row![
                text_input("HH:MM", &editor.preview_time)
                    .on_input(Message::ScenePreviewTimeChanged)
                    .width(Fill),
                pick_list(days, selected_day, |day| Message::ScenePreviewDaySelected(day.value))
                    .width(Fill),
            ]
            .spacing(8),
            pick_list(powers, selected_power, |power| Message::ScenePreviewPowerSelected(
                power.value
            ))
            .width(Fill),
            outputs,
            text(matching).size(13),
            text(why).size(11),
            text(forecast).size(12),
        ]
        .spacing(9),
    )
    .padding(14)
    .style(panel_style)
    .into()
}

fn current_status(app: &App) -> Element<'_, Message> {
    let language = app.language;
    let mut lines = column![text(language.text(Text::SceneCurrent)).size(17)].spacing(6);
    if let RuntimeStatus::Raw(raw) = &app.runtime_status {
        if let Ok(value) = toml::from_str::<toml::Value>(raw) {
            if let Some(status) = value.get("scene_runtime") {
                if let Some(profile) = status
                    .get("matched_profile")
                    .and_then(toml::Value::as_str)
                    .filter(|name| !name.is_empty())
                {
                    let label = if let Some(index) =
                        status.get("matched_rule").and_then(toml::Value::as_integer)
                    {
                        format!("{}: #{index:02} · {profile}", language.text(Text::SceneMatch))
                    } else {
                        format!("{}: {profile}", language.text(Text::SceneMatch))
                    };
                    lines = lines.push(text(label).size(12));
                }
                for (key, name) in [
                    ("active_profile", language.text(Text::SceneProfile)),
                    ("last_error", language.text(Text::SceneError)),
                ] {
                    if let Some(value) =
                        status.get(key).and_then(toml::Value::as_str).filter(|v| !v.is_empty())
                    {
                        lines = lines.push(text(format!("{name}: {value}")).size(12));
                    }
                }
                if status.get("manual_override").and_then(toml::Value::as_bool) == Some(true) {
                    lines = lines.push(text(language.text(Text::SceneManualOverride)).size(12));
                }
                if let Some(power) = value
                    .get("integration_runtime")
                    .and_then(|v| v.get("power_state"))
                    .and_then(toml::Value::as_str)
                {
                    let label = match power {
                        "ac" => language.text(Text::SceneAc),
                        "battery" => language.text(Text::SceneBattery),
                        _ => language.text(Text::ScenePowerUnknown),
                    };
                    lines = lines.push(
                        text(format!("{}: {label}", language.text(Text::ScenePower))).size(12),
                    );
                }
                if let Some(cap) = value
                    .get("integration_runtime")
                    .and_then(|v| v.get("adaptive_fps_cap"))
                    .and_then(toml::Value::as_integer)
                {
                    lines = lines.push(
                        text(format!("{}: {cap} FPS", language.text(Text::SceneAppliedBatteryFps)))
                            .size(12),
                    );
                }
                return container(lines).padding(14).style(panel_style).into();
            }
        }
    }
    container(lines.push(text(language.text(Text::SceneStatusUnavailable)).size(12)))
        .padding(14)
        .style(panel_style)
        .into()
}

fn divider() -> Element<'static, Message> {
    container(text(""))
        .width(Fill)
        .height(1)
        .style(|theme: &Theme| container::Style {
            background: Some(Background::Color(if matches!(theme, Theme::Light) {
                Color::from_rgb8(213, 215, 220)
            } else {
                Color::from_rgb8(67, 70, 76)
            })),
            ..Default::default()
        })
        .into()
}

fn panel_style(theme: &Theme) -> container::Style {
    let light = matches!(theme, Theme::Light);
    container::Style {
        background: Some(Background::Color(if light {
            Color::from_rgb8(244, 246, 250)
        } else {
            Color::from_rgb8(38, 41, 47)
        })),
        border: Border {
            radius: 12.0.into(),
            width: 1.0,
            color: if light {
                Color::from_rgb8(224, 226, 231)
            } else {
                Color::from_rgb8(57, 62, 70)
            },
        },
        ..Default::default()
    }
}
