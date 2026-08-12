use std::{collections::HashMap, time::Duration};

use anyhow::{Context, Result};
use we_renderer::{MediaPlaybackState as RendererMediaPlaybackState, MediaState};
use zbus::{
    blocking::{connection::Builder as ConnectionBuilder, Connection, Proxy},
    zvariant::OwnedValue,
};

const MPRIS_METHOD_TIMEOUT: Duration = Duration::from_secs(1);

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum MediaPlaybackState {
    Playing,
    Paused,
    #[default]
    Stopped,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct MediaCandidate {
    pub(crate) bus_name: String,
    pub(crate) playback: MediaPlaybackState,
    pub(crate) title: String,
    pub(crate) artist: String,
    pub(crate) album_title: String,
    pub(crate) album_artist: String,
    pub(crate) genres: String,
}

pub(crate) fn choose_media_candidate(candidates: &[MediaCandidate]) -> Option<MediaCandidate> {
    let mut candidates = candidates.to_vec();
    candidates.sort_by(|left, right| {
        media_playback_rank(left.playback)
            .cmp(&media_playback_rank(right.playback))
            .then_with(|| left.bus_name.cmp(&right.bus_name))
    });
    candidates.into_iter().next()
}

fn media_playback_rank(state: MediaPlaybackState) -> u8 {
    match state {
        MediaPlaybackState::Playing => 0,
        MediaPlaybackState::Paused => 1,
        MediaPlaybackState::Stopped => 2,
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct MediaBridgeState {
    current: Option<MediaCandidate>,
}

impl MediaBridgeState {
    pub(crate) fn update(
        &mut self,
        enabled: bool,
        candidates: &[MediaCandidate],
    ) -> Option<MediaCandidate> {
        self.current = enabled.then(|| choose_media_candidate(candidates)).flatten();
        self.current.clone()
    }
}

pub(crate) fn session_connection() -> Result<Connection> {
    ConnectionBuilder::session()
        .context("failed to resolve the session D-Bus for MPRIS")?
        .method_timeout(MPRIS_METHOD_TIMEOUT)
        .build()
        .context("failed to connect to the session D-Bus for MPRIS")
}

pub(crate) fn read_mpris_candidates_until(
    connection: &Connection,
    mut cancelled: impl FnMut() -> bool,
) -> Result<Option<Vec<MediaCandidate>>> {
    if cancelled() {
        return Ok(None);
    }
    let dbus = Proxy::new(
        connection,
        "org.freedesktop.DBus",
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
    )
    .context("failed to create D-Bus discovery proxy")?;
    let names: Vec<String> = dbus.call("ListNames", &()).context("failed to list D-Bus names")?;
    if cancelled() {
        return Ok(None);
    }
    let mut candidates = Vec::new();
    for bus_name in names.into_iter().filter(|name| name.starts_with("org.mpris.MediaPlayer2.")) {
        if cancelled() {
            return Ok(None);
        }
        let Ok(proxy) = Proxy::new(
            connection,
            bus_name.as_str(),
            "/org/mpris/MediaPlayer2",
            "org.mpris.MediaPlayer2.Player",
        ) else {
            continue;
        };
        let Ok(playback_status) = proxy.get_property::<String>("PlaybackStatus") else {
            continue;
        };
        if cancelled() {
            return Ok(None);
        }
        let metadata =
            proxy.get_property::<HashMap<String, OwnedValue>>("Metadata").unwrap_or_default();
        candidates.push(MediaCandidate {
            bus_name: bus_name.clone(),
            playback: match playback_status.as_str() {
                "Playing" => MediaPlaybackState::Playing,
                "Paused" => MediaPlaybackState::Paused,
                _ => MediaPlaybackState::Stopped,
            },
            title: metadata_string(&metadata, "xesam:title"),
            artist: metadata_strings(&metadata, "xesam:artist").join(", "),
            album_title: metadata_string(&metadata, "xesam:album"),
            album_artist: metadata_strings(&metadata, "xesam:albumArtist").join(", "),
            genres: metadata_strings(&metadata, "xesam:genre").join(", "),
        });
    }
    Ok(Some(candidates))
}

pub(crate) fn to_renderer_state(candidate: Option<&MediaCandidate>) -> MediaState {
    let Some(candidate) = candidate else {
        return MediaState::default();
    };
    MediaState {
        playback: match candidate.playback {
            MediaPlaybackState::Playing => RendererMediaPlaybackState::Playing,
            MediaPlaybackState::Paused => RendererMediaPlaybackState::Paused,
            MediaPlaybackState::Stopped => RendererMediaPlaybackState::Stopped,
        },
        title: candidate.title.clone(),
        artist: candidate.artist.clone(),
        album_title: candidate.album_title.clone(),
        album_artist: candidate.album_artist.clone(),
        genres: candidate.genres.clone(),
        content_type: "music".to_string(),
        ..MediaState::default()
    }
}

fn metadata_string(metadata: &HashMap<String, OwnedValue>, key: &str) -> String {
    metadata
        .get(key)
        .and_then(|value| value.try_clone().ok())
        .and_then(|value| String::try_from(value).ok())
        .unwrap_or_default()
}

fn metadata_strings(metadata: &HashMap<String, OwnedValue>, key: &str) -> Vec<String> {
    metadata
        .get(key)
        .and_then(|value| value.try_clone().ok())
        .and_then(|value| Vec::<String>::try_from(value).ok())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::{choose_media_candidate, MediaBridgeState, MediaCandidate, MediaPlaybackState};

    fn candidate(name: &str, playback: MediaPlaybackState) -> MediaCandidate {
        MediaCandidate {
            bus_name: name.to_string(),
            playback,
            title: name.to_string(),
            artist: String::new(),
            album_title: String::new(),
            album_artist: String::new(),
            genres: String::new(),
        }
    }

    #[test]
    fn playing_player_is_preferred_then_selection_is_deterministic() {
        let chosen = choose_media_candidate(&[
            candidate("org.mpris.MediaPlayer2.zeta", MediaPlaybackState::Paused),
            candidate("org.mpris.MediaPlayer2.beta", MediaPlaybackState::Playing),
            candidate("org.mpris.MediaPlayer2.alpha", MediaPlaybackState::Playing),
        ])
        .expect("a player is available");

        assert_eq!(chosen.bus_name, "org.mpris.MediaPlayer2.alpha");
        assert_eq!(chosen.playback, MediaPlaybackState::Playing);
    }

    #[test]
    fn disabling_media_bridge_clears_the_previously_selected_player() {
        let mut bridge = MediaBridgeState::default();
        assert!(bridge
            .update(
                true,
                &[candidate("org.mpris.MediaPlayer2.player", MediaPlaybackState::Playing)],
            )
            .is_some());

        assert_eq!(bridge.update(false, &[]), None);
        assert_eq!(bridge, MediaBridgeState::default());
    }
}
