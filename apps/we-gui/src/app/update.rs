use std::{path::Path, time::Duration};

use iced::{window, Task};
use rand::Rng;
use we_core::wallpaper::{properties::UserPropertySchema, WallpaperType};

use crate::{
    domain::{
        runtime_status::RuntimeStatus,
        ui_state::{AnimatedPreview, Sidebar},
    },
    platform::tray,
    services::{config, preferences, runtime, wallpaper as wallpaper_service},
    ui::sidebar::detail as wallpaper_detail,
};

use super::{detail_update::{persist_current_config, set_resolution_inputs}, App, Message};

pub(crate) fn update(app: &mut App, message: Message) -> Task<Message> {
    match message {
        Message::AutoScan => Task::perform(
            wallpaper_service::scan(app.ui_settings.workshop_path.clone()),
            Message::Scanned,
        ),
        Message::Scanned(result) => match result {
            Ok(entries) => {
                app.entries = entries;
                app.animated_previews.clear();
                Task::batch(app.entries.iter().filter_map(|entry| {
                    let path = entry.preview.as_ref()?.clone();
                    (path.extension().and_then(|ext| ext.to_str()) == Some("gif")).then(|| {
                        Task::perform(wallpaper_service::decode_gif(path.clone()), move |result| Message::GifLoaded(path, result))
                    })
                }))
            }
            Err(_err) => Task::none(),
        },
        Message::SelectWallpaper(index) => {
            if !select_wallpaper(app, index, true) {
                return Task::none();
            }
            Task::perform(runtime::fetch_status(), Message::StatusLoaded)
        }
        Message::GifLoaded(path, result) => {
            if let Ok(frames) = result {
                if !frames.is_empty() {
                    app.animated_previews.insert(path, AnimatedPreview { frames, current: 0, elapsed: Duration::ZERO });
                }
            }
            Task::none()
        }
        Message::GifTick => {
            for preview in app.animated_previews.values_mut() {
                preview.elapsed += Duration::from_millis(16);
                while preview.elapsed >= preview.frames[preview.current].delay {
                    preview.elapsed -= preview.frames[preview.current].delay;
                    preview.current = (preview.current + 1) % preview.frames.len();
                }
            }
            if app.ui_settings.shuffle_enabled && app.playback_running && !app.playback_paused {
                app.shuffle_elapsed += Duration::from_millis(16);
                let interval = Duration::from_millis(u64::from(app.ui_settings.shuffle_interval_ms));
                if app.shuffle_elapsed >= interval {
                    return shuffle_to_next(app);
                }
            }
            Task::none()
        }
        Message::Detail(message) => super::detail_update::update(app, message),
        Message::PlayPressed => play_selected(app, PlaybackStart::SwitchOrStart),
        Message::ShufflePlaybackPressed => play_selected(app, PlaybackStart::Restart),
        Message::StopPressed => {
            app.shuffle_elapsed = Duration::ZERO;
            let stopped = app.shutdown_runtime();
            app.runtime_status = if stopped {
                RuntimeStatus::StoppedDaemon
            } else {
                RuntimeStatus::StopFailed
            };
            if !stopped {
                eprintln!("failed to stop daemon via IPC or owned child process");
            }
            Task::none()
        }
        Message::SettingsPressed => {
            app.sidebar = match app.sidebar {
                Some(Sidebar::Settings) => None,
                _ => Some(Sidebar::Settings),
            };
            app.show_settings = app.sidebar == Some(Sidebar::Settings);
            if app.show_settings {
                return Task::perform(runtime::fetch_status(), Message::StatusLoaded);
            }
            Task::none()
        }
        Message::SearchChanged(value) => {
            app.search_query = value;
            Task::none()
        }
        Message::TypeFilterSelected(value) => {
            app.type_filter = value;
            Task::none()
        }
        Message::PaneResized(event) => {
            app.panes.resize(event.split, event.ratio.clamp(0.45, 0.82));
            Task::none()
        }
        Message::AssetsPathChanged(value) => {
            app.ui_settings.assets_path = value;
            super::settings::sync(app);
            Task::none()
        }
        Message::WorkshopPathChanged(value) => {
            app.ui_settings.workshop_path = value.clone();
            super::settings::sync(app);
            if Path::new(&value).is_dir() {
                return Task::perform(
                    wallpaper_service::scan(app.ui_settings.workshop_path.clone()),
                    Message::Scanned,
                );
            }
            Task::none()
        }
        Message::RendererLibraryPathChanged(value) => {
            app.ui_settings.renderer_library_path = value;
            super::settings::sync(app);
            Task::none()
        }
        Message::RendererCachePathChanged(value) => {
            app.ui_settings.renderer_cache_path = value;
            super::settings::sync(app);
            Task::none()
        }
        Message::PickAssetsPath => {
            let title = app.language.text(crate::domain::i18n::Text::SelectAssetsDirectory);
            Task::perform(
                async move {
                    rfd::FileDialog::new()
                        .set_title(title)
                        .pick_folder()
                },
                Message::AssetsPathPicked,
            )
        }
        Message::PickWorkshopPath => {
            let title = app.language.text(crate::domain::i18n::Text::SelectWorkshopDirectory);
            Task::perform(
                async move {
                    rfd::FileDialog::new().set_title(title).pick_folder()
                },
                Message::WorkshopPathPicked,
            )
        }
        Message::AssetsPathPicked(path) => {
            if let Some(path) = path {
                if path.join("assets").is_dir() {
                    app.ui_settings.assets_path = path.display().to_string();
                    super::settings::sync(app);
                } else {
                    app.runtime_status = RuntimeStatus::InvalidWallpaperEngineDirectory;
                }
            }
            Task::none()
        }
        Message::WorkshopPathPicked(path) => {
            if let Some(path) = path {
                app.ui_settings.workshop_path = path.display().to_string();
                super::settings::sync(app);
                return Task::perform(
                    wallpaper_service::scan(app.ui_settings.workshop_path.clone()),
                    Message::Scanned,
                );
            }
            Task::none()
        }
        Message::FpsLimitChanged(value) => {
            app.ui_settings.fps_limit = value;
            super::settings::sync(app);
            Task::none()
        }
        Message::InteractiveToggled(value) => {
            app.ui_settings.interactive = value;
            super::settings::sync(app);
            Task::none()
        }
        Message::ForceSceneAudioLoopToggled(value) => {
            if let Err(error) =
                config::persist_force_scene_audio_loop(&app.config_path, value)
            {
                app.runtime_status = RuntimeStatus::ConfigSaveFailed(error.clone());
                eprintln!("failed to save config: {error}");
            } else {
                app.ui_settings.force_scene_audio_loop = value;
                super::settings::sync(app);
            }
            Task::none()
        }
        Message::ShowFpsToggled(value) => {
            app.ui_settings.show_fps = value;
            super::settings::sync(app);
            Task::none()
        }
        Message::ScaleModeSelected(value) => {
            app.ui_settings.scale_mode = value;
            super::settings::sync(app);
            Task::none()
        }
        Message::PreferDmabufToggled(value) => {
            app.ui_settings.prefer_dmabuf = value;
            super::settings::sync(app);
            Task::none()
        }
        Message::AllowShmFallbackToggled(value) => {
            app.ui_settings.allow_shm_fallback = value;
            super::settings::sync(app);
            Task::none()
        }
        Message::LanguageSelected(language) => {
            app.language = language;
            if let Some(tray) = app.tray.as_mut() {
                tray.set_language(language);
            }
            queue_preferences_save(app)
        }
        Message::PreferencesSaved { generation, result } => {
            if generation != app.preferences_generation {
                return persist_gui_preferences(app);
            }
            if let Err(error) = result {
                eprintln!("failed to save GUI preferences: {error}");
                app.runtime_status = RuntimeStatus::PreferencesSaveFailed(error);
            }
            Task::none()
        }
        Message::ShufflePressed => shuffle_to_next(app),
        Message::ShuffleEnabledToggled(value) => {
            app.ui_settings.shuffle_enabled = value;
            app.shuffle_elapsed = Duration::ZERO;
            queue_preferences_save(app)
        }
        Message::ShuffleIntervalSelected(value) => {
            if crate::domain::settings::is_shuffle_interval_ms(value) {
                app.ui_settings.shuffle_interval_ms = value;
                app.ui_settings.shuffle_interval_input = value.to_string();
                app.shuffle_elapsed = Duration::ZERO;
                return queue_preferences_save(app);
            }
            Task::none()
        }
        Message::ShuffleIntervalChanged(value) => {
            app.ui_settings.shuffle_interval_input = value.clone();
            let Ok(interval_ms) = value.parse::<u32>() else {
                return Task::none();
            };
            if !crate::domain::settings::is_shuffle_interval_ms(interval_ms) {
                return Task::none();
            }
            app.ui_settings.shuffle_interval_ms = interval_ms;
            app.shuffle_elapsed = Duration::ZERO;
            queue_preferences_save(app)
        }
        Message::ShuffleIncludeVideoToggled(value) => {
            if !value
                && !app.ui_settings.shuffle_include_scene
                && !app.ui_settings.shuffle_include_web
            {
                return Task::none();
            }
            app.ui_settings.shuffle_include_video = value;
            app.shuffle_elapsed = Duration::ZERO;
            queue_preferences_save(app)
        }
        Message::ShuffleIncludeSceneToggled(value) => {
            if !value
                && !app.ui_settings.shuffle_include_video
                && !app.ui_settings.shuffle_include_web
            {
                return Task::none();
            }
            app.ui_settings.shuffle_include_scene = value;
            app.shuffle_elapsed = Duration::ZERO;
            queue_preferences_save(app)
        }
        Message::ShuffleIncludeWebToggled(value) => {
            if !value
                && !app.ui_settings.shuffle_include_video
                && !app.ui_settings.shuffle_include_scene
            {
                return Task::none();
            }
            app.ui_settings.shuffle_include_web = value;
            app.shuffle_elapsed = Duration::ZERO;
            queue_preferences_save(app)
        }
        Message::StatusLoaded(result) => {
            app.runtime_status = match result {
                Ok(runtime::DaemonStatus::Running(text)) => {
                    app.playback_running = status_value(&text, "phase") == Some("running");
                    app.playback_paused = status_value(&text, "phase") == Some("paused");
                    if app.playback_running || app.playback_paused {
                        app.running_source = status_value(&text, "source").map(str::to_string);
                    } else {
                        app.clear_playback_state();
                    }
                    RuntimeStatus::Raw(text)
                }
                Ok(runtime::DaemonStatus::NotRunning) => {
                    app.clear_playback_state();
                    RuntimeStatus::DaemonNotRunning
                }
                Ok(runtime::DaemonStatus::EmptyResponse) => {
                    app.clear_playback_state();
                    RuntimeStatus::EmptyResponse
                }
                Err(err) => {
                    app.clear_playback_state();
                    RuntimeStatus::Unavailable(err)
                }
            };
            Task::none()
        }
        Message::StatusTick => {
            Task::perform(runtime::fetch_status(), Message::StatusLoaded)
        }
        Message::OutputsLoaded(result) => {
            if let Ok(outputs) = result {
                app.selected_outputs = outputs.iter().cloned().collect();
                app.outputs = outputs;
            }
            Task::none()
        }
        Message::ToggleOutput(output) => {
            if !app.selected_outputs.remove(&output) {
                app.selected_outputs.insert(output);
            }
            Task::none()
        }
        Message::WindowResized(size) => {
            app.viewport_width = size.width;
            Task::none()
        }
        Message::WindowCloseRequested(id) => window::close(id),
        Message::WindowOpened(id) => {
            app.main_window_id = Some(id);
            Task::none()
        }
        Message::WindowClosed(id) => {
            if app.main_window_id == Some(id) {
                app.main_window_id = None;
            }
            Task::none()
        }
        Message::TrayTick => {
            if let Some(tray) = app.tray.as_mut() {
                if let Some(action) = tray.poll_action() {
                    return Task::done(Message::TrayAction(action));
                }
            }
            Task::none()
        }
        Message::ThemeTick => {
            app.theme = super::init::detect_system_theme();
            Task::none()
        }
        Message::TrayAction(action) => match action {
            tray::TrayAction::ShowWindow => {
                if let Some(id) = app.main_window_id {
                    return window::gain_focus(id);
                }
                let (_id, task) = window::open(window::Settings::default());
                task.map(Message::WindowOpened)
            }
            tray::TrayAction::PlaySwitch => Task::done(Message::PlayPressed),
            tray::TrayAction::ShuffleOnce => Task::done(Message::ShufflePressed),
            tray::TrayAction::Stop => Task::done(Message::StopPressed),
            tray::TrayAction::Pause => {
                if runtime::send_control("pause") {
                    app.playback_paused = true;
                }
                Task::none()
            }
            tray::TrayAction::Resume => {
                if runtime::send_control("resume") {
                    app.playback_running = true;
                    app.playback_paused = false;
                }
                Task::none()
            }
            tray::TrayAction::Quit => {
                if !app.shutdown_runtime() {
                    eprintln!("failed to stop daemon while exiting we-gui");
                }
                iced::exit()
            }
        },
    }
}

