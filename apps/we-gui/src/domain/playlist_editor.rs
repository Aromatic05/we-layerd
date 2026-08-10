use we_core::{
    playlist::{Playlist, PlaylistConfig, PlaylistItem, PlaylistMode, MIN_PLAYLIST_DURATION_MS},
    wallpaper::{WallpaperEntry, WallpaperType},
};

pub(crate) const MIGRATED_SHUFFLE_NAME: &str = "Migrated shuffle";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MoveDirection {
    Up,
    Down,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LegacyShuffleMigration {
    pub enabled: bool,
    pub interval_ms: u32,
    pub include_video: bool,
    pub include_scene: bool,
    pub include_web: bool,
}

pub(crate) fn create_playlist(config: &mut PlaylistConfig, name: &str) -> Result<(), String> {
    let name = normalized_name(name)?;
    if config.definitions.contains_key(name) {
        return Err(format!("playlist '{name}' already exists"));
    }
    config.definitions.insert(name.to_string(), Playlist::default());
    Ok(())
}

pub(crate) fn rename_playlist(
    config: &mut PlaylistConfig,
    current_name: &str,
    new_name: &str,
) -> Result<(), String> {
    let new_name = normalized_name(new_name)?;
    if current_name == new_name {
        return Ok(());
    }
    if config.definitions.contains_key(new_name) {
        return Err(format!("playlist '{new_name}' already exists"));
    }
    let playlist = config
        .definitions
        .remove(current_name)
        .ok_or_else(|| format!("playlist '{current_name}' does not exist"))?;
    config.definitions.insert(new_name.to_string(), playlist);
    if config.active.as_deref() == Some(current_name) {
        config.active = Some(new_name.to_string());
    }
    Ok(())
}

pub(crate) fn delete_playlist(config: &mut PlaylistConfig, name: &str) -> Result<(), String> {
    if config.definitions.remove(name).is_none() {
        return Err(format!("playlist '{name}' does not exist"));
    }
    if config.active.as_deref() == Some(name) {
        config.active = None;
    }
    Ok(())
}

pub(crate) fn add_wallpaper(
    config: &mut PlaylistConfig,
    playlist_name: &str,
    entry: &WallpaperEntry,
) -> Result<(), String> {
    let playlist = playlist_mut(config, playlist_name)?;
    let source =
        entry.project_json.parent().unwrap_or(&entry.project_json).to_string_lossy().into_owned();
    playlist.items.push(PlaylistItem { wallpaper_id: entry.id.clone(), source, duration_ms: None });
    Ok(())
}

pub(crate) fn remove_entry(
    config: &mut PlaylistConfig,
    playlist_name: &str,
    index: usize,
) -> Result<(), String> {
    let playlist = playlist_mut(config, playlist_name)?;
    if index >= playlist.items.len() {
        return Err(format!("playlist entry {index} does not exist"));
    }
    playlist.items.remove(index);
    Ok(())
}

pub(crate) fn move_entry(
    config: &mut PlaylistConfig,
    playlist_name: &str,
    index: usize,
    direction: MoveDirection,
) -> Result<usize, String> {
    let playlist = playlist_mut(config, playlist_name)?;
    if index >= playlist.items.len() {
        return Err(format!("playlist entry {index} does not exist"));
    }
    let target = match direction {
        MoveDirection::Up => index.checked_sub(1),
        MoveDirection::Down => index.checked_add(1).filter(|target| *target < playlist.items.len()),
    }
    .ok_or_else(|| "playlist entry is already at the boundary".to_string())?;
    playlist.items.swap(index, target);
    Ok(target)
}

pub(crate) fn set_mode(
    config: &mut PlaylistConfig,
    playlist_name: &str,
    mode: PlaylistMode,
) -> Result<(), String> {
    playlist_mut(config, playlist_name)?.mode = mode;
    Ok(())
}

pub(crate) fn set_default_duration_ms(
    config: &mut PlaylistConfig,
    playlist_name: &str,
    duration_ms: u64,
) -> Result<(), String> {
    if duration_ms < MIN_PLAYLIST_DURATION_MS {
        return Err(format!("playlist duration must be at least {MIN_PLAYLIST_DURATION_MS} ms"));
    }
    playlist_mut(config, playlist_name)?.default_duration_ms = duration_ms;
    Ok(())
}

pub(crate) fn set_entry_duration_ms(
    config: &mut PlaylistConfig,
    playlist_name: &str,
    index: usize,
    duration_ms: Option<u64>,
) -> Result<(), String> {
    if duration_ms.is_some_and(|duration| duration < MIN_PLAYLIST_DURATION_MS) {
        return Err(format!("playlist duration must be at least {MIN_PLAYLIST_DURATION_MS} ms"));
    }
    let item = playlist_mut(config, playlist_name)?
        .items
        .get_mut(index)
        .ok_or_else(|| format!("playlist entry {index} does not exist"))?;
    item.duration_ms = duration_ms;
    Ok(())
}

pub(crate) fn migrate_legacy_shuffle(
    config: &mut PlaylistConfig,
    entries: &[WallpaperEntry],
    legacy: LegacyShuffleMigration,
) -> Result<bool, String> {
    if !config.definitions.is_empty() || !legacy.enabled {
        return Ok(false);
    }

    let items = entries
        .iter()
        .filter(|entry| match entry.ty {
            WallpaperType::Video => legacy.include_video,
            WallpaperType::Scene => legacy.include_scene,
            WallpaperType::Web => legacy.include_web,
            WallpaperType::Unknown => false,
        })
        .map(|entry| PlaylistItem {
            wallpaper_id: entry.id.clone(),
            source: entry
                .project_json
                .parent()
                .unwrap_or(&entry.project_json)
                .to_string_lossy()
                .into_owned(),
            duration_ms: None,
        })
        .collect::<Vec<_>>();

    if items.is_empty() {
        return Err("legacy shuffle is enabled but no matching wallpapers were found".to_string());
    }

    config.definitions.insert(
        MIGRATED_SHUFFLE_NAME.to_string(),
        Playlist {
            mode: PlaylistMode::Shuffle,
            default_duration_ms: u64::from(legacy.interval_ms).max(MIN_PLAYLIST_DURATION_MS),
            items,
        },
    );
    config.active = Some(MIGRATED_SHUFFLE_NAME.to_string());
    Ok(true)
}

fn playlist_mut<'a>(
    config: &'a mut PlaylistConfig,
    name: &str,
) -> Result<&'a mut Playlist, String> {
    config.definitions.get_mut(name).ok_or_else(|| format!("playlist '{name}' does not exist"))
}

