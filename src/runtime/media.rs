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
}

pub(crate) fn choose_media_candidate(_candidates: &[MediaCandidate]) -> Option<MediaCandidate> {
    todo!("implemented after the behavior tests are established")
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct MediaBridgeState {
    current: Option<MediaCandidate>,
}

impl MediaBridgeState {
    pub(crate) fn update(
        &mut self,
        _enabled: bool,
        _candidates: &[MediaCandidate],
    ) -> Option<MediaCandidate> {
        todo!("implemented after the behavior tests are established")
    }
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