fn status_value<'a>(status: &'a str, key: &str) -> Option<&'a str> {
    status.lines().find_map(|line| line.strip_prefix(&format!("{key} = ")))
        .map(|value| value.trim_matches('"'))
}

fn queue_preferences_save(app: &mut App) -> Task<Message> {
    app.preferences_generation = app.preferences_generation.wrapping_add(1);
    if app.preferences_path.is_some() {
        return persist_gui_preferences(app);
    }

    let error = "XDG_CONFIG_HOME and HOME are unavailable".to_string();
    eprintln!("failed to save GUI preferences: {error}");
    app.runtime_status = RuntimeStatus::PreferencesSaveFailed(error);
    Task::none()
}

fn persist_gui_preferences(app: &App) -> Task<Message> {
    let Some(path) = app.preferences_path.clone() else {
        return Task::none();
    };
    let generation = app.preferences_generation;
    let preferences = preferences::GuiPreferences {
        language: app.language,
        shuffle_enabled: app.ui_settings.shuffle_enabled,
        shuffle_interval_ms: app.ui_settings.shuffle_interval_ms,
        shuffle_include_video: app.ui_settings.shuffle_include_video,
        shuffle_include_scene: app.ui_settings.shuffle_include_scene,
        shuffle_include_web: app.ui_settings.shuffle_include_web,
    };
    Task::perform(async move { preferences::save(&path, preferences) }, move |result| {
        Message::PreferencesSaved { generation, result }
    })
}

