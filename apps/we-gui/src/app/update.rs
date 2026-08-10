use std::{path::Path, time::Duration};

use iced::{window, Task};
use we_core::wallpaper::properties::UserPropertySchema;

use crate::{
    domain::{
        playlist_editor::{
            self, add_wallpaper, create_playlist, delete_playlist, move_entry, remove_entry,
            rename_playlist, set_default_duration_ms, set_entry_duration_ms, set_mode,
        },
        runtime_status::RuntimeStatus,
        ui_state::{AnimatedPreview, Sidebar},
    },
    platform::tray,
    services::{config, preferences, runtime, wallpaper as wallpaper_service},
    ui::sidebar::detail as wallpaper_detail,
};

use super::{
    detail_update::{persist_current_config, set_resolution_inputs},
    App, Message,
};

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
                let mut tasks = app
                    .entries
                    .iter()
                    .filter_map(|entry| {
                        let path = entry.preview.as_ref()?.clone();
                        (path.extension().and_then(|ext| ext.to_str()) == Some("gif")).then(|| {
                            Task::perform(
                                wallpaper_service::decode_gif(path.clone()),
                                move |result| Message::GifLoaded(path, result),
                            )
                        })
                    })
                    .collect::<Vec<_>>();
                let migration = migrate_legacy_shuffle_if_needed(app);
                tasks.push(migration);
                Task::batch(tasks)
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
                    app.animated_previews.insert(
                        path,
                        AnimatedPreview { frames, current: 0, elapsed: Duration::ZERO },
                    );
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
            Task::none()
        }
        Message::Detail(message) => super::detail_update::update(app, message),
        Message::PlayPressed => play_selected(app, PlaybackStart::SwitchOrStart),
        Message::StopPressed => {
            let stopped = app.shutdown_runtime();
            app.runtime_status =
                if stopped { RuntimeStatus::StoppedDaemon } else { RuntimeStatus::StopFailed };
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
                async move { rfd::FileDialog::new().set_title(title).pick_folder() },
                Message::AssetsPathPicked,
            )
        }
        Message::PickWorkshopPath => {
            let title = app.language.text(crate::domain::i18n::Text::SelectWorkshopDirectory);
            Task::perform(
                async move { rfd::FileDialog::new().set_title(title).pick_folder() },
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
            if let Err(error) = config::persist_force_scene_audio_loop(&app.config_path, value) {
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
        Message::PlaylistsPressed => {
            app.sidebar = match app.sidebar {
                Some(Sidebar::Playlist) => None,
                _ => Some(Sidebar::Playlist),
            };
            if app.sidebar == Some(Sidebar::Playlist) {
                ensure_playlist_selection(app);
            }
            Task::none()
        }
        Message::PlaylistSelect(name) => {
            if app.launch_settings.playlists.definitions.contains_key(&name) {
                app.playlist_selected = Some(name);
                sync_playlist_editor_inputs(app);
            }
            Task::none()
        }
        Message::PlaylistNewNameChanged(value) => {
            app.playlist_new_name_input = value;
            Task::none()
        }
        Message::PlaylistCreate => {
            let name = app.playlist_new_name_input.trim().to_string();
            match create_playlist(&mut app.launch_settings.playlists, &name) {
                Ok(()) => {
                    app.playlist_selected = Some(name);
                    app.playlist_new_name_input.clear();
                    sync_playlist_editor_inputs(app);
                    persist_playlist_changes(app);
                }
                Err(error) => set_playlist_error(app, error),
            }
            Task::none()
        }
        Message::PlaylistNameChanged(value) => {
            app.playlist_name_input = value;
            Task::none()
        }
        Message::PlaylistRename => {
            let Some(current) = app.playlist_selected.clone() else {
                return Task::none();
            };
            let was_running = app.runtime_playlist_active.as_deref() == Some(current.as_str());
            let next = app.playlist_name_input.trim().to_string();
            match rename_playlist(&mut app.launch_settings.playlists, &current, &next) {
                Ok(()) => {
                    app.playlist_selected = Some(next.clone());
                    sync_playlist_editor_inputs(app);
                    if persist_playlist_changes_and_reload_if(app, was_running) && was_running {
                        app.runtime_playlist_active = Some(next);
                    }
                }
                Err(error) => set_playlist_error(app, error),
            }
            Task::none()
        }
        Message::PlaylistDelete => {
            let Some(name) = app.playlist_selected.clone() else {
                return Task::none();
            };
            let was_running = match playlist_is_running(app, &name) {
                Ok(value) => value,
                Err(error) => {
                    set_playlist_error(app, error);
                    return Task::none();
                }
            };
            if was_running {
                let stopped = runtime::send_playlist_action("stop");
                let daemon_running = !stopped && runtime::daemon_is_running();
                if !playlist_stop_can_be_persisted(stopped, daemon_running) {
                    set_playlist_error(
                        app,
                        "the daemon is still running and did not accept the playlist stop command"
                            .to_string(),
                    );
                    return Task::none();
                }
                app.runtime_playlist_active = None;
                app.runtime_playlist_index = None;
            }
            match delete_playlist(&mut app.launch_settings.playlists, &name) {
                Ok(()) => {
                    app.playlist_selected =
                        app.launch_settings.playlists.definitions.keys().next().cloned();
                    sync_playlist_editor_inputs(app);
                    persist_playlist_changes(app);
                }
                Err(error) => set_playlist_error(app, error),
            }
            Task::none()
        }
        Message::PlaylistModeSelected(mode) => {
            let Some(name) = app.playlist_selected.clone() else {
                return Task::none();
            };
            match set_mode(&mut app.launch_settings.playlists, &name, mode) {
                Ok(()) => {
                    persist_playlist_changes_and_reload(app, Some(&name));
                }
                Err(error) => set_playlist_error(app, error),
            }
            Task::none()
        }
        Message::PlaylistDefaultDurationChanged(value) => {
            app.playlist_default_duration_input = value;
            Task::none()
        }
        Message::PlaylistDefaultDurationApply => {
            let Some(name) = app.playlist_selected.clone() else {
                return Task::none();
            };
            let Ok(duration_ms) = app.playlist_default_duration_input.parse::<u64>() else {
                set_playlist_error(
                    app,
                    "default duration must be an integer in milliseconds".to_string(),
                );
                return Task::none();
            };
            match set_default_duration_ms(&mut app.launch_settings.playlists, &name, duration_ms) {
                Ok(()) => {
                    persist_playlist_changes_and_reload(app, Some(&name));
                }
                Err(error) => set_playlist_error(app, error),
            }
            Task::none()
        }
        Message::PlaylistEntryDurationChanged { index, value } => {
            if index >= app.playlist_entry_duration_inputs.len() {
                return Task::none();
            }
            app.playlist_entry_duration_inputs[index] = value;
            Task::none()
        }
        Message::PlaylistEntryDurationApply(index) => {
            let Some(name) = app.playlist_selected.clone() else {
                return Task::none();
            };
            let Some(value) = app.playlist_entry_duration_inputs.get(index) else {
                return Task::none();
            };
            let Ok(duration_ms) = value.parse::<u64>() else {
                set_playlist_error(
                    app,
                    "entry duration must be an integer in milliseconds".to_string(),
                );
                return Task::none();
            };
            match set_entry_duration_ms(
                &mut app.launch_settings.playlists,
                &name,
                index,
                Some(duration_ms),
            ) {
                Ok(()) => {
                    persist_playlist_changes_and_reload(app, Some(&name));
                }
                Err(error) => set_playlist_error(app, error),
            }
            Task::none()
        }
        Message::PlaylistEntryDurationClear(index) => {
            let Some(name) = app.playlist_selected.clone() else {
                return Task::none();
            };
            match set_entry_duration_ms(&mut app.launch_settings.playlists, &name, index, None) {
                Ok(()) => {
                    sync_playlist_editor_inputs(app);
                    persist_playlist_changes_and_reload(app, Some(&name));
                }
                Err(error) => set_playlist_error(app, error),
            }
            Task::none()
        }
        Message::PlaylistEntryMove { index, direction } => {
            let Some(name) = app.playlist_selected.clone() else {
                return Task::none();
            };
            match move_entry(&mut app.launch_settings.playlists, &name, index, direction) {
                Ok(_) => {
                    sync_playlist_editor_inputs(app);
                    persist_playlist_changes_and_reload(app, Some(&name));
                }
                Err(error) => set_playlist_error(app, error),
            }
            Task::none()
        }
        Message::PlaylistEntryRemove(index) => {
            let Some(name) = app.playlist_selected.clone() else {
                return Task::none();
            };
            match remove_entry(&mut app.launch_settings.playlists, &name, index) {
                Ok(()) => {
                    sync_playlist_editor_inputs(app);
                    persist_playlist_changes_and_reload(app, Some(&name));
                }
                Err(error) => set_playlist_error(app, error),
            }
            Task::none()
        }
        Message::AddWallpaperToSelectedPlaylist(index) => {
            let (Some(name), Some(entry)) =
                (app.playlist_selected.clone(), app.entries.get(index).cloned())
            else {
                return Task::none();
            };
            match add_wallpaper(&mut app.launch_settings.playlists, &name, &entry) {
                Ok(()) => {
                    sync_playlist_editor_inputs(app);
                    persist_playlist_changes_and_reload(app, Some(&name));
                }
                Err(error) => set_playlist_error(app, error),
            }
            Task::none()
        }
        Message::PlaylistPlay => play_selected_playlist(app),
        Message::PlaylistNext => playlist_runtime_action(app, "next"),
        Message::PlaylistPrevious => playlist_runtime_action(app, "previous"),
        Message::PlaylistStop => playlist_runtime_action(app, "stop"),
        Message::StatusLoaded(result) => {
            app.runtime_status = match result {
                Ok(runtime::DaemonStatus::Running(text)) => {
                    app.playback_running = status_value(&text, "phase") == Some("running");
                    app.playback_paused = status_value(&text, "phase") == Some("paused");
                    if app.playback_running || app.playback_paused {
                        app.running_source = status_value(&text, "source").map(str::to_string);
                        app.runtime_playlist_active =
                            status_section_value(&text, "playlist_runtime", "active")
                                .filter(|value| *value != "false")
                                .map(str::to_string);
                        app.runtime_playlist_index =
                            status_section_value(&text, "playlist_runtime", "index")
                                .and_then(|value| value.parse::<usize>().ok());
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
        Message::StatusTick => Task::perform(runtime::fetch_status(), Message::StatusLoaded),
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
            if super::signal::take_shutdown_request() {
                return Task::done(Message::ExitRequested);
            }
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
        Message::ExitRequested => exit_application(app),
        Message::TrayAction(action) => match action {
            tray::TrayAction::ShowWindow => {
                if let Some(id) = app.main_window_id {
                    return window::gain_focus(id);
                }
                let (_id, task) = window::open(window::Settings::default());
                task.map(Message::WindowOpened)
            }
            tray::TrayAction::PlaySwitch => Task::done(Message::PlayPressed),
            tray::TrayAction::NextPlaylistItem => Task::done(Message::PlaylistNext),
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
            tray::TrayAction::Quit => Task::done(Message::ExitRequested),
        },
    }
}

fn exit_application(app: &mut App) -> Task<Message> {
    if !app.shutdown_runtime() {
        eprintln!("failed to stop daemon while exiting we-gui");
    }
    iced::exit()
}

fn status_value<'a>(status: &'a str, key: &str) -> Option<&'a str> {
    status
        .lines()
        .find_map(|line| line.strip_prefix(&format!("{key} = ")))
        .map(|value| value.trim_matches('"'))
}

fn status_section_value<'a>(status: &'a str, section: &str, key: &str) -> Option<&'a str> {
    let header = format!("[{section}]");
    let mut in_section = false;
    for line in status.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            in_section = trimmed == header;
            continue;
        }
        if in_section {
            if let Some(value) = trimmed.strip_prefix(&format!("{key} = ")) {
                return Some(value.trim_matches('"'));
            }
        }
    }
    None
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
        shuffle_enabled: app.legacy_shuffle.enabled,
        shuffle_interval_ms: app.legacy_shuffle.interval_ms,
        shuffle_include_video: app.legacy_shuffle.include_video,
        shuffle_include_scene: app.legacy_shuffle.include_scene,
        shuffle_include_web: app.legacy_shuffle.include_web,
        playlist_migration_completed: app.playlist_migration_completed,
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

