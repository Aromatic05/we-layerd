use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{config::OutputBinding, playlist::PlaylistConfig};

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct OutputProfile {
    #[serde(default)]
    pub outputs: BTreeMap<String, OutputBinding>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProfileConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active: Option<String>,
    #[serde(default)]
    pub definitions: BTreeMap<String, OutputProfile>,
}

pub fn snapshot_outputs(outputs: &BTreeMap<String, OutputBinding>) -> OutputProfile {
    OutputProfile { outputs: outputs.clone() }
}

pub fn create_profile(
    profiles: &mut ProfileConfig,
    name: &str,
    outputs: &BTreeMap<String, OutputBinding>,
) -> Result<(), String> {
    let name = normalized_name(name)?;
    if outputs.is_empty() {
        return Err("cannot save a multi-output profile without output bindings".to_string());
    }
    if profiles.definitions.contains_key(name) {
        return Err(format!("profile '{name}' already exists"));
    }
    profiles.definitions.insert(name.to_string(), snapshot_outputs(outputs));
    Ok(())
}

pub fn save_current_to_profile(
    profiles: &mut ProfileConfig,
    name: &str,
    outputs: &BTreeMap<String, OutputBinding>,
) -> Result<(), String> {
    let name = normalized_name(name)?;
    if outputs.is_empty() {
        return Err("cannot save a multi-output profile without output bindings".to_string());
    }
    let profile = profiles
        .definitions
        .get_mut(name)
        .ok_or_else(|| format!("profile '{name}' does not exist"))?;
    *profile = snapshot_outputs(outputs);
    Ok(())
}

pub fn rename_profile(profiles: &mut ProfileConfig, old: &str, new: &str) -> Result<(), String> {
    let new = normalized_name(new)?;
    if old == new {
        return Ok(());
    }
    if profiles.definitions.contains_key(new) {
        return Err(format!("profile '{new}' already exists"));
    }
    let profile = profiles
        .definitions
        .remove(old)
        .ok_or_else(|| format!("profile '{old}' does not exist"))?;
    profiles.definitions.insert(new.to_string(), profile);
    if profiles.active.as_deref() == Some(old) {
        profiles.active = Some(new.to_string());
    }
    Ok(())
}

pub fn delete_profile(profiles: &mut ProfileConfig, name: &str) -> Result<(), String> {
    if profiles.definitions.remove(name).is_none() {
        return Err(format!("profile '{name}' does not exist"));
    }
    if profiles.active.as_deref() == Some(name) {
        profiles.active = None;
    }
    Ok(())
}

pub fn apply_profile_to_outputs<F>(
    profiles: &ProfileConfig,
    name: &str,
    playlists: &PlaylistConfig,
    outputs: &mut BTreeMap<String, OutputBinding>,
    source_available: F,
) -> Result<(), String>
where
    F: Fn(&str) -> bool,
{
    let profile =
        profiles.definitions.get(name).ok_or_else(|| format!("profile '{name}' does not exist"))?;
    validate_profile(profile, playlists, source_available)?;
    *outputs = profile.outputs.clone();
    Ok(())
}

pub fn validate_profile<F>(
    profile: &OutputProfile,
    playlists: &PlaylistConfig,
    source_available: F,
) -> Result<(), String>
where
    F: Fn(&str) -> bool,
{
    if profile.outputs.is_empty() {
        return Err("profile does not contain any output bindings".to_string());
    }
    for (output, binding) in &profile.outputs {
        if binding.is_ambiguous() {
            return Err(format!(
                "profile output '{output}' cannot bind both a wallpaper and playlist"
            ));
        }
        if let Some(playlist_name) = binding.playlist.as_deref() {
            let playlist = playlists.definitions.get(playlist_name).ok_or_else(|| {
                format!("profile output '{output}' references missing playlist '{playlist_name}'")
            })?;
            if !playlist.items.iter().any(|item| source_available(&item.source)) {
                return Err(format!(
                    "profile output '{output}' references playlist '{playlist_name}' with no playable wallpaper"
                ));
            }
            continue;
        }
        match (binding.wallpaper_id.as_deref(), binding.source.as_deref()) {
            (Some(_), Some(source)) => {
                if !source_available(source) {
                    return Err(format!(
                        "profile output '{output}' references missing wallpaper source '{source}'"
                    ));
                }
            }
            (None, None) => {
                return Err(format!("profile output '{output}' has no wallpaper or playlist"));
            }
            _ => {
                return Err(format!(
                    "profile output '{output}' wallpaper binding requires wallpaper_id and source"
                ));
            }
        }
    }
    Ok(())
}

pub fn rename_playlist_references(profiles: &mut ProfileConfig, old: &str, new: &str) {
    for profile in profiles.definitions.values_mut() {
        for binding in profile.outputs.values_mut() {
            if binding.playlist.as_deref() == Some(old) {
                binding.playlist = Some(new.to_string());
            }
        }
    }
}