fn normalized_name(name: &str) -> Result<&str, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("playlist name cannot be empty".to_string());
    }
    Ok(name)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use we_core::{
        playlist::{PlaylistConfig, PlaylistMode},
        wallpaper::{WallpaperEntry, WallpaperType},
    };

    use super::{
        add_wallpaper, create_playlist, delete_playlist, migrate_legacy_shuffle, move_entry,
        rename_playlist, set_default_duration_ms, set_entry_duration_ms, set_mode,
        LegacyShuffleMigration, MoveDirection,
    };

    fn wallpaper(id: &str, ty: WallpaperType) -> WallpaperEntry {
        WallpaperEntry {
            id: id.to_string(),
            project_json: PathBuf::from(format!("/workshop/{id}/project.json")),
            title: format!("Wallpaper {id}"),
            ty,
            preview: None,
            source_file: None,
        }
    }

    #[test]
    fn playlist_crud_preserves_duplicates_and_active_identity() {
        let mut config = PlaylistConfig::default();
        create_playlist(&mut config, "Focus").expect("create playlist");
        config.active = Some("Focus".to_string());

        let entry = wallpaper("42", WallpaperType::Scene);
        add_wallpaper(&mut config, "Focus", &entry).expect("first add");
        add_wallpaper(&mut config, "Focus", &entry).expect("duplicate add");

        let playlist = config.definitions.get("Focus").expect("playlist exists");
        assert_eq!(playlist.items.len(), 2);
        assert_eq!(playlist.items[0].wallpaper_id, "42");
        assert_eq!(playlist.items[1].wallpaper_id, "42");

        rename_playlist(&mut config, "Focus", "Deep Work").expect("rename playlist");
        assert!(!config.definitions.contains_key("Focus"));
        assert_eq!(config.active.as_deref(), Some("Deep Work"));
        assert_eq!(config.definitions["Deep Work"].items.len(), 2);

        delete_playlist(&mut config, "Deep Work").expect("delete playlist");
        assert!(config.definitions.is_empty());
        assert_eq!(config.active, None);
    }

    #[test]
    fn playlist_edits_change_order_mode_and_duration_without_rebuilding_entries() {
        let mut config = PlaylistConfig::default();
        create_playlist(&mut config, "Daily").expect("create playlist");
        for id in ["a", "b", "c"] {
            add_wallpaper(&mut config, "Daily", &wallpaper(id, WallpaperType::Video))
                .expect("add wallpaper");
        }

        set_mode(&mut config, "Daily", PlaylistMode::Repeat).expect("set mode");
        set_default_duration_ms(&mut config, "Daily", 90_000).expect("set default duration");
        set_entry_duration_ms(&mut config, "Daily", 1, Some(12_000)).expect("set entry duration");
        move_entry(&mut config, "Daily", 2, MoveDirection::Up).expect("move entry");

        let playlist = &config.definitions["Daily"];
        assert_eq!(playlist.mode, PlaylistMode::Repeat);
        assert_eq!(playlist.default_duration_ms, 90_000);
        assert_eq!(
            playlist.items.iter().map(|item| item.wallpaper_id.as_str()).collect::<Vec<_>>(),
            vec!["a", "c", "b"]
        );
        assert_eq!(playlist.items[2].duration_ms, Some(12_000));
    }

    #[test]
    fn legacy_shuffle_migrates_once_using_previous_source_filters() {
        let entries = vec![
            wallpaper("video", WallpaperType::Video),
            wallpaper("scene", WallpaperType::Scene),
            wallpaper("web", WallpaperType::Web),
            wallpaper("unknown", WallpaperType::Unknown),
        ];
        let legacy = LegacyShuffleMigration {
            enabled: true,
            interval_ms: 300_000,
            include_video: true,
            include_scene: false,
            include_web: true,
        };
        let mut config = PlaylistConfig::default();

        let migrated = migrate_legacy_shuffle(&mut config, &entries, legacy)
            .expect("legacy shuffle should migrate");
        assert!(migrated);
        assert_eq!(config.active.as_deref(), Some("Migrated shuffle"));
        let playlist = &config.definitions["Migrated shuffle"];
        assert_eq!(playlist.mode, PlaylistMode::Shuffle);
        assert_eq!(playlist.default_duration_ms, 300_000);
        assert_eq!(
            playlist.items.iter().map(|item| item.wallpaper_id.as_str()).collect::<Vec<_>>(),
            vec!["video", "web"]
        );

        let migrated_again = migrate_legacy_shuffle(&mut config, &entries, legacy)
            .expect("second migration should be harmless");
        assert!(!migrated_again);
        assert_eq!(config.definitions.len(), 1);
        assert_eq!(config.definitions["Migrated shuffle"].items.len(), 2);
    }
}
