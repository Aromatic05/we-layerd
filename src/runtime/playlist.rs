use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use we_core::playlist::{
    Playlist, PlaylistConfig, PlaylistCursor, PlaylistCursorSnapshot, PlaylistItem,
};
use we_core::wallpaper::settings::{RenderResolution, WallpaperSettings};

use crate::config::Config;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AdvanceDirection {
    Next,
    Previous,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PlaylistSelection {
    pub(crate) index: usize,
    pub(crate) wallpaper_id: String,
    pub(crate) source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct PlaylistRuntimeSnapshot {
    pub(crate) active_playlist: String,
    pub(crate) cursor: PlaylistCursorSnapshot,
}

#[derive(Debug, Clone)]
pub(crate) struct PlaylistRuntime {
    config: PlaylistConfig,
    active_playlist: Option<String>,
    cursor: PlaylistCursor,
    timer_started_at: Option<Instant>,
    elapsed_before_pause: Duration,
    paused: bool,
    exhausted: bool,
    seed: u64,
}

impl PlaylistRuntime {
    pub(crate) fn new(config: PlaylistConfig, random_seed: u64) -> Self {
        let active_playlist =
            config.active.as_ref().filter(|name| config.definitions.contains_key(*name)).cloned();
        let item_count = active_playlist
            .as_deref()
            .and_then(|name| config.definitions.get(name))
            .map(|playlist| playlist.items.len())
            .unwrap_or(0);
        Self {
            config,
            active_playlist,
            cursor: PlaylistCursor::new(item_count, random_seed),
            timer_started_at: None,
            elapsed_before_pause: Duration::ZERO,
            paused: false,
            exhausted: false,
            seed: random_seed,
        }
    }

    pub(crate) fn restore(
        config: PlaylistConfig,
        snapshot: PlaylistRuntimeSnapshot,
        now: Instant,
    ) -> Self {
        let mut runtime = Self::new(config, snapshot.cursor.random_state);
        if runtime.config.definitions.contains_key(&snapshot.active_playlist) {
            runtime.active_playlist = Some(snapshot.active_playlist.clone());
            runtime.config.active = Some(snapshot.active_playlist);
            let item_count =
                runtime.active_playlist_definition().map(|p| p.items.len()).unwrap_or(0);
            runtime.cursor = PlaylistCursor::restore(item_count, snapshot.cursor);
            if runtime.cursor.current_index().is_some() {
                runtime.timer_started_at = Some(now);
            }
        }
        runtime
    }

    pub(crate) fn configure(&mut self, config: PlaylistConfig, now: Instant) {
        let previous_snapshot = self.snapshot();
        let previous_active = self.active_playlist.clone();
        let previous_active_definition = self.active_playlist_definition().cloned();
        self.config = config;
        self.active_playlist = self
            .config
            .active
            .as_ref()
            .filter(|name| self.config.definitions.contains_key(*name))
            .cloned();
        if self.active_playlist == previous_active
            && self.active_playlist_definition() == previous_active_definition.as_ref()
        {
            return;
        }
        self.exhausted = false;

        if self.active_playlist == previous_active {
            if let Some(snapshot) = previous_snapshot {
                let item_count =
                    self.active_playlist_definition().map(|p| p.items.len()).unwrap_or(0);
                self.cursor = PlaylistCursor::restore(item_count, snapshot.cursor);
                self.reset_timer(now);
                return;
            }
        }

        let item_count = self.active_playlist_definition().map(|p| p.items.len()).unwrap_or(0);
        self.seed = next_seed(self.seed);
        self.cursor = PlaylistCursor::new(item_count, self.seed);
        self.reset_timer(now);
    }

    pub(crate) fn play(&mut self, name: &str, now: Instant) -> Result<PlaylistSelection, String> {
        let Some(playlist) = self.config.definitions.get(name) else {
            return Err(format!("playlist '{name}' does not exist"));
        };
        if playlist.items.is_empty() {
            return Err(format!("playlist '{name}' is empty"));
        }
        self.active_playlist = Some(name.to_string());
        self.config.active = Some(name.to_string());
        self.seed = next_seed(self.seed);
        self.cursor = PlaylistCursor::new(playlist.items.len(), self.seed);
        self.exhausted = false;
        let index =
            self.cursor.start(playlist).ok_or_else(|| format!("playlist '{name}' is empty"))?;
        self.reset_timer(now);
        self.selection_at(index).ok_or_else(|| "playlist selection is invalid".to_string())
    }

    pub(crate) fn stop(&mut self) {
        self.active_playlist = None;
        self.config.active = None;
        self.timer_started_at = None;
        self.elapsed_before_pause = Duration::ZERO;
        self.exhausted = false;
    }

    pub(crate) fn ensure_started(&mut self, now: Instant) -> Option<PlaylistSelection> {
        if self.exhausted {
            return None;
        }
        let playlist = self.active_playlist_definition()?.clone();
        let index = self.cursor.start(&playlist)?;
        if self.timer_started_at.is_none() && !self.paused {
            self.timer_started_at = Some(now);
        }
        self.selection_at(index)
    }

    pub(crate) fn current_selection(&self) -> Option<PlaylistSelection> {
        self.selection_at(self.cursor.current_index()?)
    }

    pub(crate) fn active_name(&self) -> Option<&str> {
        self.active_playlist.as_deref()
    }

    pub(crate) fn due_selection<F>(
        &mut self,
        now: Instant,
        playable: F,
    ) -> Option<PlaylistSelection>
    where
        F: Fn(&PlaylistItem) -> bool,
    {
        if self.paused || self.exhausted {
            return None;
        }
        let current = self.ensure_started(now)?;
        let playlist = self.active_playlist_definition()?;
        let duration = playlist.timed_duration_for(current.index)?;
        if self.elapsed(now) < duration {
            return None;
        }
        self.advance(AdvanceDirection::Next, now, playable)
    }

    pub(crate) fn advance<F>(
        &mut self,
        direction: AdvanceDirection,
        now: Instant,
        playable: F,
    ) -> Option<PlaylistSelection>
    where
        F: Fn(&PlaylistItem) -> bool,
    {
        if self.exhausted && direction == AdvanceDirection::Next {
            return None;
        }
        let playlist = self.active_playlist_definition()?.clone();
        if self.cursor.current_index().is_none() {
            self.cursor.start(&playlist)?;
        }

        let attempts = playlist.items.len().max(1);
        for _ in 0..attempts {
            let index = match direction {
                AdvanceDirection::Next => self.cursor.next(&playlist),
                AdvanceDirection::Previous => self.cursor.previous(&playlist),
            };
            let Some(index) = index else {
                if direction == AdvanceDirection::Next {
                    self.exhausted = true;
                    self.timer_started_at = None;
                }
                return None;
            };
            let item = playlist.items.get(index)?;
            if playable(item) {
                self.exhausted = false;
                self.reset_timer(now);
                return self.selection_at(index);
            }
        }
        None
    }

    pub(crate) fn pause(&mut self, now: Instant) {
        if self.paused {
            return;
        }
        if let Some(started) = self.timer_started_at.take() {
            self.elapsed_before_pause += now.saturating_duration_since(started);
        }
        self.paused = true;
    }

    pub(crate) fn resume(&mut self, now: Instant) {
        if !self.paused {
            return;
        }
        self.paused = false;
        if self.current_selection().is_some() && !self.exhausted {
            self.timer_started_at = Some(now);
        }
    }

    pub(crate) fn snapshot(&self) -> Option<PlaylistRuntimeSnapshot> {
        Some(PlaylistRuntimeSnapshot {
            active_playlist: self.active_playlist.clone()?,
            cursor: self.cursor.snapshot(),
        })
    }

    pub(crate) fn render_status_toml(&self) -> String {
        let mut lines = vec!["[playlist_runtime]".to_string()];
        match self.active_playlist.as_deref() {
            Some(name) => lines.push(format!("active = {:?}", name)),
            None => lines.push("active = false".to_string()),
        }
        if let Some(selection) = self.current_selection() {
            lines.push(format!("index = {}", selection.index));
            lines.push(format!("wallpaper_id = {:?}", selection.wallpaper_id));
            lines.push(format!("source = {:?}", selection.source));
        }
        lines.push(format!("paused = {}", self.paused));
        lines.push(format!("exhausted = {}", self.exhausted));
        lines.join("\n")
    }

    fn active_playlist_definition(&self) -> Option<&Playlist> {
        self.config.definitions.get(self.active_playlist.as_deref()?)
    }

    fn selection_at(&self, index: usize) -> Option<PlaylistSelection> {
        let playlist_name = self.active_playlist.as_ref()?;
        let item = self.config.definitions.get(playlist_name)?.items.get(index)?;
        Some(PlaylistSelection {
            index,
            wallpaper_id: item.wallpaper_id.clone(),
            source: item.source.clone(),
        })
    }

    fn elapsed(&self, now: Instant) -> Duration {
        let live = self
            .timer_started_at
            .map(|started| now.saturating_duration_since(started))
            .unwrap_or(Duration::ZERO);
        self.elapsed_before_pause + live
    }

    fn reset_timer(&mut self, now: Instant) {
        self.elapsed_before_pause = Duration::ZERO;
        self.timer_started_at = if self.paused { None } else { Some(now) };
    }
}

pub(crate) fn state_path_for_config(config_path: Option<&Path>) -> Option<PathBuf> {
    config_path.map(|path| {
        let mut state_path = path.as_os_str().to_os_string();
        state_path.push(".playlist-state.json");
        PathBuf::from(state_path)
    })
}

pub(crate) fn load_snapshot(path: &Path) -> Result<Option<PlaylistRuntimeSnapshot>> {
    let raw = match fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(error).with_context(|| format!("failed to read {}", path.display()))
        }
    };
    serde_json::from_str(&raw)
        .map(Some)
        .with_context(|| format!("invalid playlist runtime state in {}", path.display()))
}