fn migrate_legacy_shuffle_if_needed(app: &mut App) -> Task<Message> {
    if app.playlist_migration_completed {
        return Task::none();
    }

    if !app.launch_settings.playlists.definitions.is_empty() || !app.legacy_shuffle.enabled {
        app.playlist_migration_completed = true;
        return queue_preferences_save(app);
    }

    let previous = app.launch_settings.playlists.clone();
    match playlist_editor::migrate_legacy_shuffle(
        &mut app.launch_settings.playlists,
        &app.entries,
        app.legacy_shuffle,
    ) {
        Ok(true) => {
            app.playlist_selected = app.launch_settings.playlists.active.clone();
            sync_playlist_editor_inputs(app);
            if persist_playlist_changes(app) {
                synchronize_migrated_playlist_with_running_daemon(app);
                app.playlist_migration_completed = true;
                queue_preferences_save(app)
            } else {
                app.launch_settings.playlists = previous;
                sync_playlist_editor_inputs(app);
                Task::none()
            }
        }
        Ok(false) => {
            app.playlist_migration_completed = true;
            queue_preferences_save(app)
        }
        Err(_) => Task::none(),
    }
}

fn persist_playlist_changes(app: &mut App) -> bool {
    match config::persist_playlists(&app.config_path, &app.launch_settings.playlists) {
        Ok(()) => {
            app.runtime_status = RuntimeStatus::PlaylistSaved;
            true
        }
        Err(error) => {
            app.runtime_status = RuntimeStatus::ConfigSaveFailed(error.clone());
            eprintln!("failed to save playlists: {error}");
            false
        }
    }
}

