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
    use super::settings::sync_from_ui;
    use crate::domain::settings::{ScaleModeOption, UiSettings};
    use we_core::config::{LaunchSettings, ScaleMode};

    #[test]
    fn sync_launch_settings_copies_workshop_path_from_ui() {
        let ui_settings = UiSettings {
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
        };
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
}
