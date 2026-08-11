use std::path::Path;

use we_core::{
    config::{
        build_config_for_wallpaper, save_config, save_force_scene_audio_loop,
        save_integrations_and_rules, save_playlists_profiles_and_outputs,
        save_profiles_and_outputs, save_wallpapers, save_wallpapers_playlists_profiles_and_outputs,
        IntegrationsConfig, LaunchSettings, OutputBinding, RuntimeRulesConfig,
    },
    playlist::PlaylistConfig,
    profile::ProfileConfig,
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

pub(crate) fn persist_integrations_and_rules(
    config_path: &Path,
    integrations: &IntegrationsConfig,
    rules: &RuntimeRulesConfig,
) -> Result<(), String> {
    save_integrations_and_rules(config_path, integrations, rules).map_err(|error| error.to_string())
}

pub(crate) fn persist_wallpapers(
    config_path: &Path,
    wallpapers: &std::collections::BTreeMap<
        String,
        we_core::wallpaper::settings::WallpaperSettings,
    >,
) -> Result<(), String> {
    save_wallpapers(config_path, wallpapers).map_err(|error| error.to_string())
}

pub(crate) fn persist_profiles_and_outputs(
    config_path: &Path,
    profiles: &ProfileConfig,
    outputs: &std::collections::BTreeMap<String, OutputBinding>,
) -> Result<(), String> {
    save_profiles_and_outputs(config_path, profiles, outputs).map_err(|error| error.to_string())
}

pub(crate) fn persist_playlists_profiles_and_outputs(
    config_path: &Path,
    playlists: &PlaylistConfig,
    profiles: &ProfileConfig,
    outputs: &std::collections::BTreeMap<String, OutputBinding>,
) -> Result<(), String> {
    save_playlists_profiles_and_outputs(config_path, playlists, profiles, outputs)
        .map_err(|error| error.to_string())
}

pub(crate) fn persist_wallpapers_playlists_profiles_and_outputs(
    config_path: &Path,
    wallpapers: &std::collections::BTreeMap<
        String,
        we_core::wallpaper::settings::WallpaperSettings,
    >,
    playlists: &PlaylistConfig,
    profiles: &ProfileConfig,
    outputs: &std::collections::BTreeMap<String, OutputBinding>,
) -> Result<(), String> {
    save_wallpapers_playlists_profiles_and_outputs(
        config_path,
        wallpapers,
        playlists,
        profiles,
        outputs,
    )
    .map_err(|error| error.to_string())
}
