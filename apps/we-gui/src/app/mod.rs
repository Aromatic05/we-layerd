mod detail_update;
mod init;
mod settings;
mod signal;
mod state;
mod subscription;
mod update;
mod view;
pub(crate) use state::{App, Message};
pub fn run() -> iced::Result {
    signal::install().expect("install GUI shutdown signal handler");
    iced::daemon(init::initialize, update::update, view::daemon_view)
        .title("we-gui")
        .theme(|app: &App, _window| app.theme.clone())
        .subscription(subscription::subscription)
        .run()
}

pub fn was_interrupted() -> bool {
    signal::was_interrupted()
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    #[cfg(unix)]
    use std::{
        fs,
        os::unix::fs::PermissionsExt,
        sync::Mutex,
        time::{SystemTime, UNIX_EPOCH},
    };

    use iced::{widget::pane_grid, Theme};

    use super::{settings::sync_from_ui, update, App, Message};
    use crate::{
        domain::{
            i18n::Language,
            playlist_editor::LegacyShuffleMigration,
            runtime_status::RuntimeStatus,
            settings::{ScaleModeOption, UiSettings},
            ui_state::Pane,
        },
        ui::sidebar::detail::DetailTab,
    };
    use we_core::{
        config::{LaunchSettings, ScaleMode},
        wallpaper::properties::UserPropertySchema,
    };

    #[cfg(unix)]
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn test_ui_settings() -> UiSettings {
        UiSettings {
            assets_path: "/opt/wallpaper_engine".to_string(),
            workshop_path: "/tmp/workshop/content/431960".to_string(),
            renderer_library_path: "/opt/libwallpaper-engine-renderer.so".to_string(),
            renderer_cache_path: "~/.cache/we-layerd/test".to_string(),
            prefer_dmabuf: false,
            allow_shm_fallback: true,
            interactive: false,
            force_scene_audio_loop: true,
            fps_limit: "144".to_string(),
            show_fps: true,
            scale_mode: ScaleModeOption::Stretch,
            media_integration: true,
            audio_spectrum: false,
            audio_source: "@DEFAULT_MONITOR@".to_string(),
            rule_focused: we_core::config::RuntimeRuleAction::Keep,
            rule_maximized: we_core::config::RuntimeRuleAction::Mute,
            rule_fullscreen: we_core::config::RuntimeRuleAction::Pause,
        }
    }

    fn legacy_shuffle() -> LegacyShuffleMigration {
        LegacyShuffleMigration {
            enabled: false,
            interval_ms: 1_800_000,
            include_video: true,
            include_scene: true,
            include_web: true,
        }
    }

    #[test]
    fn sync_launch_settings_copies_workshop_path_from_ui() {
        let ui_settings = test_ui_settings();
        let mut launch_settings = LaunchSettings::default();

        sync_from_ui(&ui_settings, &mut launch_settings);

        assert_eq!(launch_settings.workshop_path, "/tmp/workshop/content/431960");
        assert_eq!(launch_settings.assets_path, "/opt/wallpaper_engine");
        assert_eq!(launch_settings.renderer_library_path, "/opt/libwallpaper-engine-renderer.so");
        assert_eq!(launch_settings.renderer_cache_path, "~/.cache/we-layerd/test");
        assert!(!launch_settings.prefer_dmabuf);
        assert!(launch_settings.allow_shm_fallback);
        assert!(!launch_settings.interactive);
        assert!(launch_settings.force_scene_audio_loop);
        assert_eq!(launch_settings.scale_mode, ScaleMode::Stretch);
    }

    #[test]
    fn language_switch_updates_headless_state_and_queues_independent_save() {
        let suffix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock")
            .as_nanos();
        let temp = std::env::temp_dir().join(format!("we-gui-language-{suffix}"));
        let preferences_path = temp.join("gui.toml");
        let mut app = App {
            entries: Vec::new(),
            selected_id: None,
            selected_schema: UserPropertySchema { entries: Vec::new() },
            resolution_width: String::new(),
            resolution_height: String::new(),
            config_path: temp.join("config.toml"),
            runtime_child: None,
            viewport_width: 1280.0,
            layerd_available: true,
            launch_settings: LaunchSettings::default(),
            ui_settings: test_ui_settings(),
            show_settings: true,
            sidebar: None,
            detail_tab: DetailTab::Actions,
            playback_paused: false,
            playback_running: false,
            search_query: String::new(),
            type_filter: None,
            filtered_entry_indices: Vec::new(),
            panes: pane_grid::State::with_configuration(pane_grid::Configuration::Pane(
                Pane::Library,
            )),
            animated_previews: HashMap::new(),
            gif_preview_desired: Default::default(),
            gif_preview_loading: Default::default(),
            gif_preview_failed: Default::default(),
            library_scroll_y: 0.0,
            library_viewport_width: 860.0,
            library_viewport_height: 720.0,
            tray: None,
            main_window_id: None,
            theme: Theme::Dark,
            runtime_shutdown: true,
            outputs: Vec::new(),
            selected_outputs: Default::default(),
            running_source: None,
            language: Language::English,
            preferences_path: Some(preferences_path.clone()),
            runtime_status: RuntimeStatus::DaemonNotRunning,
            preferences_generation: 0,
            playlist_selected: None,
            playlist_new_name_input: String::new(),
            playlist_name_input: String::new(),
            playlist_default_duration_input: String::new(),
            playlist_entry_duration_inputs: Vec::new(),
            runtime_playlist_active: None,
            runtime_playlist_index: None,
            runtime_outputs: Default::default(),
            profile_selected: None,
            profile_new_name_input: String::new(),
            profile_name_input: String::new(),
            legacy_shuffle: legacy_shuffle(),
            playlist_migration_completed: true,
        };

        let _task =
            update::update(&mut app, Message::LanguageSelected(Language::SimplifiedChinese));

        assert_eq!(app.language, Language::SimplifiedChinese);
        assert_eq!(app.preferences_generation, 1);
        assert!(!app.config_path.exists());

        let _newest_task = update::update(&mut app, Message::LanguageSelected(Language::English));
        let _retry_task = update::update(
            &mut app,
            Message::PreferencesSaved { generation: 1, result: Err("stale failure".to_string()) },
        );
        assert_eq!(app.language, Language::English);
        assert_eq!(app.preferences_generation, 2);
        assert_eq!(app.runtime_status, RuntimeStatus::DaemonNotRunning);

        let _completed_task = update::update(
            &mut app,
            Message::PreferencesSaved { generation: 2, result: Err("disk full".to_string()) },
        );
        assert_eq!(
            app.runtime_status,
            RuntimeStatus::PreferencesSaveFailed("disk full".to_string())
        );
    }

    #[cfg(unix)]
    #[test]
    fn dropping_gui_stops_layerd() {
        let _lock = ENV_LOCK.lock().expect("environment lock");
        let suffix = SystemTime::now().duration_since(UNIX_EPOCH).expect("system clock").as_nanos();
        let temp = std::env::temp_dir().join(format!("we-gui-exit-{suffix}"));
        let command = temp.join("we-layerd");
        let log = temp.join("commands.log");
        fs::create_dir_all(&temp).expect("create test directory");
        fs::write(&command, "#!/bin/sh\nprintf '%s\n' \"$*\" >> \"$WE_GUI_TEST_LOG\"\n")
            .expect("write fake we-layerd");
        let mut permissions = fs::metadata(&command).expect("fake command metadata").permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&command, permissions).expect("make fake command executable");

        let old_path = std::env::var_os("PATH");
        let old_log = std::env::var_os("WE_GUI_TEST_LOG");
        std::env::set_var("PATH", &temp);
        std::env::set_var("WE_GUI_TEST_LOG", &log);

        let app = App {
            entries: Vec::new(),
            selected_id: None,
            selected_schema: UserPropertySchema { entries: Vec::new() },
            resolution_width: String::new(),
            resolution_height: String::new(),
            config_path: temp.join("config.toml"),
            runtime_child: None,
            viewport_width: 1280.0,
            layerd_available: true,
            launch_settings: LaunchSettings::default(),
            ui_settings: test_ui_settings(),
            show_settings: false,
            sidebar: None,
            detail_tab: DetailTab::Actions,
            playback_paused: false,
            playback_running: true,
            search_query: String::new(),
            type_filter: None,
            filtered_entry_indices: Vec::new(),
            panes: pane_grid::State::with_configuration(pane_grid::Configuration::Pane(
                Pane::Library,
            )),
            animated_previews: HashMap::new(),
            gif_preview_desired: Default::default(),
            gif_preview_loading: Default::default(),
            gif_preview_failed: Default::default(),
            library_scroll_y: 0.0,
            library_viewport_width: 860.0,
            library_viewport_height: 720.0,
            tray: None,
            main_window_id: None,
            theme: Theme::Dark,
            runtime_shutdown: false,
            outputs: Vec::new(),
            selected_outputs: Default::default(),
            running_source: None,
            language: Language::English,
            preferences_path: None,
            runtime_status: RuntimeStatus::DaemonNotRunning,
            preferences_generation: 0,
            playlist_selected: None,
            playlist_new_name_input: String::new(),
            playlist_name_input: String::new(),
            playlist_default_duration_input: String::new(),
            playlist_entry_duration_inputs: Vec::new(),
            runtime_playlist_active: None,
            runtime_playlist_index: None,
            runtime_outputs: Default::default(),
            profile_selected: None,
            profile_new_name_input: String::new(),
            profile_name_input: String::new(),
            legacy_shuffle: legacy_shuffle(),
            playlist_migration_completed: true,
        };
        drop(app);

        match old_path {
            Some(path) => std::env::set_var("PATH", path),
            None => std::env::remove_var("PATH"),
        }
        match old_log {
            Some(value) => std::env::set_var("WE_GUI_TEST_LOG", value),
            None => std::env::remove_var("WE_GUI_TEST_LOG"),
        }

        let commands = fs::read_to_string(&log).expect("read fake command log");
        assert!(commands.lines().any(|line| line == "ctl stop"));
        fs::remove_dir_all(temp).expect("remove test directory");
    }

    #[cfg(unix)]
    #[test]
    fn runtime_restart_stops_old_daemon_before_starting_new_one() {
        let _lock = ENV_LOCK.lock().expect("environment lock");
        let suffix = SystemTime::now().duration_since(UNIX_EPOCH).expect("system clock").as_nanos();
        let temp = std::env::temp_dir().join(format!("we-gui-restart-{suffix}"));
        let command = temp.join("we-layerd");
        let config = temp.join("config.toml");
        let log = temp.join("commands.log");
        fs::create_dir_all(&temp).expect("create test directory");
        fs::write(
            &command,
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$WE_GUI_TEST_LOG\"\nif [ \"$1 $2\" = \"ctl status\" ]; then exit 1; fi\nif [ \"$1\" = \"run\" ]; then /usr/bin/sleep 30; fi\n",
        )
        .expect("write fake we-layerd");
        let mut permissions = fs::metadata(&command).expect("fake command metadata").permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&command, permissions).expect("make fake command executable");

        let old_path = std::env::var_os("PATH");
        let old_log = std::env::var_os("WE_GUI_TEST_LOG");
        std::env::set_var("PATH", &temp);
        std::env::set_var("WE_GUI_TEST_LOG", &log);

        let mut owned_child = None;
        let mut replacement =
            crate::services::runtime::restart(&config, &mut owned_child).expect("restart daemon");
        std::thread::sleep(std::time::Duration::from_millis(50));
        let _ = replacement.kill();
        let _ = replacement.wait();

        match old_path {
            Some(path) => std::env::set_var("PATH", path),
            None => std::env::remove_var("PATH"),
        }
        match old_log {
            Some(value) => std::env::set_var("WE_GUI_TEST_LOG", value),
            None => std::env::remove_var("WE_GUI_TEST_LOG"),
        }

        let commands = fs::read_to_string(&log).expect("read fake command log");
        let lines = commands.lines().collect::<Vec<_>>();
        assert_eq!(lines[0], "ctl stop");
        assert_eq!(lines[1], "ctl status");
        assert_eq!(lines[2], format!("run --config {}", config.display()));
        fs::remove_dir_all(temp).expect("remove test directory");
    }
}