fn normalized_name(name: &str) -> Result<&str, String> {
    let name = name.trim();
    if name.is_empty() {
        Err("profile name must not be empty".to_string())
    } else {
        Ok(name)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::{
        config::OutputBinding,
        playlist::{Playlist, PlaylistConfig, PlaylistItem},
    };

    use super::{
        apply_profile_to_outputs, rename_playlist_references, snapshot_outputs, OutputProfile,
        ProfileConfig,
    };

    #[test]
    fn snapshot_preserves_the_complete_output_binding_map_including_disconnected_outputs() {
        let outputs = BTreeMap::from([
            ("DP-1".to_string(), OutputBinding::wallpaper("42", "/workshop/42")),
            ("HDMI-A-1".to_string(), OutputBinding::playlist("Ambient")),
        ]);

        let profile = snapshot_outputs(&outputs);

        assert_eq!(profile.outputs, outputs);
    }

    #[test]
    fn apply_is_atomic_when_a_referenced_playlist_is_missing() {
        let mut profiles = ProfileConfig::default();
        profiles.definitions.insert(
            "Desk".to_string(),
            OutputProfile {
                outputs: BTreeMap::from([("DP-1".to_string(), OutputBinding::playlist("Deleted"))]),
            },
        );
        let mut outputs =
            BTreeMap::from([("DP-1".to_string(), OutputBinding::wallpaper("42", "/workshop/42"))]);
        let before = outputs.clone();

        let error = apply_profile_to_outputs(
            &profiles,
            "Desk",
            &PlaylistConfig::default(),
            &mut outputs,
            |_| true,
        )
        .expect_err("missing playlist must reject the whole profile");

        assert!(error.contains("Deleted"));
        assert_eq!(outputs, before);
    }

    #[test]
    fn apply_is_atomic_when_a_referenced_playlist_has_no_playable_item() {
        let mut playlists = PlaylistConfig::default();
        playlists.definitions.insert(
            "EmptyHere".to_string(),
            Playlist {
                items: vec![PlaylistItem {
                    wallpaper_id: "missing".to_string(),
                    source: "/missing/wallpaper".to_string(),
                    duration_ms: None,
                }],
                ..Playlist::default()
            },
        );
        let mut profiles = ProfileConfig::default();
        profiles.definitions.insert(
            "Desk".to_string(),
            OutputProfile {
                outputs: BTreeMap::from([(
                    "DP-1".to_string(),
                    OutputBinding::playlist("EmptyHere"),
                )]),
            },
        );
        let mut outputs =
            BTreeMap::from([("DP-1".to_string(), OutputBinding::wallpaper("42", "/workshop/42"))]);
        let before = outputs.clone();

        apply_profile_to_outputs(&profiles, "Desk", &playlists, &mut outputs, |source| {
            source != "/missing/wallpaper"
        })
        .expect_err("playlist with no playable item must reject the whole profile");

        assert_eq!(outputs, before);
    }

    #[test]
    fn apply_is_atomic_when_a_wallpaper_source_is_missing() {
        let mut profiles = ProfileConfig::default();
        profiles.definitions.insert(
            "Desk".to_string(),
            OutputProfile {
                outputs: BTreeMap::from([(
                    "DP-1".to_string(),
                    OutputBinding::wallpaper("42", "/missing/42"),
                )]),
            },
        );
        let mut outputs =
            BTreeMap::from([("DP-1".to_string(), OutputBinding::wallpaper("7", "/workshop/7"))]);
        let before = outputs.clone();

        let error = apply_profile_to_outputs(
            &profiles,
            "Desk",
            &PlaylistConfig::default(),
            &mut outputs,
            |source| source != "/missing/42",
        )
        .expect_err("missing wallpaper source must reject the whole profile");

        assert!(error.contains("/missing/42"));
        assert_eq!(outputs, before);
    }

    #[test]
    fn applying_a_valid_profile_replaces_the_complete_output_set() {
        let mut playlists = PlaylistConfig::default();
        playlists.definitions.insert(
            "Ambient".to_string(),
            Playlist {
                items: vec![PlaylistItem {
                    wallpaper_id: "ambient".to_string(),
                    source: "/workshop/ambient".to_string(),
                    duration_ms: None,
                }],
                ..Playlist::default()
            },
        );
        let expected = BTreeMap::from([
            ("DP-1".to_string(), OutputBinding::wallpaper("42", "/workshop/42")),
            ("HDMI-A-1".to_string(), OutputBinding::playlist("Ambient")),
        ]);
        let mut profiles = ProfileConfig::default();
        profiles
            .definitions
            .insert("Desk".to_string(), OutputProfile { outputs: expected.clone() });
        let mut outputs = BTreeMap::from([(
            "eDP-1".to_string(),
            OutputBinding::wallpaper("old", "/workshop/old"),
        )]);

        apply_profile_to_outputs(&profiles, "Desk", &playlists, &mut outputs, |_| true)
            .expect("valid profile");

        assert_eq!(outputs, expected);
    }

    #[test]
    fn playlist_rename_updates_saved_profile_references_without_touching_other_bindings() {
        let wallpaper = OutputBinding::wallpaper("42", "/workshop/42");
        let mut profiles = ProfileConfig::default();
        profiles.definitions.insert(
            "Desk".to_string(),
            OutputProfile {
                outputs: BTreeMap::from([
                    ("DP-1".to_string(), OutputBinding::playlist("Old")),
                    ("HDMI-A-1".to_string(), wallpaper.clone()),
                ]),
            },
        );

        rename_playlist_references(&mut profiles, "Old", "New");

        let outputs = &profiles.definitions["Desk"].outputs;
        assert_eq!(outputs["DP-1"].playlist.as_deref(), Some("New"));
        assert_eq!(outputs["HDMI-A-1"], wallpaper);
    }
}