pub(crate) fn persist_snapshot(
    path: &Path,
    snapshot: Option<&PlaylistRuntimeSnapshot>,
) -> Result<()> {
    let Some(snapshot) = snapshot else {
        match fs::remove_file(path) {
            Ok(()) => return Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => {
                return Err(error).with_context(|| format!("failed to remove {}", path.display()))
            }
        }
    };
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let raw = serde_json::to_vec_pretty(snapshot).context("failed to serialize playlist state")?;
    let mut temporary = path.as_os_str().to_os_string();
    temporary.push(format!(".{}.tmp", std::process::id()));
    let temporary = PathBuf::from(temporary);
    fs::write(&temporary, raw)
        .with_context(|| format!("failed to write {}", temporary.display()))?;
    fs::rename(&temporary, path).with_context(|| format!("failed to replace {}", path.display()))
}

pub(crate) fn random_seed() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos() as u64)
        .unwrap_or(0x9e37_79b9_7f4a_7c15)
}

pub(crate) fn apply_selection_to_config(
    config: &mut Config,
    selection: &PlaylistSelection,
) -> Result<()> {
    config.renderer.source = selection.source.clone();
    let wallpaper_profile = config.wallpapers.get(&selection.wallpaper_id).cloned();
    let wallpaper = wallpaper_profile.clone().unwrap_or_else(WallpaperSettings::default);
    config.renderer.fps = wallpaper.fps.clamp(1, 360);
    config.renderer.speed = wallpaper.speed;
    config.renderer.volume = wallpaper.volume;
    config.renderer.muted = wallpaper.muted;
    if let Some(profile) = wallpaper_profile {
        config.renderer.msaa_samples = profile.msaa_samples.max(1);
    }
    config.renderer.fill_mode = wallpaper.fill_mode;
    config.renderer.rotation_degrees = wallpaper.rotation_degrees.degrees();
    match wallpaper.render_resolution {
        RenderResolution::Automatic => {
            config.renderer.render_width = None;
            config.renderer.render_height = None;
        }
        RenderResolution::Fixed { width, height } => {
            config.renderer.render_width = Some(width.max(1));
            config.renderer.render_height = Some(height.max(1));
        }
    }
    config.renderer.options_json = Some(we_core::config::merge_scene_source_options(
        config.renderer.options_json.as_deref(),
        Some(wallpaper.user_properties),
        config.general.force_scene_audio_loop,
    )?);
    Ok(())
}