fn persist_playlist_changes_and_reload(app: &mut App, edited_name: Option<&str>) -> bool {
    let reload_running =
        edited_name.is_some_and(|name| app.runtime_playlist_active.as_deref() == Some(name));
    persist_playlist_changes_and_reload_if(app, reload_running)
}

fn persist_playlist_changes_and_reload_if(app: &mut App, reload_running: bool) -> bool {
    if !persist_playlist_changes(app) {
        return false;
    }
    if reload_running && !runtime::try_switch(&app.config_path) {
        set_playlist_error(
            app,
            "playlist was saved but the running daemon could not reload it".to_string(),
        );
        return false;
    }
    true
}

fn set_playlist_error(app: &mut App, error: String) {
    app.runtime_status = RuntimeStatus::PlaylistError(error);
}

fn ensure_playlist_selection(app: &mut App) {
    let selected_is_valid = app
        .playlist_selected
        .as_deref()
        .is_some_and(|name| app.launch_settings.playlists.definitions.contains_key(name));
    if !selected_is_valid {
        app.playlist_selected = app
            .launch_settings
            .playlists
            .active
            .clone()
            .filter(|name| app.launch_settings.playlists.definitions.contains_key(name))
            .or_else(|| app.launch_settings.playlists.definitions.keys().next().cloned());
    }
    sync_playlist_editor_inputs(app);
}

