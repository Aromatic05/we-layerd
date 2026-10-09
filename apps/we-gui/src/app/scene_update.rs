use iced::Task;
use we_core::config::parse_clock_time;

use crate::{
    domain::{i18n::Text, ui_state::Sidebar},
    services::{config, runtime},
};

use super::{App, Message};

pub(crate) fn update(app: &mut App, message: Message) -> Task<Message> {
    // Any rule edit invalidates the old saved or failed status; simulation-only controls do not.
    if matches!(
        &message,
        Message::SceneAdd
            | Message::SceneDelete
            | Message::SceneMove(_)
            | Message::SceneProfileSelected(_)
            | Message::SceneStartChanged(_)
            | Message::SceneEndChanged(_)
            | Message::SceneAllDay
            | Message::SceneDayToggled(_, _)
            | Message::ScenePowerSelected(_)
            | Message::SceneOutputToggled(_, _)
            | Message::SceneAddOutput
            | Message::SceneBatteryFpsChanged(_)
            | Message::SceneBatteryActionSelected(_)
    ) {
        app.scene_editor.notice = None;
        app.scene_editor.error = None;
    }
    match message {
        Message::ScenesPressed => {
            app.sidebar =
                if app.sidebar == Some(Sidebar::Scenes) { None } else { Some(Sidebar::Scenes) };
            if app.sidebar == Some(Sidebar::Scenes) {
                if app.scene_editor.preview_outputs.is_empty() {
                    app.scene_editor.preview_outputs.extend(app.outputs.iter().cloned());
                }
                return Task::batch(vec![
                    Task::perform(runtime::fetch_status(), Message::StatusLoaded),
                    Task::perform(runtime::fetch_outputs(), Message::OutputsLoaded),
                ]);
            }
        }
        Message::SceneSelect(index) => {
            if index < app.scene_editor.rules.rules.len() {
                app.scene_editor.selected = Some(index);
                app.scene_editor.error = None;
            }
        }
        Message::SceneAdd => {
            if let Some(profile) = app.launch_settings.profiles.definitions.keys().next().cloned() {
                app.scene_editor.add(profile);
            } else {
                app.scene_editor.error = Some(app.language.text(Text::SceneNoProfiles).into());
            }
        }
        Message::SceneDelete => app.scene_editor.remove_selected(),
        Message::SceneMove(direction) => app.scene_editor.move_selected(direction),
        Message::SceneProfileSelected(profile) => {
            if app.launch_settings.profiles.definitions.contains_key(&profile) {
                if let Some(rule) = app.scene_editor.selected_rule_mut() {
                    rule.profile = profile;
                }
            }
        }
        Message::SceneStartChanged(time) => {
            if let Some(rule) = app.scene_editor.selected_rule_mut() {
                rule.start = (!time.is_empty()).then_some(time);
            }
        }
        Message::SceneEndChanged(time) => {
            if let Some(rule) = app.scene_editor.selected_rule_mut() {
                rule.end = (!time.is_empty()).then_some(time);
            }
        }
        Message::SceneAllDay => {
            if let Some(rule) = app.scene_editor.selected_rule_mut() {
                rule.start = None;
                rule.end = None;
            }
        }
        Message::SceneDayToggled(day, enabled) => {
            if let Some(rule) = app.scene_editor.selected_rule_mut() {
                rule.days.retain(|item| *item != day);
                if enabled {
                    rule.days.push(day);
                    rule.days.sort_by_key(|day| *day as u8);
                }
            }
        }
        Message::ScenePowerSelected(power) => {
            if let Some(rule) = app.scene_editor.selected_rule_mut() {
                rule.power = power;
            }
        }
        Message::SceneOutputToggled(name, enabled) => {
            if let Some(rule) = app.scene_editor.selected_rule_mut() {
                rule.outputs.retain(|output| output != &name);
                if enabled {
                    rule.outputs.push(name);
                    rule.outputs.sort();
                }
            }
        }
        Message::SceneOutputNameChanged(value) => app.scene_editor.output_name_input = value,
        Message::SceneAddOutput => {
            if !app.scene_editor.add_output_name() {
                app.scene_editor.error = Some(app.language.text(Text::SceneInvalidDisplay).into());
            }
        }
        Message::SceneBatteryFpsChanged(value) => app.scene_editor.battery_fps = value,
        Message::SceneBatteryActionSelected(action) => {
            app.scene_editor.adaptive.on_battery = action
        }
        Message::SceneDiscard => app.scene_editor.reset(&app.launch_settings),
        Message::SceneSave => {
            let adaptive = match app.scene_editor.validate(&app.launch_settings.profiles) {
                Ok(adaptive) => adaptive,
                Err(error) => {
                    app.scene_editor.error = Some(error);
                    return Task::none();
                }
            };
            if let Err(error) =
                config::persist_scene_settings(&app.config_path, &app.scene_editor.rules, &adaptive)
            {
                app.scene_editor.error = Some(error);
                return Task::none();
            }
            app.launch_settings.scenes = app.scene_editor.rules.clone();
            app.launch_settings.adaptive = adaptive;
            app.scene_editor.reset(&app.launch_settings);
            // Do not force-restart the user's desktop when the daemon cannot accept a reload.
            let reloaded =
                !runtime::daemon_is_running() || runtime::reload_scene_settings(&app.config_path);
            app.scene_editor.notice = Some(if reloaded {
                app.language.text(Text::SceneSaved).to_string()
            } else {
                app.language.text(Text::SceneReloadFailed).to_string()
            });
            return Task::perform(runtime::fetch_status(), Message::StatusLoaded);
        }
        Message::ScenePreviewTimeChanged(value) => {
            app.scene_editor.preview_time = value;
            if parse_clock_time(&app.scene_editor.preview_time).is_some() {
                app.scene_editor.error = None;
            }
        }
        Message::ScenePreviewDaySelected(value) => app.scene_editor.preview_day = value,
        Message::ScenePreviewPowerSelected(value) => app.scene_editor.preview_power = value,
        Message::ScenePreviewOutputToggled(name, enabled) => {
            if enabled {
                app.scene_editor.preview_outputs.insert(name);
            } else {
                app.scene_editor.preview_outputs.remove(&name);
            }
        }
        _ => unreachable!("only scene editing messages enter scene_update"),
    }
    Task::none()
}