fn next_seed(seed: u64) -> u64 {
    seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407)
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        time::{Duration, Instant, SystemTime, UNIX_EPOCH},
    };

    use we_core::playlist::{Playlist, PlaylistConfig, PlaylistItem, PlaylistMode};

    use super::{load_snapshot, persist_snapshot, AdvanceDirection, PlaylistRuntime};

    fn config(mode: PlaylistMode, ids: &[&str]) -> PlaylistConfig {
        let mut config =
            PlaylistConfig { active: Some("daily".to_string()), ..PlaylistConfig::default() };
        config.definitions.insert(
            "daily".to_string(),
            Playlist {
                mode,
                default_duration_ms: 1_000,
                items: ids
                    .iter()
                    .map(|id| PlaylistItem {
                        wallpaper_id: (*id).to_string(),
                        source: format!("/wallpapers/{id}"),
                        duration_ms: None,
                    })
                    .collect(),
            },
        );
        config
    }

    #[test]
    fn timer_freezes_while_paused_and_resumes_from_the_same_elapsed_time() {
        let started = Instant::now();
        let mut runtime = PlaylistRuntime::new(config(PlaylistMode::Repeat, &["a", "b"]), 5);
        runtime.ensure_started(started).expect("playlist starts");

        runtime.pause(started + Duration::from_millis(400));
        assert!(runtime.due_selection(started + Duration::from_secs(10), |_| true).is_none());

        runtime.resume(started + Duration::from_secs(10));
        assert!(runtime.due_selection(started + Duration::from_millis(10_599), |_| true).is_none());
        assert_eq!(
            runtime
                .due_selection(started + Duration::from_millis(10_600), |_| true)
                .expect("remaining 600ms expires")
                .wallpaper_id,
            "b"
        );
    }

    #[test]
    fn unrelated_playlist_definition_refresh_preserves_active_elapsed_time() {
        let started = Instant::now();
        let mut playlist_config = config(PlaylistMode::Repeat, &["a", "b"]);
        let mut runtime = PlaylistRuntime::new(playlist_config.clone(), 7);
        runtime.ensure_started(started).expect("playlist starts");

        playlist_config.definitions.insert("later".to_string(), Playlist::default());
        runtime.configure(playlist_config, started + Duration::from_millis(400));

        assert_eq!(
            runtime
                .due_selection(started + Duration::from_millis(1_000), |_| true)
                .expect("unrelated refresh must not restart the active timer")
                .wallpaper_id,
            "b"
        );
    }

    #[test]
    fn progression_skips_unavailable_wallpapers_without_ending_the_playlist() {
        let started = Instant::now();
        let mut runtime =
            PlaylistRuntime::new(config(PlaylistMode::Repeat, &["a", "missing", "c"]), 9);
        runtime.ensure_started(started).expect("playlist starts");

        let selection = runtime
            .advance(AdvanceDirection::Next, started, |item| item.wallpaper_id != "missing")
            .expect("a later playable item exists");
        assert_eq!(selection.wallpaper_id, "c");
    }

    #[test]
    fn profileless_playlist_selection_preserves_global_msaa() {
        let mut runtime_config = crate::config::Config::default();
        runtime_config.renderer.msaa_samples = 8;
        let selection = super::PlaylistSelection {
            index: 0,
            wallpaper_id: "scene".to_string(),
            source: "/wallpapers/scene".to_string(),
        };

        super::apply_selection_to_config(&mut runtime_config, &selection)
            .expect("apply playlist selection");

        assert_eq!(runtime_config.renderer.msaa_samples, 8);
    }

    #[test]
    fn restored_snapshot_resumes_at_the_same_item_with_a_fresh_timer() {
        let started = Instant::now();
        let playlist_config = config(PlaylistMode::Repeat, &["a", "b", "c"]);
        let mut runtime = PlaylistRuntime::new(playlist_config.clone(), 17);
        runtime.ensure_started(started).expect("playlist starts");
        runtime
            .advance(AdvanceDirection::Next, started + Duration::from_secs(1), |_| true)
            .expect("advance to second item");
        let snapshot = runtime.snapshot().expect("runtime snapshot");

        let restored_at = started + Duration::from_secs(30);
        let mut restored = PlaylistRuntime::restore(playlist_config, snapshot, restored_at);
        assert_eq!(restored.current_selection().expect("current item").wallpaper_id, "b");
        assert!(restored
            .due_selection(restored_at + Duration::from_millis(999), |_| true)
            .is_none());
        assert_eq!(
            restored
                .due_selection(restored_at + Duration::from_millis(1_000), |_| true)
                .expect("fresh timer expires")
                .wallpaper_id,
            "c"
        );
    }

    #[test]
    fn clearing_playlist_runtime_state_prevents_a_stopped_playlist_from_restoring() {
        let suffix = SystemTime::now().duration_since(UNIX_EPOCH).expect("system clock").as_nanos();
        let root = std::env::temp_dir()
            .join(format!("we-layerd-playlist-runtime-state-{}-{suffix}", std::process::id()));
        let path = root.join("runtime.json");
        let started = Instant::now();
        let mut runtime = PlaylistRuntime::new(config(PlaylistMode::Repeat, &["a", "b"]), 23);
        runtime.ensure_started(started).expect("playlist starts");
        let snapshot = runtime.snapshot().expect("active runtime snapshot");

        persist_snapshot(&path, Some(&snapshot)).expect("persist active playlist state");
        assert!(load_snapshot(&path).expect("load active state").is_some());

        runtime.stop();
        persist_snapshot(&path, runtime.snapshot().as_ref()).expect("clear stopped playlist state");
        assert_eq!(load_snapshot(&path).expect("load cleared state"), None);

        let _ = fs::remove_dir_all(root);
    }
}
