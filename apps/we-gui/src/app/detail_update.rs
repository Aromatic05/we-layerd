use iced::Task;
use we_core::wallpaper::settings::{RenderResolution, WallpaperSettings};

use crate::{services::{config, runtime}, ui::sidebar::detail as wallpaper_detail};

use super::{App, Message};

pub(crate) fn update(
    app: &mut App,
    message: wallpaper_detail::DetailMessage,
) -> Task<Message> {
    use wallpaper_detail::{DetailMessage, ResolutionMode};

    if matches!(message, DetailMessage::Apply) {
        persist_current_config(app);
        return super::update::update(app, Message::PlayPressed);
    }
    match message {
        DetailMessage::SelectTab(tab) => {
            app.detail_tab = tab;
            return Task::none();
        }
        DetailMessage::TogglePlayback => {
            if !app.playback_running {
                return super::update::update(app, Message::PlayPressed);
            }
            let action = if app.playback_paused { "resume" } else { "pause" };
            if runtime::send_control(action) {
                app.playback_paused = !app.playback_paused;
            }
            return Task::none();
        }
        DetailMessage::Stop => return super::update::update(app, Message::StopPressed),
        DetailMessage::ToggleOutput(output) => return super::update::update(app, Message::ToggleOutput(output)),
        _ => {}
    }
    if let DetailMessage::PickPath { key, directory } = message {
        return Task::perform(
            async move {
                let dialog = rfd::FileDialog::new().set_title("Select wallpaper property path");
                let path = if directory { dialog.pick_folder() } else { dialog.pick_file() };
                DetailMessage::PathPicked { key, path: path.map(|path| path.display().to_string()) }
            },
            Message::Detail,
        );
    }

    let Some(selected_id) = app.selected_id.clone() else {
        return Task::none();
    };
    let profile = app.launch_settings.wallpapers.entry(selected_id).or_default();
    match message {
        DetailMessage::Apply | DetailMessage::TogglePlayback | DetailMessage::Stop | DetailMessage::ToggleOutput(_) | DetailMessage::SelectTab(_) => unreachable!("detail action handled before profile mutation"),
        DetailMessage::FpsChanged(value) => {
            if let Ok(fps) = value.parse::<u32>() {
                profile.fps = fps.clamp(1, 360);
            }
        }
        DetailMessage::SpeedChanged(value) => profile.speed = value,
        DetailMessage::VolumeChanged(value) => profile.volume = value,
        DetailMessage::MutedChanged(value) => profile.muted = value,
        DetailMessage::ResolutionModeChanged(ResolutionMode::Automatic) => {
            profile.render_resolution = RenderResolution::Automatic;
            app.resolution_width.clear();
            app.resolution_height.clear();
        }
        DetailMessage::ResolutionModeChanged(ResolutionMode::Fixed) => {
            let width = app.resolution_width.parse().unwrap_or(1920).max(1);
            let height = app.resolution_height.parse().unwrap_or(1080).max(1);
            profile.render_resolution = RenderResolution::Fixed { width, height };
            app.resolution_width = width.to_string();
            app.resolution_height = height.to_string();
        }
        DetailMessage::ResolutionWidthChanged(value) => {
            app.resolution_width = value;
            sync_fixed_resolution(profile, &app.resolution_width, &app.resolution_height);
        }
        DetailMessage::ResolutionHeightChanged(value) => {
            app.resolution_height = value;
            sync_fixed_resolution(profile, &app.resolution_width, &app.resolution_height);
        }
        DetailMessage::FillModeChanged(value) => profile.fill_mode = value,
        DetailMessage::RotationChanged(value) => profile.rotation_degrees = value,
        DetailMessage::PropertyChanged { key, value } => {
            profile.user_properties.insert(key, value);
        }
        DetailMessage::PathPicked { key, path } => {
            if let Some(path) = path {
                profile.user_properties.insert(key, serde_json::Value::String(path));
            }
        }
        DetailMessage::PickPath { .. } => unreachable!("path picker handled before profile mutation"),
        DetailMessage::ResetProperties => profile.user_properties.clear(),
    }
    persist_current_config(app);
    Task::none()
}

fn sync_fixed_resolution(profile: &mut WallpaperSettings, width: &str, height: &str) {
    let (Ok(width), Ok(height)) = (width.parse::<u32>(), height.parse::<u32>()) else {
        return;
    };
    profile.render_resolution = RenderResolution::Fixed { width: width.max(1), height: height.max(1) };
}

pub(crate) fn set_resolution_inputs(app: &mut App, profile: &WallpaperSettings) {
    match profile.render_resolution {
        RenderResolution::Automatic => {
            app.resolution_width.clear();
            app.resolution_height.clear();
        }
        RenderResolution::Fixed { width, height } => {
            app.resolution_width = width.to_string();
            app.resolution_height = height.to_string();
        }
    }
}

pub(crate) fn persist_current_config(app: &App) {
    let Some(selected_id) = app.selected_id.as_deref() else {
        return;
    };
    let Some(entry) = app.entries.iter().find(|entry| entry.id == selected_id) else {
        return;
    };

    config::persist_selected(&app.config_path, &app.launch_settings, entry);
}
