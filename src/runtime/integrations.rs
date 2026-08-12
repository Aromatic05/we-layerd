use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};

use serde::Serialize;
use tracing::warn;
use we_core::config::IntegrationsConfig;
use we_renderer::MediaState;

use crate::config::Config;

use super::{
    audio::{PulseAudioCapture, AUDIO_SPECTRUM_BINS},
    media::{self, MediaBridgeState},
    rules::{policies_for_outputs, ForeignToplevelMonitor, RuleSet, RuntimePolicy},
};

const INTEGRATION_SUPERVISOR_POLL: Duration = Duration::from_millis(50);
const MEDIA_STALE_AFTER: Duration = Duration::from_secs(3);
const AUDIO_STALE_AFTER: Duration = Duration::from_secs(2);

#[derive(Debug, Clone)]
pub(crate) struct OutputIntegrationSnapshot {
    pub(crate) media_generation: u64,
    pub(crate) media: MediaState,
    pub(crate) audio_generation: u64,
    pub(crate) audio: Arc<[f32]>,
    pub(crate) policy_generation: u64,
    pub(crate) policy: RuntimePolicy,
}

#[derive(Debug, Clone)]
struct IntegrationState {
    media_generation: u64,
    media: MediaState,
    media_enabled: bool,
    media_available: bool,
    media_player: Option<String>,
    media_error: Option<String>,
    audio_generation: u64,
    audio: Arc<[f32]>,
    audio_enabled: bool,
    audio_available: bool,
    audio_source: String,
    audio_error: Option<String>,
    policy_generation: u64,
    policies: BTreeMap<String, RuntimePolicy>,
    rules_enabled: bool,
    rules_available: bool,
    rules_error: Option<String>,
}

impl Default for IntegrationState {
    fn default() -> Self {
        Self {
            media_generation: 0,
            media: MediaState::default(),
            media_enabled: false,
            media_available: false,
            media_player: None,
            media_error: None,
            audio_generation: 0,
            audio: vec![0.0; AUDIO_SPECTRUM_BINS * 2].into(),
            audio_enabled: false,
            audio_available: false,
            audio_source: String::new(),
            audio_error: None,
            policy_generation: 0,
            policies: BTreeMap::new(),
            rules_enabled: false,
            rules_available: false,
            rules_error: None,
        }
    }
}

#[derive(Clone, Default)]
pub(crate) struct HostIntegrations {
    state: Arc<Mutex<IntegrationState>>,
}

pub(crate) struct HostIntegrationRuntime {
    pub(crate) shared: HostIntegrations,
    handles: Vec<thread::JoinHandle<()>>,
    shutdown: Arc<AtomicBool>,
}

impl HostIntegrationRuntime {
    pub(crate) fn start(desired_cfg: Arc<Mutex<Config>>, shutdown: Arc<AtomicBool>) -> Self {
        let shared = HostIntegrations::default();
        let handles = vec![
            spawn_media_collector(shared.clone(), desired_cfg.clone(), shutdown.clone()),
            spawn_audio_collector(shared.clone(), desired_cfg.clone(), shutdown.clone()),
            spawn_rule_collector(shared.clone(), desired_cfg, shutdown.clone()),
        ];
        Self { shared, handles, shutdown }
    }

    pub(crate) fn shutdown(self) {
        self.shutdown.store(true, Ordering::Relaxed);
        // PulseAudio simple reads and blocking D-Bus calls are owned by external libraries and may
        // not return promptly when their peer disappears. This runs only during daemon teardown:
        // reap collectors that already finished and detach the rest so external I/O cannot hold
        // the process exit path indefinitely.
        for handle in self.handles {
            if handle.is_finished() {
                let _ = handle.join();
            }
        }
    }
}

impl HostIntegrations {
    pub(crate) fn snapshot_for_output(&self, output: &str) -> OutputIntegrationSnapshot {
        let state = self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        OutputIntegrationSnapshot {
            media_generation: state.media_generation,
            media: state.media.clone(),
            audio_generation: state.audio_generation,
            audio: Arc::clone(&state.audio),
            policy_generation: state.policy_generation,
            policy: state.policies.get(output).copied().unwrap_or_default(),
        }
    }

