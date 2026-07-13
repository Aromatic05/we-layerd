use we_core::config::{LaunchSettings, ScaleMode};

use crate::domain::settings::UiSettings;

use super::App;

pub(crate) fn sync_from_ui(ui_settings: &UiSettings, launch_settings: &mut LaunchSettings) {
    launch_settings.assets_path = ui_settings.assets_path.clone();
    launch_settings.workshop_path = ui_settings.workshop_path.clone();
    launch_settings.renderer_library_path = ui_settings.renderer_library_path.clone();
    launch_settings.renderer_cache_path = ui_settings.renderer_cache_path.clone();
    launch_settings.prefer_dmabuf = ui_settings.prefer_dmabuf;
    launch_settings.allow_shm_fallback = ui_settings.allow_shm_fallback;
    launch_settings.interactive = ui_settings.interactive;
    launch_settings.show_fps = ui_settings.show_fps;
    launch_settings.scale_mode = ScaleMode::from(ui_settings.scale_mode);

    if let Ok(v) = ui_settings.fps_limit.parse::<u32>() {
        launch_settings.fps_limit = v.clamp(1, 360);
    }
}

pub(crate) fn sync(app: &mut App) {
    sync_from_ui(&app.ui_settings, &mut app.launch_settings);
}