fn sync_playlist_editor_inputs(app: &mut App) {
    let Some(name) = app.playlist_selected.clone() else {
        app.playlist_name_input.clear();
        app.playlist_default_duration_input.clear();
        app.playlist_entry_duration_inputs.clear();
        return;
    };
    let Some(playlist) = app.launch_settings.playlists.definitions.get(&name) else {
        app.playlist_selected = None;
        app.playlist_name_input.clear();
        app.playlist_default_duration_input.clear();
        app.playlist_entry_duration_inputs.clear();
        return;
    };
    app.playlist_name_input = name;
    app.playlist_default_duration_input = playlist.default_duration_ms.to_string();
    app.playlist_entry_duration_inputs = playlist
        .items
        .iter()
        .map(|item| item.duration_ms.map(|value| value.to_string()).unwrap_or_default())
        .collect();
}

fn play_selected_playlist(app: &mut App) -> Task<Message> {
    let Some(name) = app.playlist_selected.clone() else {
        set_playlist_error(app, "select a playlist first".to_string());
        return Task::none();
    };
    let Some(playlist) = app.launch_settings.playlists.definitions.get(&name) else {
        set_playlist_error(app, format!("playlist '{name}' does not exist"));
        return Task::none();
    };
    if playlist.items.is_empty() {
        set_playlist_error(app, format!("playlist '{name}' is empty"));
        return Task::none();
    }
    if !app.layerd_available {
        app.runtime_status = RuntimeStatus::DaemonNotFound;
        return Task::none();
    }

    app.launch_settings.playlists.active = Some(name.clone());
    if !persist_playlist_changes(app) {
        return Task::none();
    }
    app.runtime_shutdown = false;
    if let Err(error) = runtime::reap(&mut app.runtime_child) {
        eprintln!("failed to query daemon child status: {error}");
    }

    let controlled = runtime::play_playlist(&name)
        || (runtime::try_switch(&app.config_path) && runtime::play_playlist(&name));
    if controlled {
        app.playback_running = true;
        app.playback_paused = false;
        app.runtime_playlist_active = Some(name);
        return Task::perform(runtime::fetch_status(), Message::StatusLoaded);
    }

    match runtime::start(&app.config_path) {
        Ok(child) => {
            app.runtime_child = Some(child);
            app.playback_running = true;
            app.playback_paused = false;
            app.runtime_playlist_active = Some(name);
            app.runtime_status = RuntimeStatus::StartedDaemon;
            Task::perform(runtime::fetch_status(), Message::StatusLoaded)
        }
        Err(error) => {
            app.runtime_status = RuntimeStatus::StartFailed(error.to_string());
            Task::none()
        }
    }
}