    pub(crate) fn render_status_toml(&self) -> String {
        #[derive(Serialize)]
        struct Status<'a> {
            integration_runtime: IntegrationsStatus<'a>,
        }
        #[derive(Serialize)]
        struct IntegrationsStatus<'a> {
            media_enabled: bool,
            media_available: bool,
            media_player: &'a str,
            media_error: &'a str,
            audio_enabled: bool,
            audio_available: bool,
            audio_source: &'a str,
            audio_error: &'a str,
            rules_enabled: bool,
            rules_available: bool,
            rules_error: &'a str,
        }

        let state = self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        toml::to_string(&Status {
            integration_runtime: IntegrationsStatus {
                media_enabled: state.media_enabled,
                media_available: state.media_available,
                media_player: state.media_player.as_deref().unwrap_or(""),
                media_error: state.media_error.as_deref().unwrap_or(""),
                audio_enabled: state.audio_enabled,
                audio_available: state.audio_available,
                audio_source: &state.audio_source,
                audio_error: state.audio_error.as_deref().unwrap_or(""),
                rules_enabled: state.rules_enabled,
                rules_available: state.rules_available,
                rules_error: state.rules_error.as_deref().unwrap_or(""),
            },
        })
        .unwrap_or_default()
    }

    fn update_media(
        &self,
        enabled: bool,
        available: bool,
        player: Option<String>,
        media_state: MediaState,
        error: Option<String>,
    ) {
        let mut state = self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if state.media != media_state {
            state.media = media_state;
            state.media_generation = state.media_generation.wrapping_add(1);
        }
        state.media_enabled = enabled;
        state.media_available = available;
        state.media_player = player;
        state.media_error = error;
    }

    fn update_audio(
        &self,
        enabled: bool,
        available: bool,
        source: String,
        samples: Option<Arc<[f32]>>,
        error: Option<String>,
    ) {
        let mut state = self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if !available {
            if state.audio.iter().any(|sample| *sample != 0.0) {
                state.audio = Arc::from(vec![0.0_f32; AUDIO_SPECTRUM_BINS * 2]);
                state.audio_generation = state.audio_generation.wrapping_add(1);
            }
        } else if let Some(samples) = samples {
            if state.audio.as_ref() != samples.as_ref() {
                state.audio = samples;
                state.audio_generation = state.audio_generation.wrapping_add(1);
            }
        }
        state.audio_enabled = enabled;
        state.audio_available = available;
        state.audio_source = source;
        state.audio_error = error;
    }

    fn update_rules(
        &self,
        enabled: bool,
        available: bool,
        policies: BTreeMap<String, RuntimePolicy>,
        error: Option<String>,
    ) {
        let mut state = self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if state.policies != policies {
            state.policies = policies;
            state.policy_generation = state.policy_generation.wrapping_add(1);
        }
        state.rules_enabled = enabled;
        state.rules_available = available;
        state.rules_error = error;
    }
}

#[derive(Debug)]
enum MediaWorkerEvent {
    Candidates { generation: u64, candidates: Vec<media::MediaCandidate> },
    Error { generation: u64, error: String },
}

impl MediaWorkerEvent {
    fn generation(&self) -> u64 {
        match self {
            Self::Candidates { generation, .. } | Self::Error { generation, .. } => *generation,
        }
    }
}

#[derive(Default)]
struct MediaSupervisorState {
    generation: u64,
    enabled: bool,
    available: bool,
    last_event_at: Option<Instant>,
    bridge: MediaBridgeState,
}

impl MediaSupervisorState {
    fn reconcile(
        &mut self,
        enabled: bool,
        _now: Instant,
        shared: &HostIntegrations,
    ) -> Option<u64> {
        if self.enabled == enabled {
            return None;
        }

        self.generation = self.generation.wrapping_add(1);
        self.enabled = enabled;
        self.available = false;
        self.last_event_at = None;
        self.bridge.update(false, &[]);
        if enabled {
            shared.update_media(true, false, None, MediaState::default(), None);
            Some(self.generation)
        } else {
            shared.update_media(false, false, None, MediaState::default(), None);
            None
        }
    }

    fn apply_event(&mut self, event: MediaWorkerEvent, now: Instant, shared: &HostIntegrations) {
        if !self.enabled || event.generation() != self.generation {
            return;
        }

        self.last_event_at = Some(now);
        match event {
            MediaWorkerEvent::Candidates { candidates, .. } => {
                let selected = self.bridge.update(true, &candidates);
                let player = selected.as_ref().map(|candidate| candidate.bus_name.clone());
                self.available = true;
                shared.update_media(
                    true,
                    true,
                    player,
                    media::to_renderer_state(selected.as_ref()),
                    None,
                );
            }
            MediaWorkerEvent::Error { error, .. } => {
                self.bridge.update(false, &[]);
                self.available = false;
                shared.update_media(true, false, None, MediaState::default(), Some(error));
            }
        }
    }

