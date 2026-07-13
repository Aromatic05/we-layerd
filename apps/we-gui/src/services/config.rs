use std::path::Path;

use we_core::{
    config::{build_config_for_wallpaper, save_config, LaunchSettings},
    wallpaper::WallpaperEntry,
};

pub(crate) fn persist_selected(
    config_path: &Path,
    launch_settings: &LaunchSettings,
    entry: &WallpaperEntry,
) {
    let config = build_config_for_wallpaper(launch_settings, &entry.id, &entry.project_json);
    let _ = save_config(config_path, &config);
}
