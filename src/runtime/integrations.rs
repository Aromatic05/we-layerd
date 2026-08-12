use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread,
    time::Duration,
};

use serde::Serialize;
use tracing::warn;
use we_renderer::MediaState;

use crate::config::Config;

use super::{
    audio::{PulseAudioCapture, AUDIO_SPECTRUM_BINS},
    media::{self, MediaBridgeState},
    rules::{policies_for_outputs, ForeignToplevelMonitor, RuleSet, RuntimePolicy},
};

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

fn spawn_media_collector(
    shared: HostIntegrations,
    desired_cfg: Arc<Mutex<Config>>,
    shutdown: Arc<AtomicBool>,
) -> thread::JoinHandle<()> {
    thread::Builder::new()
        .name("we-layerd-mpris".to_string())
        .spawn(move || {
            let mut connection = None;
            let mut bridge = MediaBridgeState::default();
            while !shutdown.load(Ordering::Relaxed) {
                let enabled =
                    desired_cfg.lock().map(|config| config.integrations.media).unwrap_or(false);
                if !enabled {
                    bridge.update(false, &[]);
                    shared.update_media(false, false, None, MediaState::default(), None);
                    thread::sleep(Duration::from_millis(500));
                    continue;
                }

                if connection.is_none() {
                    match media::session_connection() {
                        Ok(value) => connection = Some(value),
                        Err(error) => {
                            shared.update_media(
                                true,
                                false,
                                None,
                                MediaState::default(),
                                Some(error.to_string()),
                            );
                            thread::sleep(Duration::from_secs(2));
                            continue;
                        }
                    }
                }

                let result = media::read_mpris_candidates(connection.as_ref().expect("connection"));
                match result {
                    Ok(candidates) => {
                        let selected = bridge.update(true, &candidates);
                        let player = selected.as_ref().map(|candidate| candidate.bus_name.clone());
                        shared.update_media(
                            true,
                            true,
                            player,
                            media::to_renderer_state(selected.as_ref()),
                            None,
                        );
                    }
                    Err(error) => {
                        connection = None;
                        bridge.update(false, &[]);
                        shared.update_media(
                            true,
                            false,
                            None,
                            MediaState::default(),
                            Some(error.to_string()),
                        );
                    }
                }
                thread::sleep(Duration::from_millis(500));
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
            let mut capture: Option<(String, u32, u32, PulseAudioCapture)> = None;
            let zero: Arc<[f32]> = vec![0.0; AUDIO_SPECTRUM_BINS * 2].into();
            let mut was_enabled = false;
            while !shutdown.load(Ordering::Relaxed) {
                let integration = desired_cfg
                    .lock()
                    .map(|config| config.integrations.clone())
                    .unwrap_or_default();
                if !integration.audio_spectrum {
                    capture = None;
                    if was_enabled {
                        shared.update_audio(
                            false,
                            false,
                            integration.audio_source.clone(),
                            Some(Arc::clone(&zero)),
                            None,
                        );
                    } else {
                        shared.update_audio(
                            false,
                            false,
                            integration.audio_source.clone(),
                            None,
                            None,
                        );
                    }
                    was_enabled = false;
                    thread::sleep(Duration::from_millis(250));
                    continue;
                }
                was_enabled = true;

                let needs_open = match capture.as_ref() {
                    None => true,
                    Some((source, rate, hz, _)) => {
                        source != &integration.audio_source
                            || *rate != integration.audio_sample_rate
                            || *hz != integration.audio_update_hz
                    }
                };
                if needs_open {
                    capture = match PulseAudioCapture::connect(
                        &integration.audio_source,
                        integration.audio_sample_rate,
                        integration.audio_update_hz,
                    ) {
                        Ok(value) => Some((
                            integration.audio_source.clone(),
                            integration.audio_sample_rate,
                            integration.audio_update_hz,
                            value,
                        )),
                        Err(error) => {
                            shared.update_audio(
                                true,
                                false,
                                integration.audio_source.clone(),
                                None,
                                Some(error.to_string()),
                            );
                            thread::sleep(Duration::from_secs(2));
                            continue;
                        }
                    };
                }

                let Some((source, _, _, capture_stream)) = capture.as_mut() else {
                    continue;
                };
                match capture_stream.read_spectrum() {
                    Ok(samples) => {
                        shared.update_audio(true, true, source.clone(), Some(samples), None);
                    }
                    Err(error) => {
                        shared.update_audio(
                            true,
                            false,
                            source.clone(),
                            None,
                            Some(error.to_string()),
                        );
                        capture = None;
                        thread::sleep(Duration::from_secs(1));
                    }
                }
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
        time::Duration,
    };

    use super::{HostIntegrationRuntime, HostIntegrations, AUDIO_SPECTRUM_BINS};
    use crate::runtime::rules::RuntimePolicy;

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