fn synchronize_migrated_playlist_with_running_daemon(app: &mut App) {
    if !runtime::daemon_is_running() {
        return;
    }
    let Some(name) = app.launch_settings.playlists.active.clone() else {
        return;
    };

    if runtime::try_switch(&app.config_path) && runtime::play_playlist(&name) {
        app.runtime_shutdown = false;
        app.playback_running = true;
        app.playback_paused = false;
        app.runtime_playlist_active = Some(name);
        app.runtime_playlist_index = None;
        return;
    }

    match runtime::restart(&app.config_path, &mut app.runtime_child) {
        Ok(child) => {
            app.runtime_child = Some(child);
            app.runtime_shutdown = false;
            app.playback_running = true;
            app.playback_paused = false;
            app.runtime_playlist_active = Some(name);
            app.runtime_playlist_index = None;
        }
        Err(error) => set_playlist_error(
            app,
            format!(
                "legacy shuffle was migrated but the running daemon could not reload it: {error}"
            ),
        ),
    }
}

fn playlist_is_running(app: &App, playlist_name: &str) -> Result<bool, String> {
    if app.runtime_playlist_active.as_deref() == Some(playlist_name) {
        return Ok(true);
    }
    if !app.layerd_available {
        return Ok(false);
    }

    match runtime::fetch_status_sync()? {
        runtime::DaemonStatus::NotRunning => Ok(false),
        runtime::DaemonStatus::EmptyResponse => {
            Err("cannot determine the running playlist from an empty daemon status".to_string())
        }
        runtime::DaemonStatus::Running(status) => {
            Ok(status_section_value(&status, "playlist_runtime", "active")
                .filter(|active| *active != "false")
                == Some(playlist_name))
        }
    }
}

fn playlist_stop_can_be_persisted(command_succeeded: bool, daemon_running: bool) -> bool {
    command_succeeded || !daemon_running
}

fn playlist_runtime_action(app: &mut App, action: &str) -> Task<Message> {
    if action == "stop" {
        let daemon_stopped_playlist = runtime::send_playlist_action(action);
        let daemon_running = !daemon_stopped_playlist && runtime::daemon_is_running();
        if !playlist_stop_can_be_persisted(daemon_stopped_playlist, daemon_running) {
            set_playlist_error(
                app,
                "the daemon is still running and did not accept the playlist stop command"
                    .to_string(),
            );
            return Task::none();
        }
        app.launch_settings.playlists.active = None;
        app.runtime_playlist_active = None;
        app.runtime_playlist_index = None;
        if !persist_playlist_changes(app) {
            return Task::none();
        }
        return if daemon_stopped_playlist {
            Task::perform(runtime::fetch_status(), Message::StatusLoaded)
        } else {
            Task::none()
        };
    }

    if !runtime::send_playlist_action(action) {
        set_playlist_error(app, format!("failed to send playlist {action} command"));
        return Task::none();
    }
    Task::perform(runtime::fetch_status(), Message::StatusLoaded)
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

    if app.runtime_playlist_active.is_some() {
        let _ = runtime::send_playlist_action("stop");
    }
    app.launch_settings.playlists.active = None;
    app.runtime_playlist_active = None;
    app.runtime_playlist_index = None;

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
        PlaybackStart::SwitchOrStart => {
            runtime::start(&app.config_path).map_err(|error| error.to_string())
        }
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
}

fn selected_wallpaper_source(app: &App) -> Option<String> {
    let selected_id = app.selected_id.as_deref()?;
    let entry = app.entries.iter().find(|entry| entry.id == selected_id)?;
    Some(entry.project_json.parent().unwrap_or(&entry.project_json).to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::{
        effective_playback_start, playlist_stop_can_be_persisted, status_section_value,
        PlaybackStart,
    };

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
    fn playlist_runtime_status_reads_the_runtime_section_not_config_metadata() {
        let status = r#"
[playlists]
active = "Configured"

[playlist_runtime]
active = "Running"
index = 2
wallpaper_id = "42"
"#;
        assert_eq!(status_section_value(status, "playlist_runtime", "active"), Some("Running"));
        assert_eq!(status_section_value(status, "playlist_runtime", "index"), Some("2"));
    }

    #[test]
    fn playlist_stop_is_only_persisted_after_the_daemon_stops_progressing_or_is_absent() {
        assert!(playlist_stop_can_be_persisted(true, true));
        assert!(playlist_stop_can_be_persisted(true, false));
        assert!(playlist_stop_can_be_persisted(false, false));
        assert!(!playlist_stop_can_be_persisted(false, true));
    }
}
