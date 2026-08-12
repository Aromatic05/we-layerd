use std::{path::Path, time::Duration};

use iced::{window, Task};
use we_core::config::{load_launch_settings, OutputBinding};
use we_core::profile::{
    apply_profile_to_outputs, create_profile, delete_profile, rename_playlist_references,
    rename_profile, save_current_to_profile,
};
use we_core::wallpaper::{
    properties::UserPropertySchema,
    settings::{inherited_final_output_msaa, WallpaperSettings},
};

use crate::{
    domain::{
        library_grid::{bounded_animation_candidates, grid_window},
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
    detail_update::{persist_playback_config, persist_wallpaper_profiles, set_resolution_inputs},
    state::OutputRuntimeState,
    App, Message,
};

const MAX_CONCURRENT_GIF_DECODES: usize = 2;

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
                app.gif_preview_desired.clear();
                app.gif_preview_failed.clear();
                app.library_scroll_y = 0.0;
                refresh_filtered_entries(app);
                let previews = refresh_visible_gif_previews(app);
                let migration = migrate_legacy_shuffle_if_needed(app);
                Task::batch(vec![previews, migration])
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
            app.gif_preview_loading.remove(&path);
            match result {
                Ok(frames) if !frames.is_empty() && app.gif_preview_desired.contains(&path) => {
                    app.animated_previews.insert(
                        path.clone(),
                        AnimatedPreview { frames, current: 0, elapsed: Duration::ZERO },
                    );
                    app.gif_preview_failed.remove(&path);
                }
                Ok(_) | Err(_) => {
                    if app.gif_preview_desired.contains(&path) {
                        app.gif_preview_failed.insert(path);
                    }
                }
            }
            schedule_desired_gif_previews(app)
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
        Message::LibraryScrolled { offset_y, viewport_width, viewport_height } => {
            app.library_scroll_y = offset_y.max(0.0);
            app.library_viewport_width = viewport_width.max(180.0);
            app.library_viewport_height = viewport_height.max(1.0);
            refresh_visible_gif_previews(app)
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
            app.library_scroll_y = 0.0;
            refresh_filtered_entries(app);
            Task::batch(vec![
                refresh_visible_gif_previews(app),
                iced::widget::operation::scroll_to(
                    "library.scroll",
                    iced::widget::operation::AbsoluteOffset { x: 0.0, y: 0.0 },
                ),
            ])
        }
        Message::TypeFilterSelected(value) => {
            app.type_filter = value;
            app.library_scroll_y = 0.0;
            refresh_filtered_entries(app);
            Task::batch(vec![
                refresh_visible_gif_previews(app),
                iced::widget::operation::scroll_to(
                    "library.scroll",
                    iced::widget::operation::AbsoluteOffset { x: 0.0, y: 0.0 },
                ),
            ])
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
        Message::MediaIntegrationToggled(value) => {
            app.ui_settings.media_integration = value;
            persist_host_integration_settings(app);
            Task::none()
        }
        Message::AudioSpectrumToggled(value) => {
            app.ui_settings.audio_spectrum = value;
            persist_host_integration_settings(app);
            Task::none()
        }
        Message::AudioSourceChanged(value) => {
            app.ui_settings.audio_source = value;
            super::settings::sync(app);
            Task::none()
        }
        Message::AudioSourceApply => {
            persist_host_integration_settings(app);
            Task::none()
        }
        Message::FocusedRuleSelected(value) => {
            app.ui_settings.rule_focused = value;
            persist_host_integration_settings(app);
            Task::none()
        }
        Message::MaximizedRuleSelected(value) => {
            app.ui_settings.rule_maximized = value;
            persist_host_integration_settings(app);
            Task::none()
        }
        Message::FullscreenRuleSelected(value) => {
            app.ui_settings.rule_fullscreen = value;
            persist_host_integration_settings(app);
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
                sync_selected_outputs_for_playlist(app);
            }
            Task::none()
        }
        Message::ProfilesPressed => {
            app.sidebar = match app.sidebar {
                Some(Sidebar::Profile) => None,
                _ => Some(Sidebar::Profile),
            };
            if app.sidebar == Some(Sidebar::Profile) {
                ensure_profile_selection(app);
            }
            Task::none()
        }
        Message::ProfileSelect(name) => {
            if app.launch_settings.profiles.definitions.contains_key(&name) {
                app.profile_selected = Some(name);
                sync_profile_inputs(app);
            }
            Task::none()
        }
        Message::ProfileNewNameChanged(value) => {
            app.profile_new_name_input = value;
            Task::none()
        }
        Message::ProfileCreate => {
            let name = app.profile_new_name_input.trim().to_string();
            match create_profile(
                &mut app.launch_settings.profiles,
                &name,
                &app.launch_settings.outputs,
            ) {
                Ok(()) => {
                    app.launch_settings.profiles.active = Some(name.clone());
                    app.profile_selected = Some(name);
                    app.profile_new_name_input.clear();
                    sync_profile_inputs(app);
                    persist_profile_changes(app, true);
                }
                Err(error) => set_profile_error(app, error),
            }
            Task::none()
        }
        Message::ProfileNameChanged(value) => {
            app.profile_name_input = value;
            Task::none()
        }
        Message::ProfileRename => {
            let Some(current) = app.profile_selected.clone() else {
                return Task::none();
            };
            let next = app.profile_name_input.trim().to_string();
            match rename_profile(&mut app.launch_settings.profiles, &current, &next) {
                Ok(()) => {
                    app.profile_selected = Some(next);
                    sync_profile_inputs(app);
                    persist_profile_changes(app, true);
                }
                Err(error) => set_profile_error(app, error),
            }
            Task::none()
        }
        Message::ProfileDelete => {
            let Some(name) = app.profile_selected.clone() else {
                return Task::none();
            };
            match delete_profile(&mut app.launch_settings.profiles, &name) {
                Ok(()) => {
                    app.profile_selected =
                        app.launch_settings.profiles.definitions.keys().next().cloned();
                    sync_profile_inputs(app);
                    persist_profile_changes(app, true);
                }
                Err(error) => set_profile_error(app, error),
            }
            Task::none()
        }
        Message::ProfileSaveCurrent => {
            let Some(name) = app.profile_selected.clone() else {
                return Task::none();
            };
            match save_current_to_profile(
                &mut app.launch_settings.profiles,
                &name,
                &app.launch_settings.outputs,
            ) {
                Ok(()) => {
                    app.launch_settings.profiles.active = Some(name);
                    persist_profile_changes(app, true);
                }
                Err(error) => set_profile_error(app, error),
            }
            Task::none()
        }
        Message::ProfileApply => {
            let Some(name) = app.profile_selected.clone() else {
                set_profile_error(app, "select a profile first".to_string());
                return Task::none();
            };
            let profiles = app.launch_settings.profiles.clone();
            let playlists = app.launch_settings.playlists.clone();
            match apply_profile_to_outputs(
                &profiles,
                &name,
                &playlists,
                &mut app.launch_settings.outputs,
                |source| Path::new(source).join("project.json").is_file(),
            ) {
                Ok(()) => {
                    app.launch_settings.profiles.active = Some(name);
                    if !persist_profile_changes(app, false) {
                        return Task::none();
                    }
                    start_or_reconfigure_multi_output(app)
                }
                Err(error) => {
                    set_profile_error(app, error);
                    Task::none()
                }
            }
        }
        Message::PlaylistSelect(name) => {
            if app.launch_settings.playlists.definitions.contains_key(&name) {
                app.playlist_selected = Some(name);
                sync_playlist_editor_inputs(app);
                sync_selected_outputs_for_playlist(app);
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
                    sync_selected_outputs_for_playlist(app);
                    persist_playlist_changes_and_reload(app);
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
            let was_running = match playlist_is_running(app, &current) {
                Ok(value) => value,
                Err(error) => {
                    set_playlist_error(app, error);
                    return Task::none();
                }
            };
            let next = app.playlist_name_input.trim().to_string();
            match rename_playlist(&mut app.launch_settings.playlists, &current, &next) {
                Ok(()) => {
                    for binding in app.launch_settings.outputs.values_mut() {
                        if binding.playlist.as_deref() == Some(current.as_str()) {
                            binding.playlist = Some(next.clone());
                        }
                    }
                    rename_playlist_references(&mut app.launch_settings.profiles, &current, &next);
                    app.playlist_selected = Some(next.clone());
                    sync_playlist_editor_inputs(app);
                    sync_selected_outputs_for_playlist(app);
                    if persist_playlist_changes_and_reload(app)
                        && was_running
                        && app.launch_settings.playlists.active.as_deref() == Some(next.as_str())
                    {
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
            let globally_running = match global_playlist_is_running(app, &name) {
                Ok(value) => value,
                Err(error) => {
                    set_playlist_error(app, error);
                    return Task::none();
                }
            };
            if globally_running {
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
                    let active_profile_references_deleted =
                        app.launch_settings
                            .profiles
                            .active
                            .as_deref()
                            .and_then(|active| app.launch_settings.profiles.definitions.get(active))
                            .is_some_and(|profile| {
                                profile.outputs.values().any(|binding| {
                                    binding.playlist.as_deref() == Some(name.as_str())
                                })
                            });
                    if active_profile_references_deleted {
                        app.launch_settings.profiles.active = None;
                    }
                    app.launch_settings
                        .outputs
                        .retain(|_, binding| binding.playlist.as_deref() != Some(name.as_str()));
                    app.playlist_selected =
                        app.launch_settings.playlists.definitions.keys().next().cloned();
                    sync_playlist_editor_inputs(app);
                    sync_selected_outputs_for_playlist(app);
                    persist_playlist_changes_and_reload(app);
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
                    persist_playlist_changes_and_reload(app);
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
                    persist_playlist_changes_and_reload(app);
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
                    persist_playlist_changes_and_reload(app);
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
                    persist_playlist_changes_and_reload(app);
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
                    persist_playlist_changes_and_reload(app);
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
                    persist_playlist_changes_and_reload(app);
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
                    persist_playlist_changes_and_reload(app);
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
                        app.runtime_outputs = parse_output_runtime_states(&text);
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
                app.outputs = outputs;
                if app.sidebar == Some(Sidebar::Playlist) && app.playlist_selected.is_some() {
                    sync_selected_outputs_for_playlist(app);
                } else if app.selected_id.is_some() {
                    sync_selected_outputs_for_wallpaper(app);
                } else if app.launch_settings.outputs.is_empty() {
                    app.selected_outputs = app.outputs.iter().cloned().collect();
                } else {
                    app.selected_outputs.clear();
                }
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

fn refresh_filtered_entries(app: &mut App) {
    let query = app.search_query.to_lowercase();
    let type_filter = app.type_filter;
    app.filtered_entry_indices.clear();
    app.filtered_entry_indices.extend(
        app.entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| {
                type_filter.map_or(true, |ty| entry.ty == ty)
                    && (query.is_empty() || entry.title.to_lowercase().contains(&query))
            })
            .map(|(index, _)| index),
    );
}

fn refresh_visible_gif_previews(app: &mut App) -> Task<Message> {
    let layout = grid_window(
        app.filtered_entry_indices.len(),
        app.library_viewport_width,
        app.library_scroll_y,
        app.library_viewport_height,
        app.playlist_selected.is_some(),
    );
    let desired = bounded_animation_candidates(
        app.filtered_entry_indices[layout.start_item..layout.end_item]
            .iter()
            .filter_map(|index| app.entries.get(*index))
            .filter_map(|entry| entry.preview.as_ref())
            .filter(|path| {
                path.extension()
                    .and_then(|extension| extension.to_str())
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("gif"))
            })
            .cloned(),
    )
    .into_iter()
    .collect::<std::collections::HashSet<_>>();

    app.gif_preview_desired = desired;
    app.animated_previews.retain(|path, _| app.gif_preview_desired.contains(path));
    app.gif_preview_failed.retain(|path| app.gif_preview_desired.contains(path));
    schedule_desired_gif_previews(app)
}

fn schedule_desired_gif_previews(app: &mut App) -> Task<Message> {
    let available = MAX_CONCURRENT_GIF_DECODES.saturating_sub(app.gif_preview_loading.len());
    if available == 0 {
        return Task::none();
    }

    let paths = app
        .gif_preview_desired
        .iter()
        .filter(|path| {
            !app.animated_previews.contains_key(*path)
                && !app.gif_preview_loading.contains(*path)
                && !app.gif_preview_failed.contains(*path)
        })
        .take(available)
        .cloned()
        .collect::<Vec<_>>();
    if paths.is_empty() {
        return Task::none();
    }

    let mut tasks = Vec::with_capacity(paths.len());
    for path in paths {
        app.gif_preview_loading.insert(path.clone());
        tasks.push(Task::perform(wallpaper_service::decode_gif(path.clone()), move |result| {
            Message::GifLoaded(path, result)
        }));
    }
    Task::batch(tasks)
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
    let inherited_msaa = inherited_final_output_msaa(app.launch_settings.msaa_samples, entry.ty);
    let profile = app
        .launch_settings
        .wallpapers
        .entry(entry.id.clone())
        .or_insert_with(|| WallpaperSettings {
            msaa_samples: inherited_msaa,
            ..WallpaperSettings::default()
        })
        .clone();
    set_resolution_inputs(app, &profile);
    sync_selected_outputs_for_wallpaper(app);
    if let Err(error) = persist_wallpaper_profiles(app) {
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
    match config::persist_playlists_profiles_and_outputs(
        &app.config_path,
        &app.launch_settings.playlists,
        &app.launch_settings.profiles,
        &app.launch_settings.outputs,
    ) {
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

fn persist_playlist_changes_and_reload(app: &mut App) -> bool {
    if !persist_playlist_changes(app) {
        return false;
    }
    if runtime::daemon_is_running() {
        if let Err(error) = reload_running_config(app) {
            set_playlist_error(
                app,
                format!("playlist was saved but the running daemon could not reload it: {error}"),
            );
            return false;
        }
    }
    true
}

fn reload_running_config(app: &mut App) -> Result<(), String> {
    if !runtime::daemon_is_running() {
        return Ok(());
    }
    if !multi_output_mode(app) {
        return runtime::try_switch(&app.config_path)
            .then_some(())
            .ok_or_else(|| "switch-config command failed".to_string());
    }

    let forced_renderer_library =
        std::env::var_os(we_core::install_layout::RENDERER_LIBRARY_OVERRIDE_ENV).is_some();
    let must_restart_unowned = forced_renderer_library && app.runtime_child.is_none();
    let supports_multi_output = match runtime::fetch_status_sync()? {
        runtime::DaemonStatus::Running(status) => daemon_status_supports_multi_output(&status),
        runtime::DaemonStatus::NotRunning => return Ok(()),
        runtime::DaemonStatus::EmptyResponse => false,
    };
    if supports_multi_output && !must_restart_unowned && runtime::try_switch(&app.config_path) {
        return Ok(());
    }

    let child = runtime::restart(&app.config_path, &mut app.runtime_child)?;
    app.runtime_child = Some(child);
    Ok(())
}

fn persist_host_integration_settings(app: &mut App) -> bool {
    super::settings::sync(app);
    if let Err(error) = config::persist_integrations_and_rules(
        &app.config_path,
        &app.launch_settings.integrations,
        &app.launch_settings.rules,
    ) {
        app.runtime_status = RuntimeStatus::ConfigSaveFailed(error.clone());
        eprintln!("failed to save host integrations: {error}");
        return false;
    }
    if let Err(error) = reload_running_config(app) {
        app.runtime_status = RuntimeStatus::Unavailable(format!(
            "integrations were saved but the running daemon could not reload them: {error}"
        ));
        return false;
    }
    true
}

fn set_playlist_error(app: &mut App, error: String) {
    app.runtime_status = RuntimeStatus::PlaylistError(error);
}

fn persist_profile_changes(app: &mut App, reload_running_daemon: bool) -> bool {
    match config::persist_profiles_and_outputs(
        &app.config_path,
        &app.launch_settings.profiles,
        &app.launch_settings.outputs,
    ) {
        Ok(()) => {
            if reload_running_daemon {
                if let Err(error) = reload_running_config(app) {
                    set_profile_error(
                        app,
                        format!(
                            "profile was saved but the running daemon could not reload it: {error}"
                        ),
                    );
                    return false;
                }
            }
            app.runtime_status = RuntimeStatus::ProfileSaved;
            true
        }
        Err(error) => {
            app.runtime_status = RuntimeStatus::ConfigSaveFailed(error.clone());
            eprintln!("failed to save output profiles: {error}");
            false
        }
    }
}

fn set_profile_error(app: &mut App, error: String) {
    app.runtime_status = RuntimeStatus::ProfileError(error);
}

fn ensure_profile_selection(app: &mut App) {
    let selected_is_valid = app
        .profile_selected
        .as_deref()
        .is_some_and(|name| app.launch_settings.profiles.definitions.contains_key(name));
    if !selected_is_valid {
        app.profile_selected = app
            .launch_settings
            .profiles
            .active
            .clone()
            .filter(|name| app.launch_settings.profiles.definitions.contains_key(name))
            .or_else(|| app.launch_settings.profiles.definitions.keys().next().cloned());
    }
    sync_profile_inputs(app);
}

fn sync_profile_inputs(app: &mut App) {
    app.profile_name_input = app.profile_selected.clone().unwrap_or_default();
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

fn sync_selected_outputs_for_wallpaper(app: &mut App) {
    let Some(wallpaper_id) = app.selected_id.as_deref() else {
        return;
    };
    if app.launch_settings.outputs.is_empty() {
        app.selected_outputs = app.outputs.iter().cloned().collect();
        return;
    }
    app.selected_outputs = app
        .outputs
        .iter()
        .filter(|output| {
            app.launch_settings
                .outputs
                .get(*output)
                .and_then(|binding| binding.wallpaper_id.as_deref())
                == Some(wallpaper_id)
        })
        .cloned()
        .collect();
}

fn sync_selected_outputs_for_playlist(app: &mut App) {
    let Some(playlist_name) = app.playlist_selected.as_deref() else {
        app.selected_outputs.clear();
        return;
    };
    if app.launch_settings.outputs.is_empty() {
        app.selected_outputs = app.outputs.iter().cloned().collect();
        return;
    }
    app.selected_outputs = app
        .outputs
        .iter()
        .filter(|output| {
            app.launch_settings.outputs.get(*output).and_then(|binding| binding.playlist.as_deref())
                == Some(playlist_name)
        })
        .cloned()
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

    if multi_output_mode(app) {
        if app.outputs.is_empty() {
            set_playlist_error(app, "no connected displays are available".to_string());
            return Task::none();
        }
        if app.selected_outputs.is_empty() {
            set_playlist_error(app, "select at least one display for the playlist".to_string());
            return Task::none();
        }
        app.launch_settings.profiles.active = None;
        let available_outputs = app.outputs.clone();
        let selected_outputs = app.selected_outputs.clone();
        app.launch_settings.outputs.retain(|output, binding| {
            !available_outputs.contains(output)
                || binding.playlist.as_deref() != Some(name.as_str())
                || selected_outputs.contains(output)
        });
        for output in app.selected_outputs.clone() {
            app.launch_settings.outputs.insert(output, OutputBinding::playlist(name.clone()));
        }
        if let Err(error) = config::persist_playlists_profiles_and_outputs(
            &app.config_path,
            &app.launch_settings.playlists,
            &app.launch_settings.profiles,
            &app.launch_settings.outputs,
        ) {
            app.runtime_status = RuntimeStatus::ConfigSaveFailed(error);
            return Task::none();
        }
        app.runtime_shutdown = false;
        return start_or_reconfigure_multi_output(app);
    }

    app.launch_settings.playlists.active = Some(name.clone());
    if !persist_playlist_changes(app) {
        return Task::none();
    }
    app.runtime_shutdown = false;
    if let Err(error) = runtime::reap(&mut app.runtime_child) {
        eprintln!("failed to query daemon child status: {error}");
    }

    if runtime::daemon_is_running() {
        if runtime::try_switch(&app.config_path) && runtime::play_playlist(&name) {
            app.playback_running = true;
            app.playback_paused = false;
            app.runtime_playlist_active = Some(name);
            return Task::perform(runtime::fetch_status(), Message::StatusLoaded);
        }

        return match runtime::restart(&app.config_path, &mut app.runtime_child) {
            Ok(child) => {
                app.runtime_child = Some(child);
                app.playback_running = true;
                app.playback_paused = false;
                app.runtime_playlist_active = Some(name);
                app.runtime_status = RuntimeStatus::StartedDaemon;
                Task::perform(runtime::fetch_status(), Message::StatusLoaded)
            }
            Err(error) => {
                app.runtime_status = RuntimeStatus::StartFailed(error);
                Task::none()
            }
        };
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
    let output_bound = app
        .launch_settings
        .outputs
        .values()
        .any(|binding| binding.playlist.as_deref() == Some(playlist_name));
    if output_bound {
        match runtime::fetch_status_sync()? {
            runtime::DaemonStatus::NotRunning => return Ok(false),
            runtime::DaemonStatus::EmptyResponse => {
                return Err("cannot determine output playlist state from an empty daemon status"
                    .to_string())
            }
            runtime::DaemonStatus::Running(status) => {
                if !daemon_status_supports_multi_output(&status) {
                    return Ok(true);
                }
                return Ok(parse_output_runtime_states(&status)
                    .values()
                    .any(|runtime| runtime.playlist_active.as_deref() == Some(playlist_name)));
            }
        }
    }
    global_playlist_is_running(app, playlist_name)
}

fn global_playlist_is_running(app: &App, playlist_name: &str) -> Result<bool, String> {
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
    if multi_output_mode(app) {
        let Some(playlist_name) = app.playlist_selected.clone() else {
            set_playlist_error(app, "select a playlist first".to_string());
            return Task::none();
        };
        let targets = app
            .selected_outputs
            .iter()
            .filter(|output| {
                app.launch_settings
                    .outputs
                    .get(*output)
                    .and_then(|binding| binding.playlist.as_deref())
                    == Some(playlist_name.as_str())
            })
            .cloned()
            .collect::<Vec<_>>();
        if targets.is_empty() {
            set_playlist_error(
                app,
                format!("no selected display is bound to playlist '{playlist_name}'"),
            );
            return Task::none();
        }
        let failed = targets
            .iter()
            .filter(|output| !runtime::send_output_playlist_action(output, action))
            .cloned()
            .collect::<Vec<_>>();
        if action == "stop" {
            match load_launch_settings(&app.config_path) {
                Ok(settings) => {
                    app.launch_settings.outputs = settings.outputs;
                    app.launch_settings.profiles = settings.profiles;
                    sync_selected_outputs_for_playlist(app);
                }
                Err(error) => {
                    set_playlist_error(
                        app,
                        format!(
                            "playlist stopped but persisted output bindings could not be reloaded: {error}"
                        ),
                    );
                    return Task::none();
                }
            }
        }
        if !failed.is_empty() {
            set_playlist_error(
                app,
                format!("failed to send playlist {action} to {}", failed.join(", ")),
            );
            return Task::none();
        }
        return Task::perform(runtime::fetch_status(), Message::StatusLoaded);
    }

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

    let multi_output = multi_output_mode(app);
    if !multi_output {
        if app.runtime_playlist_active.is_some() {
            let _ = runtime::send_playlist_action("stop");
        }
        app.launch_settings.playlists.active = None;
        app.runtime_playlist_active = None;
        app.runtime_playlist_index = None;
    }

    if let Err(error) = bind_selected_wallpaper_outputs(app) {
        app.runtime_status = RuntimeStatus::ConfigSaveFailed(error);
        return Task::none();
    }

    if let Err(error) = persist_playback_config(app) {
        app.runtime_status = RuntimeStatus::ConfigSaveFailed(error.clone());
        eprintln!("failed to save config: {error}");
        return Task::none();
    }

    if multi_output {
        return start_or_reconfigure_multi_output(app);
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

fn bind_selected_wallpaper_outputs(app: &mut App) -> Result<(), String> {
    if !multi_output_mode(app) {
        return Ok(());
    }
    if app.outputs.is_empty() {
        return Err("no connected displays are available".to_string());
    }
    if app.selected_outputs.is_empty() {
        return Err("select at least one display before applying the wallpaper".to_string());
    }
    app.launch_settings.profiles.active = None;
    let selected_id =
        app.selected_id.clone().ok_or_else(|| "select a wallpaper first".to_string())?;
    let entry = app
        .entries
        .iter()
        .find(|entry| entry.id == selected_id)
        .ok_or_else(|| "selected wallpaper is no longer in the library".to_string())?;
    let source =
        entry.project_json.parent().unwrap_or(&entry.project_json).to_string_lossy().into_owned();
    let available_outputs = app.outputs.clone();
    let selected_outputs = app.selected_outputs.clone();
    app.launch_settings.outputs.retain(|output, binding| {
        !available_outputs.contains(output)
            || binding.wallpaper_id.as_deref() != Some(selected_id.as_str())
            || selected_outputs.contains(output)
    });
    for output in app.selected_outputs.clone() {
        app.launch_settings
            .outputs
            .insert(output, OutputBinding::wallpaper(entry.id.clone(), source.clone()));
    }
    Ok(())
}

fn start_or_reconfigure_multi_output(app: &mut App) -> Task<Message> {
    if let Err(error) = runtime::reap(&mut app.runtime_child) {
        eprintln!("failed to query daemon child status: {error}");
    }
    if runtime::daemon_is_running() {
        let forced_renderer_library =
            std::env::var_os(we_core::install_layout::RENDERER_LIBRARY_OVERRIDE_ENV).is_some();
        let must_restart_unowned = forced_renderer_library && app.runtime_child.is_none();
        let supports_multi_output = match runtime::fetch_status_sync() {
            Ok(runtime::DaemonStatus::Running(status)) => {
                daemon_status_supports_multi_output(&status)
            }
            _ => false,
        };
        if supports_multi_output && !must_restart_unowned && runtime::try_switch(&app.config_path) {
            app.playback_running = true;
            app.playback_paused = false;
            app.runtime_status = RuntimeStatus::SwitchedDaemon;
            return Task::perform(runtime::fetch_status(), Message::StatusLoaded);
        }
        return match runtime::restart(&app.config_path, &mut app.runtime_child) {
            Ok(child) => {
                app.runtime_child = Some(child);
                app.playback_running = true;
                app.playback_paused = false;
                app.runtime_status = RuntimeStatus::StartedDaemon;
                Task::perform(runtime::fetch_status(), Message::StatusLoaded)
            }
            Err(error) => {
                app.runtime_status = RuntimeStatus::StartFailed(error);
                Task::none()
            }
        };
    }
    match runtime::start(&app.config_path) {
        Ok(child) => {
            app.runtime_child = Some(child);
            app.playback_running = true;
            app.playback_paused = false;
            app.runtime_status = RuntimeStatus::StartedDaemon;
            Task::perform(runtime::fetch_status(), Message::StatusLoaded)
        }
        Err(error) => {
            app.runtime_status = RuntimeStatus::StartFailed(error.to_string());
            Task::none()
        }
    }
}

fn multi_output_mode(app: &App) -> bool {
    !app.outputs.is_empty() || !app.launch_settings.outputs.is_empty()
}

fn daemon_status_supports_multi_output(status: &str) -> bool {
    status_section_value(status, "orchestrator", "multi_output") == Some("true")
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

fn parse_output_runtime_states(
    raw: &str,
) -> std::collections::BTreeMap<String, OutputRuntimeState> {
    let Ok(document) = toml::from_str::<toml::Value>(raw) else {
        return Default::default();
    };
    let mut outputs = std::collections::BTreeMap::new();

    if let Some(runtime) = document.get("runtime").and_then(toml::Value::as_table) {
        if let Some(output_name) = runtime.get("output_name").and_then(toml::Value::as_str) {
            if !output_name.is_empty() {
                outputs.insert(output_name.to_string(), output_runtime_state(runtime));
            }
        }
    }

    if let Some(output_runtime) = document.get("output_runtime").and_then(toml::Value::as_table) {
        for (output_name, value) in output_runtime {
            let Some(runtime) = value.get("runtime").and_then(toml::Value::as_table) else {
                continue;
            };
            outputs.insert(output_name.clone(), output_runtime_state(runtime));
        }
    }
    outputs
}

fn output_runtime_state(runtime: &toml::value::Table) -> OutputRuntimeState {
    let source =
        runtime.get("source").and_then(toml::Value::as_str).unwrap_or_default().to_string();
    let playlist_active = runtime
        .get("playlist_active")
        .and_then(toml::Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let playlist_index = runtime
        .get("playlist_index")
        .and_then(toml::Value::as_integer)
        .and_then(|index| usize::try_from(index).ok());
    OutputRuntimeState { source, playlist_active, playlist_index }
}

#[cfg(test)]
mod tests {
    use super::{
        daemon_status_supports_multi_output, effective_playback_start, parse_output_runtime_states,
        playlist_stop_can_be_persisted, status_section_value, PlaybackStart,
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

    #[test]
    fn output_runtime_status_parser_supports_legacy_single_output_status() {
        let status = r#"
[runtime]
output_name = "DP-1"
source = "/wallpapers/one"
playlist_active = "Focus"
playlist_index = 2
"#;
        let outputs = parse_output_runtime_states(status);
        let dp = outputs.get("DP-1").expect("DP-1 runtime");
        assert_eq!(dp.source, "/wallpapers/one");
        assert_eq!(dp.playlist_active.as_deref(), Some("Focus"));
        assert_eq!(dp.playlist_index, Some(2));
    }

    #[test]
    fn output_runtime_status_parser_keeps_multiple_outputs_independent() {
        let status = r#"
[output_runtime."DP-1".runtime]
output_name = "DP-1"
source = "/wallpapers/one"
playlist_active = "Focus"
playlist_index = 1

[output_runtime."HDMI-A-1".runtime]
output_name = "HDMI-A-1"
source = "/wallpapers/two"
playlist_active = "Ambient"
playlist_index = 4
"#;
        let outputs = parse_output_runtime_states(status);
        assert_eq!(outputs.len(), 2);
        assert_eq!(outputs["DP-1"].playlist_active.as_deref(), Some("Focus"));
        assert_eq!(outputs["DP-1"].playlist_index, Some(1));
        assert_eq!(outputs["HDMI-A-1"].source, "/wallpapers/two");
        assert_eq!(outputs["HDMI-A-1"].playlist_index, Some(4));
    }

    #[test]
    fn output_runtime_status_parser_treats_stopped_playlist_as_inactive() {
        let status = r#"
[output_runtime."DP-1".runtime]
output_name = "DP-1"
source = "/wallpapers/one"
playlist_active = ""
playlist_index = -1
"#;
        let outputs = parse_output_runtime_states(status);
        let dp = outputs.get("DP-1").expect("DP-1 runtime");
        assert_eq!(dp.playlist_active, None);
        assert_eq!(dp.playlist_index, None);
    }

    #[test]
    fn multi_output_capability_requires_explicit_orchestrator_flag() {
        assert!(daemon_status_supports_multi_output(
            "[orchestrator]\nphase = \"running\"\nmulti_output = true\n"
        ));
        assert!(!daemon_status_supports_multi_output("[orchestrator]\nphase = \"running\"\n"));
    }
}