fn select_wallpaper(app: &mut App, index: usize, show_details: bool) -> bool {
    let Some(entry) = app.entries.get(index).cloned() else {
        return false;
    };

    app.selected_id = Some(entry.id.clone());
    app.selected_schema = UserPropertySchema::from_project_file(&entry.project_json)
        .unwrap_or(UserPropertySchema { entries: Vec::new() });
    let profile = app.launch_settings.wallpapers.entry(entry.id.clone()).or_default().clone();
    set_resolution_inputs(app, &profile);
    app.shuffle_elapsed = Duration::ZERO;
    if let Err(error) = config::persist_selected(&app.config_path, &app.launch_settings, &entry) {
        app.runtime_status = RuntimeStatus::ConfigSaveFailed(error.clone());
        eprintln!("failed to save config: {error}");
    }
    if show_details {
        app.sidebar = Some(Sidebar::Detail);
        app.detail_tab = wallpaper_detail::DetailTab::Actions;
    }
    true
}

fn shuffle_to_next(app: &mut App) -> Task<Message> {
    let candidates = shuffle_candidate_indices(app);
    if candidates.is_empty() {
        app.shuffle_elapsed = Duration::ZERO;
        app.runtime_status = RuntimeStatus::NoShuffleWallpapers;
        return Task::none();
    }

    let index = candidates[rand::rng().random_range(0..candidates.len())];
    if !select_wallpaper(app, index, false) {
        return Task::none();
    }
    Task::done(Message::ShufflePlaybackPressed)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PlaybackStart {
    SwitchOrStart,
    Restart,
}

fn play_selected(app: &mut App, start: PlaybackStart) -> Task<Message> {
    app.runtime_shutdown = false;
    if !app.layerd_available {
        app.runtime_status = RuntimeStatus::DaemonNotFound;
        return Task::none();
    }

    if let Err(error) = persist_current_config(app) {
        app.runtime_status = RuntimeStatus::ConfigSaveFailed(error.clone());
        eprintln!("failed to save config: {error}");
        return Task::none();
    }

    if let Err(error) = runtime::reap(&mut app.runtime_child) {
        eprintln!("failed to query daemon child status: {error}");
    }

    let start = effective_playback_start(
        start,
        std::env::var_os(we_core::install_layout::RENDERER_LIBRARY_OVERRIDE_ENV).is_some(),
        app.runtime_child.is_some(),
    );

    if start == PlaybackStart::SwitchOrStart && runtime::try_switch(&app.config_path) {
        app.runtime_status = RuntimeStatus::SwitchedDaemon;
        mark_selected_wallpaper_running(app);
        return Task::none();
    }

    let spawn = match start {
        PlaybackStart::SwitchOrStart => runtime::start(&app.config_path).map_err(|error| error.to_string()),
        PlaybackStart::Restart => runtime::restart(&app.config_path, &mut app.runtime_child),
    };

    match spawn {
        Ok(child) => {
            app.runtime_child = Some(child);
            app.runtime_status = RuntimeStatus::StartedDaemon;
            mark_selected_wallpaper_running(app);
        }
        Err(error) => {
            app.clear_playback_state();
            app.runtime_status = RuntimeStatus::StartFailed(error.clone());
            eprintln!("failed to start daemon: {error}");
        }
    }
    Task::none()
}

fn effective_playback_start(
    requested: PlaybackStart,
    forced_renderer_library: bool,
    owns_daemon_child: bool,
) -> PlaybackStart {
    if requested == PlaybackStart::SwitchOrStart && forced_renderer_library && !owns_daemon_child {
        PlaybackStart::Restart
    } else {
        requested
    }
}

fn mark_selected_wallpaper_running(app: &mut App) {
    app.playback_running = true;
    app.playback_paused = false;
    app.running_source = selected_wallpaper_source(app);
    app.shuffle_elapsed = Duration::ZERO;
}

fn shuffle_candidate_indices(app: &App) -> Vec<usize> {
    shuffle_candidate_indices_for(
        &app.entries,
        app.running_source.as_deref(),
        app.ui_settings.shuffle_include_video,
        app.ui_settings.shuffle_include_scene,
        app.ui_settings.shuffle_include_web,
    )
}

fn shuffle_candidate_indices_for(
    entries: &[we_core::wallpaper::WallpaperEntry],
    running_source: Option<&str>,
    include_video: bool,
    include_scene: bool,
    include_web: bool,
) -> Vec<usize> {
    entries
        .iter()
        .enumerate()
        .filter(|(_, entry)| {
            let included = match entry.ty {
                WallpaperType::Video => include_video,
                WallpaperType::Scene => include_scene,
                WallpaperType::Web => include_web,
                WallpaperType::Unknown => false,
            };
            let source = entry.project_json.parent().unwrap_or(&entry.project_json).to_string_lossy();
            included && running_source != Some(source.as_ref())
        })
        .map(|(index, _)| index)
        .collect()
}

fn selected_wallpaper_source(app: &App) -> Option<String> {
    let selected_id = app.selected_id.as_deref()?;
    let entry = app.entries.iter().find(|entry| entry.id == selected_id)?;
    Some(
        entry
            .project_json
            .parent()
            .unwrap_or(&entry.project_json)
            .to_string_lossy()
            .into_owned(),
    )
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use we_core::wallpaper::{WallpaperEntry, WallpaperType};

    use super::{effective_playback_start, shuffle_candidate_indices_for, PlaybackStart};

    #[test]
    fn appimage_restarts_an_unowned_daemon_before_first_playback() {
        assert_eq!(
            effective_playback_start(PlaybackStart::SwitchOrStart, true, false),
            PlaybackStart::Restart,
        );
        assert_eq!(
            effective_playback_start(PlaybackStart::SwitchOrStart, true, true),
            PlaybackStart::SwitchOrStart,
        );
        assert_eq!(
            effective_playback_start(PlaybackStart::SwitchOrStart, false, false),
            PlaybackStart::SwitchOrStart,
        );
    }

    #[test]
    fn shuffle_excludes_running_source_not_an_unplayed_selection() {
        let entries = vec![
            wallpaper("selected", "/wallpapers/selected/project.json", WallpaperType::Video),
            wallpaper("running", "/wallpapers/running/project.json", WallpaperType::Scene),
            wallpaper("other", "/wallpapers/other/project.json", WallpaperType::Web),
        ];

        assert_eq!(
            shuffle_candidate_indices_for(
                &entries,
                Some("/wallpapers/running"),
                true,
                true,
                true,
            ),
            vec![0, 2],
        );
    }

    fn wallpaper(id: &str, project_json: &str, ty: WallpaperType) -> WallpaperEntry {
        WallpaperEntry {
            id: id.to_string(),
            project_json: PathBuf::from(project_json),
            title: id.to_string(),
            ty,
            preview: None,
            source_file: None,
        }
    }
}
