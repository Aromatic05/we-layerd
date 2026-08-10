use std::{collections::BTreeMap, time::Duration};

use serde::{Deserialize, Serialize};

pub const DEFAULT_PLAYLIST_DURATION_MS: u64 = 1_800_000;
pub const MIN_PLAYLIST_DURATION_MS: u64 = 100;

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PlaylistMode {
    #[default]
    Sequential,
    Repeat,
    Shuffle,
    Manual,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlaylistItem {
    pub wallpaper_id: String,
    pub source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Playlist {
    #[serde(default)]
    pub mode: PlaylistMode,
    #[serde(default = "default_playlist_duration_ms")]
    pub default_duration_ms: u64,
    #[serde(default)]
    pub items: Vec<PlaylistItem>,
}

impl Default for Playlist {
    fn default() -> Self {
        Self {
            mode: PlaylistMode::Sequential,
            default_duration_ms: default_playlist_duration_ms(),
            items: Vec::new(),
        }
    }
}

impl Playlist {
    pub fn duration_ms_for(&self, index: usize) -> Option<u64> {
        self.items.get(index).map(|item| {
            item.duration_ms.unwrap_or(self.default_duration_ms).max(MIN_PLAYLIST_DURATION_MS)
        })
    }

    pub fn timed_duration_for(&self, index: usize) -> Option<Duration> {
        if self.mode == PlaylistMode::Manual {
            return None;
        }
        self.duration_ms_for(index).map(Duration::from_millis)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlaylistConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active: Option<String>,
    #[serde(default)]
    pub definitions: BTreeMap<String, Playlist>,
}

impl PlaylistConfig {
    pub fn active_playlist(&self) -> Option<(&str, &Playlist)> {
        let name = self.active.as_deref()?;
        self.definitions.get(name).map(|playlist| (name, playlist))
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlaylistCursorSnapshot {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current: Option<usize>,
    #[serde(default)]
    pub shuffle_remaining: Vec<usize>,
    #[serde(default)]
    pub history: Vec<usize>,
    #[serde(default)]
    pub forward_history: Vec<usize>,
    #[serde(default)]
    pub random_state: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaylistCursor {
    item_count: usize,
    current: Option<usize>,
    shuffle_remaining: Vec<usize>,
    history: Vec<usize>,
    forward_history: Vec<usize>,
    random_state: u64,
}

impl PlaylistCursor {
    pub fn new(item_count: usize, random_seed: u64) -> Self {
        Self {
            item_count,
            current: None,
            shuffle_remaining: Vec::new(),
            history: Vec::new(),
            forward_history: Vec::new(),
            random_state: normalize_random_state(random_seed),
        }
    }

    pub fn restore(item_count: usize, snapshot: PlaylistCursorSnapshot) -> Self {
        let valid = |index: &usize| *index < item_count;
        Self {
            item_count,
            current: snapshot.current.filter(|index| *index < item_count),
            shuffle_remaining: deduplicate_indices(
                snapshot.shuffle_remaining.into_iter().filter(valid),
            ),
            history: snapshot.history.into_iter().filter(valid).collect(),
            forward_history: snapshot.forward_history.into_iter().filter(valid).collect(),
            random_state: normalize_random_state(snapshot.random_state),
        }
    }

    pub fn snapshot(&self) -> PlaylistCursorSnapshot {
        PlaylistCursorSnapshot {
            current: self.current,
            shuffle_remaining: self.shuffle_remaining.clone(),
            history: self.history.clone(),
            forward_history: self.forward_history.clone(),
            random_state: self.random_state,
        }
    }

    pub fn current_index(&self) -> Option<usize> {
        self.current.filter(|index| *index < self.item_count)
    }

    pub fn start(&mut self, playlist: &Playlist) -> Option<usize> {
        self.sync_item_count(playlist.items.len());
        if self.item_count == 0 {
            self.current = None;
            return None;
        }
        if let Some(current) = self.current_index() {
            return Some(current);
        }

        let index = if playlist.mode == PlaylistMode::Shuffle {
            self.refill_shuffle_bag(None);
            self.take_random_from_bag()?
        } else {
            0
        };
        self.current = Some(index);
        Some(index)
    }

    pub fn next(&mut self, playlist: &Playlist) -> Option<usize> {
        self.sync_item_count(playlist.items.len());
        if self.item_count == 0 {
            self.current = None;
            return None;
        }
        if self.current.is_none() {
            return self.start(playlist);
        }

        let current = self.current?;
        let replaying_forward =
            playlist.mode == PlaylistMode::Shuffle && !self.forward_history.is_empty();
        let next = match playlist.mode {
            PlaylistMode::Sequential => {
                current.checked_add(1).filter(|next| *next < self.item_count)
            }
            PlaylistMode::Repeat | PlaylistMode::Manual => Some((current + 1) % self.item_count),
            PlaylistMode::Shuffle => {
                if let Some(forward) = self.forward_history.pop() {
                    Some(forward)
                } else {
                    if self.shuffle_remaining.is_empty() {
                        self.refill_shuffle_bag(Some(current));
                    }
                    self.take_random_from_bag()
                }
            }
        }?;

        self.history.push(current);
        if !replaying_forward {
            self.forward_history.clear();
        }
        self.current = Some(next);
        Some(next)
    }

    pub fn previous(&mut self, playlist: &Playlist) -> Option<usize> {
        self.sync_item_count(playlist.items.len());
        let current = self.current?;
        let previous = match playlist.mode {
            PlaylistMode::Sequential => current.checked_sub(1),
            PlaylistMode::Repeat | PlaylistMode::Manual => {
                Some(if current == 0 { self.item_count.checked_sub(1)? } else { current - 1 })
            }
            PlaylistMode::Shuffle => self.history.pop(),
        }?;

        if playlist.mode == PlaylistMode::Shuffle {
            self.forward_history.push(current);
        }
        self.current = Some(previous);
        Some(previous)
    }

    fn sync_item_count(&mut self, item_count: usize) {
        if self.item_count == item_count {
            return;
        }
        self.item_count = item_count;
        if self.current.is_some_and(|index| index >= item_count) {
            self.current = None;
        }
        self.shuffle_remaining.retain(|index| *index < item_count);
        self.history.retain(|index| *index < item_count);
        self.forward_history.retain(|index| *index < item_count);
    }

    fn refill_shuffle_bag(&mut self, exclude: Option<usize>) {
        self.shuffle_remaining =
            (0..self.item_count).filter(|index| Some(*index) != exclude).collect();
    }

    fn take_random_from_bag(&mut self) -> Option<usize> {
        if self.shuffle_remaining.is_empty() {
            return None;
        }
        let index = self.next_random_index(self.shuffle_remaining.len());
        Some(self.shuffle_remaining.swap_remove(index))
    }

    fn next_random_index(&mut self, upper_bound: usize) -> usize {
        debug_assert!(upper_bound > 0);
        let mut state = self.random_state;
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        self.random_state = normalize_random_state(state);
        (self.random_state as usize) % upper_bound
    }
}

fn default_playlist_duration_ms() -> u64 {
    DEFAULT_PLAYLIST_DURATION_MS
}

fn normalize_random_state(state: u64) -> u64 {
    if state == 0 {
        0x9e37_79b9_7f4a_7c15
    } else {
        state
    }
}

fn deduplicate_indices(indices: impl IntoIterator<Item = usize>) -> Vec<usize> {
    let mut unique = Vec::new();
    for index in indices {
        if !unique.contains(&index) {
            unique.push(index);
        }
    }
    unique
}

#[cfg(test)]
mod tests {
    use super::{Playlist, PlaylistCursor, PlaylistItem, PlaylistMode};

    fn item(id: &str) -> PlaylistItem {
        PlaylistItem {
            wallpaper_id: id.to_string(),
            source: format!("/workshop/{id}"),
            duration_ms: None,
        }
    }

    #[test]
    fn sequential_playback_preserves_duplicate_entries_and_stops_at_the_end() {
        let playlist = Playlist {
            mode: PlaylistMode::Sequential,
            default_duration_ms: 30_000,
            items: vec![item("alpha"), item("alpha"), item("beta")],
        };
        let mut cursor = PlaylistCursor::new(playlist.items.len(), 7);

        assert_eq!(cursor.start(&playlist), Some(0));
        assert_eq!(cursor.next(&playlist), Some(1));
        assert_eq!(cursor.next(&playlist), Some(2));
        assert_eq!(cursor.next(&playlist), None);
    }

    #[test]
    fn repeat_wraps_while_manual_mode_never_requests_timed_progression() {
        let repeat = Playlist {
            mode: PlaylistMode::Repeat,
            default_duration_ms: 1_000,
            items: vec![item("alpha"), item("beta")],
        };
        let mut cursor = PlaylistCursor::new(repeat.items.len(), 11);
        assert_eq!(cursor.start(&repeat), Some(0));
        assert_eq!(cursor.next(&repeat), Some(1));
        assert_eq!(cursor.next(&repeat), Some(0));

        let manual = Playlist { mode: PlaylistMode::Manual, ..repeat };
        assert_eq!(manual.timed_duration_for(0), None);
        assert_eq!(manual.timed_duration_for(1), None);
    }

    #[test]
    fn shuffle_bag_visits_every_entry_before_reusing_one() {
        let playlist = Playlist {
            mode: PlaylistMode::Shuffle,
            default_duration_ms: 1_000,
            items: vec![item("a"), item("b"), item("c"), item("d")],
        };
        let mut cursor = PlaylistCursor::new(playlist.items.len(), 0x1234_5678);
        let first = cursor.start(&playlist).expect("first item");
        let second = cursor.next(&playlist).expect("second item");
        let third = cursor.next(&playlist).expect("third item");
        let fourth = cursor.next(&playlist).expect("fourth item");

        let mut cycle = vec![first, second, third, fourth];
        cycle.sort_unstable();
        assert_eq!(cycle, vec![0, 1, 2, 3]);

        let next_cycle = cursor.next(&playlist).expect("next cycle starts");
        assert_ne!(next_cycle, fourth);
    }

    #[test]
    fn snapshot_restores_current_position_and_shuffle_history() {
        let playlist = Playlist {
            mode: PlaylistMode::Shuffle,
            default_duration_ms: 1_000,
            items: vec![item("a"), item("b"), item("c")],
        };
        let mut cursor = PlaylistCursor::new(playlist.items.len(), 99);
        cursor.start(&playlist);
        cursor.next(&playlist);
        let expected_current = cursor.current_index();
        let expected_previous = cursor.previous(&playlist);

        let snapshot = cursor.snapshot();
        let mut restored = PlaylistCursor::restore(playlist.items.len(), snapshot);
        assert_eq!(restored.current_index(), expected_previous);
        assert_eq!(restored.next(&playlist), expected_current);
    }
}
