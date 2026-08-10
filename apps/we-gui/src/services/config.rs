use std::path::Path;

use we_core::{
    config::{
        build_config_for_wallpaper, save_config, save_force_scene_audio_loop, save_playlists,
        LaunchSettings,
    },
    playlist::PlaylistConfig,
    wallpaper::WallpaperEntry,
};

pub(crate) fn persist_selected(
    config_path: &Path,
    launch_settings: &LaunchSettings,
    entry: &WallpaperEntry,
) -> Result<(), String> {
    let config = build_config_for_wallpaper(launch_settings, &entry.id, &entry.project_json)
        .map_err(|error| error.to_string())?;
    save_config(config_path, &config).map_err(|error| error.to_string())
}

pub(crate) fn persist_force_scene_audio_loop(
    config_path: &Path,
    enabled: bool,
) -> Result<(), String> {
    save_force_scene_audio_loop(config_path, enabled).map_err(|error| error.to_string())
}

pub(crate) fn persist_playlists(
    config_path: &Path,
    playlists: &PlaylistConfig,
) -> Result<(), String> {
    save_playlists(config_path, playlists).map_err(|error| error.to_string())
}
