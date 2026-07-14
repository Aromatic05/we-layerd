mod state;
mod init;
mod detail_update;
mod settings;
mod subscription;
mod update;
mod view;
pub(crate) use state::{App, Message};
pub fn run() -> iced::Result {
    iced::daemon(init::initialize, update::update, view::daemon_view)
        .title("we-gui")
        .theme(|app: &App, _window| app.theme.clone())
        .subscription(|_app| subscription::subscription())
        .run()
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    #[cfg(unix)]
    use std::{fs, os::unix::fs::PermissionsExt, sync::Mutex, time::{SystemTime, UNIX_EPOCH}};

    use iced::{widget::pane_grid, Theme};

    use super::{settings::sync_from_ui, App};
    use crate::{
        domain::{settings::{ScaleModeOption, UiSettings}, ui_state::Pane},
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
            assets_path: "/opt/wallpaper_engine/assets".to_string(),
            workshop_path: "/tmp/workshop/content/431960".to_string(),
            renderer_library_path: "/opt/libwallpaper-engine-renderer.so".to_string(),
            renderer_cache_path: "~/.cache/we-layerd/test".to_string(),
            prefer_dmabuf: false,
            allow_shm_fallback: true,
            interactive: false,
            fps_limit: "144".to_string(),
            show_fps: true,
            scale_mode: ScaleModeOption::Stretch,
            status_text: String::new(),
        }
    }

    #[test]
    fn sync_launch_settings_copies_workshop_path_from_ui() {
        let ui_settings = test_ui_settings();
        let mut launch_settings = LaunchSettings::default();

        sync_from_ui(&ui_settings, &mut launch_settings);

        assert_eq!(launch_settings.workshop_path, "/tmp/workshop/content/431960");
        assert_eq!(launch_settings.assets_path, "/opt/wallpaper_engine/assets");
        assert_eq!(launch_settings.renderer_library_path, "/opt/libwallpaper-engine-renderer.so");
        assert_eq!(launch_settings.renderer_cache_path, "~/.cache/we-layerd/test");
        assert!(!launch_settings.prefer_dmabuf);
        assert!(launch_settings.allow_shm_fallback);
        assert!(!launch_settings.interactive);
        assert_eq!(launch_settings.scale_mode, ScaleMode::Stretch);
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
            entries: Vec::new(), selected_id: None,
            selected_schema: UserPropertySchema { entries: Vec::new() },
            resolution_width: String::new(), resolution_height: String::new(),
            config_path: temp.join("config.toml"), runtime_child: None,
            viewport_width: 1280.0, layerd_available: true,
            launch_settings: LaunchSettings::default(), ui_settings: test_ui_settings(),
            show_settings: false, sidebar: None, detail_tab: DetailTab::Actions,
            playback_paused: false, playback_running: true,
            search_query: String::new(), type_filter: None,
            panes: pane_grid::State::with_configuration(pane_grid::Configuration::Pane(Pane::Library)),
            animated_previews: HashMap::new(), tray: None,
            main_window_id: None, theme: Theme::Dark, runtime_shutdown: false,
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
}