    fn expire_if_stale(&mut self, now: Instant, stale_after: Duration, shared: &HostIntegrations) {
        let Some(last_event_at) = self.last_event_at else {
            return;
        };
        if !self.enabled
            || !self.available
            || now.saturating_duration_since(last_event_at) <= stale_after
        {
            return;
        }

        self.bridge.update(false, &[]);
        self.available = false;
        shared.update_media(
            true,
            false,
            None,
            MediaState::default(),
            Some("MPRIS worker unresponsive".to_string()),
        );
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AudioWorkerSpec {
    source: String,
    sample_rate: u32,
    update_hz: u32,
}

impl AudioWorkerSpec {
    fn from_config(config: &IntegrationsConfig) -> Self {
        Self {
            source: config.audio_source.clone(),
            sample_rate: config.audio_sample_rate.clamp(8_000, 192_000),
            update_hz: config.audio_update_hz.clamp(5, 60),
        }
    }
}

#[derive(Debug)]
enum AudioWorkerEvent {
    Samples { generation: u64, samples: Arc<[f32]> },
    Error { generation: u64, error: String },
}

impl AudioWorkerEvent {
    fn generation(&self) -> u64 {
        match self {
            Self::Samples { generation, .. } | Self::Error { generation, .. } => *generation,
        }
    }
}

#[derive(Default)]
struct AudioSupervisorState {
    generation: u64,
    active: Option<AudioWorkerSpec>,
    available: bool,
    last_event_at: Option<Instant>,
}

impl AudioSupervisorState {
    fn reconcile(
        &mut self,
        config: &IntegrationsConfig,
        now: Instant,
        shared: &HostIntegrations,
    ) -> Option<(u64, AudioWorkerSpec)> {
        if !config.audio_spectrum {
            if self.active.take().is_some() {
                self.generation = self.generation.wrapping_add(1);
            }
            self.available = false;
            self.last_event_at = None;
            shared.update_audio(false, false, config.audio_source.clone(), None, None);
            return None;
        }

        let spec = AudioWorkerSpec::from_config(config);
        if self.active.as_ref() == Some(&spec) {
            return None;
        }

        self.generation = self.generation.wrapping_add(1);
        self.active = Some(spec.clone());
        self.available = false;
        self.last_event_at = Some(now);
        shared.update_audio(true, false, spec.source.clone(), None, None);
        Some((self.generation, spec))
    }

    fn apply_event(&mut self, event: AudioWorkerEvent, now: Instant, shared: &HostIntegrations) {
        if event.generation() != self.generation {
            return;
        }
        let Some(spec) = self.active.as_ref() else {
            return;
        };
        let source = spec.source.clone();
        self.last_event_at = Some(now);
        match event {
            AudioWorkerEvent::Samples { samples, .. } => {
                self.available = true;
                shared.update_audio(true, true, source, Some(samples), None);
            }
            AudioWorkerEvent::Error { error, .. } => {
                self.available = false;
                shared.update_audio(true, false, source, None, Some(error));
            }
        }
    }

    fn expire_if_stale(&mut self, now: Instant, stale_after: Duration, shared: &HostIntegrations) {
        let (Some(spec), Some(last_event_at)) = (self.active.as_ref(), self.last_event_at) else {
            return;
        };
        if !self.available || now.saturating_duration_since(last_event_at) <= stale_after {
            return;
        }

        self.available = false;
        shared.update_audio(
            true,
            false,
            spec.source.clone(),
            None,
            Some("audio worker unresponsive".to_string()),
        );
    }
}

fn blocking_worker_cancelled(shutdown: &AtomicBool, cancel: &AtomicBool) -> bool {
    shutdown.load(Ordering::Relaxed) || cancel.load(Ordering::Relaxed)
}

fn blocking_worker_sleep(duration: Duration, shutdown: &AtomicBool, cancel: &AtomicBool) -> bool {
    let deadline = Instant::now() + duration;
    loop {
        if blocking_worker_cancelled(shutdown, cancel) {
            return false;
        }
        let now = Instant::now();
        if now >= deadline {
            return true;
        }
        thread::sleep((deadline - now).min(Duration::from_millis(50)));
    }
}

fn spawn_media_io_worker(
    generation: u64,
    events: mpsc::Sender<MediaWorkerEvent>,
    shutdown: Arc<AtomicBool>,
    cancel: Arc<AtomicBool>,
) {
    thread::Builder::new()
        .name(format!("we-layerd-mpris-io-{generation}"))
        .spawn(move || {
            let mut connection = None;
            while !blocking_worker_cancelled(&shutdown, &cancel) {
                if connection.is_none() {
                    match media::session_connection() {
                        Ok(value) => connection = Some(value),
                        Err(error) => {
                            if events
                                .send(MediaWorkerEvent::Error {
                                    generation,
                                    error: error.to_string(),
                                })
                                .is_err()
                            {
                                return;
                            }
                            if !blocking_worker_sleep(Duration::from_secs(2), &shutdown, &cancel) {
                                return;
                            }
                            continue;
                        }
                    }
                }

                let result = media::read_mpris_candidates(connection.as_ref().expect("connection"));
                if blocking_worker_cancelled(&shutdown, &cancel) {
                    return;
                }
                let event = match result {
                    Ok(candidates) => MediaWorkerEvent::Candidates { generation, candidates },
                    Err(error) => {
                        connection = None;
                        MediaWorkerEvent::Error { generation, error: error.to_string() }
                    }
                };
                if events.send(event).is_err()
                    || !blocking_worker_sleep(Duration::from_millis(500), &shutdown, &cancel)
                {
                    return;
                }
            }
        })
        .expect("failed to spawn MPRIS I/O worker");
}

fn spawn_audio_io_worker(
    generation: u64,
    spec: AudioWorkerSpec,
    events: mpsc::Sender<AudioWorkerEvent>,
    shutdown: Arc<AtomicBool>,
    cancel: Arc<AtomicBool>,
) {
    thread::Builder::new()
        .name(format!("we-layerd-audio-io-{generation}"))
        .spawn(move || {
            while !blocking_worker_cancelled(&shutdown, &cancel) {
                let mut capture = match PulseAudioCapture::connect(
                    &spec.source,
                    spec.sample_rate,
                    spec.update_hz,
                ) {
                    Ok(capture) => capture,
                    Err(error) => {
                        if events
                            .send(AudioWorkerEvent::Error { generation, error: error.to_string() })
                            .is_err()
                        {
                            return;
                        }
                        if !blocking_worker_sleep(Duration::from_secs(2), &shutdown, &cancel) {
                            return;
                        }
                        continue;
                    }
                };

                loop {
                    if blocking_worker_cancelled(&shutdown, &cancel) {
                        return;
                    }
                    match capture.read_spectrum() {
                        Ok(samples) => {
                            if blocking_worker_cancelled(&shutdown, &cancel) {
                                return;
                            }
                            if events
                                .send(AudioWorkerEvent::Samples { generation, samples })
                                .is_err()
                            {
                                return;
                            }
                        }
                        Err(error) => {
                            if events
                                .send(AudioWorkerEvent::Error {
                                    generation,
                                    error: error.to_string(),
                                })
                                .is_err()
                            {
                                return;
                            }
                            break;
                        }
                    }
                }

                if !blocking_worker_sleep(Duration::from_secs(1), &shutdown, &cancel) {
                    return;
                }
            }
        })
        .expect("failed to spawn audio spectrum I/O worker");
}

fn spawn_media_collector(
    shared: HostIntegrations,
    desired_cfg: Arc<Mutex<Config>>,
    shutdown: Arc<AtomicBool>,
) -> thread::JoinHandle<()> {
    thread::Builder::new()
        .name("we-layerd-mpris".to_string())
        .spawn(move || {
            let (event_tx, event_rx) = mpsc::channel::<MediaWorkerEvent>();
            let mut supervisor = MediaSupervisorState::default();
            let mut worker_cancel: Option<Arc<AtomicBool>> = None;
            while !shutdown.load(Ordering::Relaxed) {
                let enabled =
                    desired_cfg.lock().map(|config| config.integrations.media).unwrap_or(false);
                let previous_generation = supervisor.generation;
                let start_generation = supervisor.reconcile(enabled, Instant::now(), &shared);
                if supervisor.generation != previous_generation {
                    if let Some(cancel) = worker_cancel.take() {
                        cancel.store(true, Ordering::Relaxed);
                    }
                }
                if let Some(generation) = start_generation {
                    let cancel = Arc::new(AtomicBool::new(false));
                    spawn_media_io_worker(
                        generation,
                        event_tx.clone(),
                        shutdown.clone(),
                        cancel.clone(),
                    );
                    worker_cancel = Some(cancel);
                }

                if let Ok(event) = event_rx.recv_timeout(INTEGRATION_SUPERVISOR_POLL) {
                    supervisor.apply_event(event, Instant::now(), &shared);
                    while let Ok(event) = event_rx.try_recv() {
                        supervisor.apply_event(event, Instant::now(), &shared);
                    }
                }
                supervisor.expire_if_stale(Instant::now(), MEDIA_STALE_AFTER, &shared);
            }
            if let Some(cancel) = worker_cancel {
                cancel.store(true, Ordering::Relaxed);
            }
        })
        .expect("failed to spawn MPRIS collector")
}

fn spawn_audio_collector(
    shared: HostIntegrations,
    desired_cfg: Arc<Mutex<Config>>,
    shutdown: Arc<AtomicBool>,
) -> thread::JoinHandle<()> {
    thread::Builder::new()
        .name("we-layerd-audio-spectrum".to_string())
        .spawn(move || {
            let (event_tx, event_rx) = mpsc::channel::<AudioWorkerEvent>();
            let mut supervisor = AudioSupervisorState::default();
            let mut worker_cancel: Option<Arc<AtomicBool>> = None;
            while !shutdown.load(Ordering::Relaxed) {
                let integration = desired_cfg
                    .lock()
                    .map(|config| config.integrations.clone())
                    .unwrap_or_default();
                let previous_generation = supervisor.generation;
                let start = supervisor.reconcile(&integration, Instant::now(), &shared);
                if supervisor.generation != previous_generation {
                    if let Some(cancel) = worker_cancel.take() {
                        cancel.store(true, Ordering::Relaxed);
                    }
                }
                if let Some((generation, spec)) = start {
                    let cancel = Arc::new(AtomicBool::new(false));
                    spawn_audio_io_worker(
                        generation,
                        spec,
                        event_tx.clone(),
                        shutdown.clone(),
                        cancel.clone(),
                    );
                    worker_cancel = Some(cancel);
                }

                if let Ok(event) = event_rx.recv_timeout(INTEGRATION_SUPERVISOR_POLL) {
                    supervisor.apply_event(event, Instant::now(), &shared);
                    while let Ok(event) = event_rx.try_recv() {
                        supervisor.apply_event(event, Instant::now(), &shared);
                    }
                }
                supervisor.expire_if_stale(Instant::now(), AUDIO_STALE_AFTER, &shared);
            }
            if let Some(cancel) = worker_cancel {
                cancel.store(true, Ordering::Relaxed);
            }
        })
        .expect("failed to spawn audio spectrum collector")
}

fn spawn_rule_collector(
    shared: HostIntegrations,
    desired_cfg: Arc<Mutex<Config>>,
    shutdown: Arc<AtomicBool>,
) -> thread::JoinHandle<()> {
    thread::Builder::new()
        .name("we-layerd-window-rules".to_string())
        .spawn(move || {
            let mut monitor = None;
            while !shutdown.load(Ordering::Relaxed) {
                let rules = desired_cfg
                    .lock()
                    .map(|config| RuleSet::from(config.rules))
                    .unwrap_or_default();
                if rules.is_keep() {
                    monitor = None;
                    shared.update_rules(false, false, BTreeMap::new(), None);
                    thread::sleep(Duration::from_millis(250));
                    continue;
                }

                if monitor.is_none() {
                    match ForeignToplevelMonitor::connect() {
                        Ok(value) => monitor = Some(value),
                        Err(error) => {
                            shared.update_rules(
                                true,
                                false,
                                BTreeMap::new(),
                                Some(error.to_string()),
                            );
                            thread::sleep(Duration::from_secs(2));
                            continue;
                        }
                    }
                }

                let result = monitor.as_mut().expect("monitor").poll(Duration::from_millis(250));
                match result {
                    Ok((outputs, toplevels)) => {
                        shared.update_rules(
                            true,
                            true,
                            policies_for_outputs(rules, outputs, &toplevels),
                            None,
                        );
                    }
                    Err(error) => {
                        warn!(%error, "window rule collector disconnected");
                        monitor = None;
                        shared.update_rules(true, false, BTreeMap::new(), Some(error.to_string()));
                        thread::sleep(Duration::from_secs(1));
                    }
                }
            }
        })
        .expect("failed to spawn window rule collector")
}

#[cfg(test)]
mod tests {
    use std::{
        sync::{atomic::AtomicBool, mpsc, Arc},
        thread,
        time::{Duration, Instant},
    };

    use super::{
        AudioSupervisorState, AudioWorkerEvent, HostIntegrationRuntime, HostIntegrations,
        MediaSupervisorState, MediaWorkerEvent, AUDIO_SPECTRUM_BINS,
    };
    use crate::runtime::{
        media::{MediaCandidate, MediaPlaybackState},
        rules::RuntimePolicy,
    };
    use we_core::config::IntegrationsConfig;

    fn media_candidate(name: &str) -> MediaCandidate {
        MediaCandidate {
            bus_name: format!("org.mpris.MediaPlayer2.{name}"),
            playback: MediaPlaybackState::Playing,
            title: name.to_string(),
            artist: String::new(),
            album_title: String::new(),
            album_artist: String::new(),
            genres: String::new(),
        }
    }

    #[test]
    fn output_snapshot_uses_only_that_outputs_rule_policy() {
        let host = HostIntegrations::default();
        host.update_rules(
            true,
            true,
            [
                ("DP-1".to_string(), RuntimePolicy { pause: true, mute: false }),
                ("HDMI-A-1".to_string(), RuntimePolicy { pause: false, mute: true }),
            ]
            .into_iter()
            .collect(),
            None,
        );
        host.update_audio(
            true,
            true,
            "monitor".to_string(),
            Some(Arc::from(vec![0.5_f32; AUDIO_SPECTRUM_BINS * 2])),
            None,
        );

        let dp = host.snapshot_for_output("DP-1");
        let hdmi = host.snapshot_for_output("HDMI-A-1");
        assert!(dp.policy.pause);
        assert!(!dp.policy.mute);
        assert!(!hdmi.policy.pause);
        assert!(hdmi.policy.mute);
        assert_eq!(dp.audio_generation, hdmi.audio_generation);
    }

    #[test]
    fn unavailable_audio_replaces_previous_spectrum_with_silence() {
        let host = HostIntegrations::default();
        host.update_audio(
            true,
            true,
            "monitor".to_string(),
            Some(Arc::from(vec![0.5_f32; AUDIO_SPECTRUM_BINS * 2])),
            None,
        );
        let before = host.snapshot_for_output("DP-1");

        host.update_audio(
            true,
            false,
            "monitor".to_string(),
            None,
            Some("capture disconnected".to_string()),
        );
        let after = host.snapshot_for_output("DP-1");

        assert!(after.audio.iter().all(|sample| *sample == 0.0));
        assert_ne!(after.audio_generation, before.audio_generation);
    }

    #[test]
    fn media_reconfigure_clears_state_and_rejects_stale_worker_results() {
        let host = HostIntegrations::default();
        let mut supervisor = MediaSupervisorState::default();
        let now = Instant::now();
        let first_generation = supervisor
            .reconcile(true, now, &host)
            .expect("enabling media starts a worker generation");
        supervisor.apply_event(
            MediaWorkerEvent::Candidates {
                generation: first_generation,
                candidates: vec![media_candidate("old")],
            },
            now,
            &host,
        );
        assert_eq!(host.snapshot_for_output("DP-1").media.title, "old");

        assert!(supervisor.reconcile(false, now + Duration::from_millis(1), &host).is_none());
        assert!(host.snapshot_for_output("DP-1").media.title.is_empty());
        let second_generation = supervisor
            .reconcile(true, now + Duration::from_millis(2), &host)
            .expect("re-enabling media starts a fresh worker generation");
        assert_ne!(first_generation, second_generation);

        supervisor.apply_event(
            MediaWorkerEvent::Candidates {
                generation: first_generation,
                candidates: vec![media_candidate("stale")],
            },
            now + Duration::from_millis(3),
            &host,
        );
        assert!(host.snapshot_for_output("DP-1").media.title.is_empty());

        supervisor.apply_event(
            MediaWorkerEvent::Candidates {
                generation: second_generation,
                candidates: vec![media_candidate("current")],
            },
            now + Duration::from_millis(4),
            &host,
        );
        assert_eq!(host.snapshot_for_output("DP-1").media.title, "current");
        supervisor.expire_if_stale(now + Duration::from_secs(10), Duration::from_secs(2), &host);
        assert!(host.snapshot_for_output("DP-1").media.title.is_empty());
    }

    #[test]
    fn audio_reconfigure_clears_state_and_rejects_stale_worker_results() {
        let host = HostIntegrations::default();
        let mut supervisor = AudioSupervisorState::default();
        let now = Instant::now();
        let mut integration = IntegrationsConfig {
            audio_spectrum: true,
            audio_source: "monitor-a".to_string(),
            ..IntegrationsConfig::default()
        };
        let (first_generation, first_spec) = supervisor
            .reconcile(&integration, now, &host)
            .expect("enabling audio starts a worker generation");
        assert_eq!(first_spec.source, "monitor-a");
        supervisor.apply_event(
            AudioWorkerEvent::Samples {
                generation: first_generation,
                samples: Arc::from(vec![0.5_f32; AUDIO_SPECTRUM_BINS * 2]),
            },
            now,
            &host,
        );
        assert!(host.snapshot_for_output("DP-1").audio.iter().any(|sample| *sample > 0.0));

        integration.audio_source = "monitor-b".to_string();
        let (second_generation, second_spec) = supervisor
            .reconcile(&integration, now + Duration::from_millis(1), &host)
            .expect("changing source starts a fresh worker generation");
        assert_eq!(second_spec.source, "monitor-b");
        assert_ne!(first_generation, second_generation);
        assert!(host.snapshot_for_output("DP-1").audio.iter().all(|sample| *sample == 0.0));

        supervisor.apply_event(
            AudioWorkerEvent::Samples {
                generation: first_generation,
                samples: Arc::from(vec![0.9_f32; AUDIO_SPECTRUM_BINS * 2]),
            },
            now + Duration::from_millis(2),
            &host,
        );
        assert!(host.snapshot_for_output("DP-1").audio.iter().all(|sample| *sample == 0.0));

        supervisor.apply_event(
            AudioWorkerEvent::Samples {
                generation: second_generation,
                samples: Arc::from(vec![0.25_f32; AUDIO_SPECTRUM_BINS * 2]),
            },
            now + Duration::from_millis(3),
            &host,
        );
        assert!(host.snapshot_for_output("DP-1").audio.iter().any(|sample| *sample > 0.0));
        supervisor.expire_if_stale(now + Duration::from_secs(10), Duration::from_secs(2), &host);
        assert!(host.snapshot_for_output("DP-1").audio.iter().all(|sample| *sample == 0.0));
    }

    #[test]
    fn host_runtime_shutdown_does_not_wait_for_a_blocked_collector() {
        let (release_tx, release_rx) = mpsc::channel::<()>();
        let collector = thread::spawn(move || {
            let _ = release_rx.recv();
        });
        let shutdown_requested = Arc::new(AtomicBool::new(false));
        let runtime = HostIntegrationRuntime {
            shared: HostIntegrations::default(),
            handles: vec![collector],
            shutdown: shutdown_requested.clone(),
        };
        let (done_tx, done_rx) = mpsc::channel();
        let shutdown = thread::spawn(move || {
            runtime.shutdown();
            let _ = done_tx.send(());
        });

        let returned_without_collector = done_rx.recv_timeout(Duration::from_millis(500)).is_ok();
        let _ = release_tx.send(());
        let _ = shutdown.join();

        assert!(
            returned_without_collector,
            "runtime shutdown must not depend on a blocking host collector returning"
        );
        assert!(shutdown_requested.load(std::sync::atomic::Ordering::Relaxed));
    }

    #[test]
    fn runtime_status_can_be_appended_to_config_without_duplicate_tables() {
        let host = HostIntegrations::default();
        let config = crate::config::Config::default().to_toml_pretty().expect("serialize config");
        let combined = format!("{config}\n{}", host.render_status_toml());
        let parsed =
            toml::from_str::<toml::Value>(&combined).expect("combined status is valid TOML");
        assert!(parsed.get("integrations").is_some());
        assert!(parsed.get("integration_runtime").is_some());
    }
}
