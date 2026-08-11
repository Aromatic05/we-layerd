use std::{
    collections::BTreeMap,
    path::Path,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc, Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};

use anyhow::{bail, Context, Result};
use tracing::{info, warn};
use we_core::config::OutputBinding;

use crate::{
    backend::{layer_shell::event_loop, traits::BackendContext, wayland_common::outputs},
    config::Config,
    ipc::{ControlCommand, OutputPlaylistAction, OutputPlaylistRequest, RuntimeLoopExit},
    runtime::{
        playlist::{self, AdvanceDirection, PlaylistRuntime, PlaylistSelection},
        status::RuntimeStatusSnapshot,
    },
};

static WORKER_INSTANCE_SEQUENCE: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone)]
pub(crate) struct OutputSpec {
    pub(crate) worker_id: String,
    pub(crate) output_name: String,
    pub(crate) config: Config,
    pub(crate) binding: OutputBinding,
    pub(crate) fingerprint: String,
}

impl OutputSpec {
    pub(crate) fn playlist_name(&self) -> Option<&str> {
        self.binding.playlist.as_deref()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum OutputAction {
    Start(String),
    Restart(String),
    Stop(String),
}

pub(crate) fn build_output_specs(
    base: &Config,
    discovered_outputs: &[String],
) -> Result<BTreeMap<String, OutputSpec>> {
    let mut specs = BTreeMap::new();
    for output_name in discovered_outputs {
        let binding = base.outputs.get(output_name).cloned().unwrap_or_default();
        if binding == OutputBinding::default() && base.renderer.source.trim().is_empty() {
            continue;
        }
        validate_binding(output_name, &binding)?;
        if let Some(playlist_name) = binding.playlist.as_deref() {
            if !base.playlists.definitions.contains_key(playlist_name) {
                bail!("output '{output_name}' references missing playlist '{playlist_name}'");
            }
        }
        let mut config = base.clone();

        if let (Some(wallpaper_id), Some(source)) =
            (binding.wallpaper_id.as_ref(), binding.source.as_ref())
        {
            playlist::apply_selection_to_config(
                &mut config,
                &PlaylistSelection {
                    index: 0,
                    wallpaper_id: wallpaper_id.clone(),
                    source: source.clone(),
                },
            )?;
        }

        // The worker owns playlist progression. The global playlist marker must not make two
        // output workers accidentally share one cursor.
        config.playlists.active = binding.playlist.clone();
        let fingerprint = fingerprint_output_config(&config, &binding)?;
        specs.insert(
            output_name.clone(),
            OutputSpec {
                worker_id: output_name.clone(),
                output_name: output_name.clone(),
                config,
                binding,
                fingerprint,
            },
        );
    }
    Ok(specs)
}

fn fingerprint_output_config(config: &Config, binding: &OutputBinding) -> Result<String> {
    let mut effective = config.clone();
    effective.outputs.clear();
    effective.gnome = Default::default();
    effective.hooks = Default::default();

    if let Some(playlist_name) = binding.playlist.as_deref() {
        effective.playlists.definitions.retain(|name, _| name == playlist_name);
        let wallpaper_ids = effective
            .playlists
            .definitions
            .get(playlist_name)
            .map(|playlist| {
                playlist
                    .items
                    .iter()
                    .map(|item| item.wallpaper_id.as_str())
                    .collect::<std::collections::BTreeSet<_>>()
            })
            .unwrap_or_default();
        effective
            .wallpapers
            .retain(|wallpaper_id, _| wallpaper_ids.contains(wallpaper_id.as_str()));
    } else {
        effective.playlists = Default::default();
        effective.wallpapers.clear();
    }

    toml::to_string(&effective).context("failed to fingerprint effective output config")
}

pub(crate) fn reconcile_workers(
    current: &BTreeMap<String, String>,
    desired: &BTreeMap<String, OutputSpec>,
) -> Vec<OutputAction> {
    let mut actions = Vec::new();
    for output in current.keys() {
        if !desired.contains_key(output) {
            actions.push(OutputAction::Stop(output.clone()));
        }
    }
    for (output, spec) in desired {
        match current.get(output) {
            None => actions.push(OutputAction::Start(output.clone())),
            Some(fingerprint) if fingerprint != &spec.fingerprint => {
                actions.push(OutputAction::Restart(output.clone()))
            }
            Some(_) => {}
        }
    }
    actions
}

fn validate_binding(output_name: &str, binding: &OutputBinding) -> Result<()> {
    if binding.is_ambiguous() {
        bail!("output '{output_name}' cannot bind a wallpaper and playlist at the same time");
    }
    if binding.playlist.is_none() && binding.wallpaper_id.is_some() != binding.source.is_some() {
        bail!("output '{output_name}' wallpaper binding requires both wallpaper_id and source");
    }
    Ok(())
}

struct OutputWorker {
    instance_id: u64,
    fingerprint: String,
    control_tx: mpsc::Sender<ControlCommand>,
    desired_cfg: Arc<Mutex<Config>>,
    playlist_runtime: Arc<Mutex<Option<PlaylistRuntime>>>,
    stop_scheduler: Arc<AtomicBool>,
    handle: Option<thread::JoinHandle<()>>,
}

enum WorkerEvent {
    Status { instance_id: u64, snapshot: RuntimeStatusSnapshot },
    Exited { output: String, instance_id: u64, error: Option<String> },
}

pub(crate) fn run(mut ctx: BackendContext<'_>) -> Result<RuntimeLoopExit> {
    let (worker_event_tx, worker_event_rx) = mpsc::channel::<WorkerEvent>();
    let mut workers = BTreeMap::<String, OutputWorker>::new();
    let mut last_discovery = Instant::now() - Duration::from_secs(2);

    reconcile_live_workers(&mut ctx, &mut workers, &worker_event_tx)?;

    loop {
        if ctx.shutdown_requested.load(Ordering::Relaxed) {
            stop_all_workers(&mut workers);
            return Ok(RuntimeLoopExit::Stop);
        }

        while let Ok(command) = ctx.control_rx.try_recv() {
            match command {
                ControlCommand::Stop => {
                    stop_all_workers(&mut workers);
                    return Ok(RuntimeLoopExit::Stop);
                }
                ControlCommand::Pause | ControlCommand::Resume => {
                    let now = Instant::now();
                    for worker in workers.values_mut() {
                        if let Ok(mut runtime) = worker.playlist_runtime.lock() {
                            if let Some(runtime) = runtime.as_mut() {
                                match command {
                                    ControlCommand::Pause => runtime.pause(now),
                                    ControlCommand::Resume => runtime.resume(now),
                                    _ => unreachable!(),
                                }
                            }
                        }
                        let _ = worker.control_tx.send(command);
                    }
                }
                ControlCommand::Reload => {
                    for worker in workers.values() {
                        let _ = worker.control_tx.send(ControlCommand::Reload);
                    }
                }
                ControlCommand::Reconfigure => {
                    reconcile_live_workers(&mut ctx, &mut workers, &worker_event_tx)?;
                }
            }
        }

        if let Some(output_playlist_rx) = ctx.output_playlist_rx {
            while let Ok(request) = output_playlist_rx.try_recv() {
                let result = handle_output_playlist_request(&request, &mut workers);
                let _ = request.reply.send(result.map_err(|error| error.to_string()));
            }
        }

        while let Ok(event) = worker_event_rx.try_recv() {
            match event {
                WorkerEvent::Status { instance_id, snapshot } => {
                    let current_instance = workers
                        .get(&snapshot.output_name)
                        .is_some_and(|worker| worker.instance_id == instance_id);
                    if current_instance {
                        (ctx.status_sink)(snapshot);
                    }
                }
                WorkerEvent::Exited { output, instance_id, error } => {
                    let current_instance = workers
                        .get(&output)
                        .is_some_and(|worker| worker.instance_id == instance_id);
                    if !current_instance {
                        continue;
                    }
                    if let Some(error) = error {
                        warn!(
                            output,
                            error, "output runtime failed without stopping other outputs"
                        );
                        let mut snapshot = RuntimeStatusSnapshot {
                            output_name: output.clone(),
                            ..RuntimeStatusSnapshot::default()
                        };
                        snapshot.frame_stats.last_error = Some(error);
                        (ctx.status_sink)(snapshot);
                    }
                    if let Some(worker) = workers.get_mut(&output) {
                        worker.handle.take();
                    }
                }
            }
        }

        if last_discovery.elapsed() >= Duration::from_secs(1) {
            last_discovery = Instant::now();
            if let Err(error) = reconcile_live_workers(&mut ctx, &mut workers, &worker_event_tx) {
                warn!(%error, "failed to reconcile layer-shell outputs; keeping existing workers");
            }
        }

        thread::sleep(Duration::from_millis(25));
    }
}

fn reconcile_live_workers(
    ctx: &mut BackendContext<'_>,
    workers: &mut BTreeMap<String, OutputWorker>,
    worker_event_tx: &mpsc::Sender<WorkerEvent>,
) -> Result<()> {
    let discovered = outputs::list_output_names()?;
    let desired_config = ctx
        .desired_cfg
        .lock()
        .map_err(|_| anyhow::anyhow!("desired config lock poisoned"))?
        .clone();
    let desired = build_output_specs(&desired_config, &discovered)?;
    let current = workers
        .iter()
        .map(|(name, worker)| (name.clone(), worker.fingerprint.clone()))
        .collect::<BTreeMap<_, _>>();

    for action in reconcile_workers(&current, &desired) {
        match action {
            OutputAction::Stop(output) => {
                stop_worker(workers, &output);
                (ctx.status_sink)(RuntimeStatusSnapshot::removed_output(output));
            }
            OutputAction::Restart(output) => {
                stop_worker(workers, &output);
                if let Some(spec) = desired.get(&output).cloned() {
                    let worker = spawn_worker(
                        spec,
                        ctx.shutdown_requested.clone(),
                        worker_event_tx.clone(),
                    )?;
                    workers.insert(output, worker);
                }
            }
            OutputAction::Start(output) => {
                if let Some(spec) = desired.get(&output).cloned() {
                    let worker = spawn_worker(
                        spec,
                        ctx.shutdown_requested.clone(),
                        worker_event_tx.clone(),
                    )?;
                    workers.insert(output, worker);
                }
            }
        }
    }
    Ok(())
}

fn spawn_worker(
    spec: OutputSpec,
    shutdown_requested: Arc<AtomicBool>,
    worker_event_tx: mpsc::Sender<WorkerEvent>,
) -> Result<OutputWorker> {
    let output_name = spec.output_name.clone();
    let instance_id = WORKER_INSTANCE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let mut initial_config = spec.config.clone();
    let playlist_runtime = prepare_output_playlist(&spec, &mut initial_config)?;
    let playlist_runtime = Arc::new(Mutex::new(playlist_runtime));
    let desired_cfg = Arc::new(Mutex::new(initial_config));
    let (control_tx, control_rx) = mpsc::channel::<ControlCommand>();
    let stop_scheduler = Arc::new(AtomicBool::new(false));

    if spec.playlist_name().is_some() {
        spawn_playlist_scheduler(
            output_name.clone(),
            playlist_runtime.clone(),
            desired_cfg.clone(),
            control_tx.clone(),
            stop_scheduler.clone(),
            shutdown_requested.clone(),
        );
    }

    let worker_desired_cfg = desired_cfg.clone();
    let worker_playlist_runtime = playlist_runtime.clone();
    let worker_stop_scheduler = stop_scheduler.clone();
    let worker_output = output_name.clone();
    let handle = thread::Builder::new()
        .name(format!("we-layerd-output-{output_name}"))
        .spawn(move || {
            info!(output = %worker_output, "starting output runtime worker");
            let result = loop {
                if shutdown_requested.load(Ordering::Relaxed) {
                    break Ok(RuntimeLoopExit::Stop);
                }
                let config = match worker_desired_cfg.lock() {
                    Ok(config) => config.clone(),
                    Err(_) => break Err(anyhow::anyhow!("output desired config lock poisoned")),
                };
                let status_source = config.renderer.source.clone();
                let status_playlist_runtime = worker_playlist_runtime.clone();
                let mut sink = |mut snapshot: RuntimeStatusSnapshot| {
                    snapshot.output_source = status_source.clone();
                    if let Ok(runtime) = status_playlist_runtime.lock() {
                        if let Some(runtime) = runtime.as_ref() {
                            snapshot.output_playlist_active =
                                runtime.active_name().map(str::to_string);
                            snapshot.output_playlist_index =
                                runtime.current_selection().map(|selection| selection.index);
                        }
                    }
                    let _ = worker_event_tx.send(WorkerEvent::Status { instance_id, snapshot });
                };
                let worker_context = BackendContext {
                    cfg: &config,
                    desired_cfg: worker_desired_cfg.clone(),
                    shutdown_requested: shutdown_requested.clone(),
                    control_rx: &control_rx,
                    output_playlist_rx: None,
                    status_sink: &mut sink,
                };
                match event_loop::run_output(worker_context, &worker_output) {
                    Ok(RuntimeLoopExit::RestartCurrent | RuntimeLoopExit::Reconfigure) => continue,
                    Ok(RuntimeLoopExit::Stop) => break Ok(RuntimeLoopExit::Stop),
                    Err(error) => break Err(error),
                }
            };
            worker_stop_scheduler.store(true, Ordering::Relaxed);
            let error = result.err().map(|error| error.to_string());
            let _ = worker_event_tx.send(WorkerEvent::Exited {
                output: worker_output,
                instance_id,
                error,
            });
        })
        .context("failed to spawn output runtime worker")?;

    Ok(OutputWorker {
        instance_id,
        fingerprint: spec.fingerprint,
        control_tx,
        desired_cfg,
        playlist_runtime,
        stop_scheduler,
        handle: Some(handle),
    })
}

fn prepare_output_playlist(
    spec: &OutputSpec,
    config: &mut Config,
) -> Result<Option<PlaylistRuntime>> {
    let Some(playlist_name) = spec.playlist_name() else {
        config.playlists.active = None;
        return Ok(None);
    };
    if !config.playlists.definitions.contains_key(playlist_name) {
        bail!("output '{}' references missing playlist '{playlist_name}'", spec.output_name);
    }
    config.playlists.active = Some(playlist_name.to_string());
    let now = Instant::now();
    let mut runtime = PlaylistRuntime::new(config.playlists.clone(), playlist::random_seed());
    let selection = runtime
        .ensure_started(now)
        .filter(playlist_selection_is_available)
        .or_else(|| runtime.advance(AdvanceDirection::Next, now, playlist_item_is_available))
        .ok_or_else(|| {
            anyhow::anyhow!(
                "output '{}' playlist '{playlist_name}' has no playable wallpaper",
                spec.output_name
            )
        })?;
    playlist::apply_selection_to_config(config, &selection)?;
    Ok(Some(runtime))
}

fn spawn_playlist_scheduler(
    output_name: String,
    runtime: Arc<Mutex<Option<PlaylistRuntime>>>,
    desired_cfg: Arc<Mutex<Config>>,
    control_tx: mpsc::Sender<ControlCommand>,
    stop: Arc<AtomicBool>,
    shutdown: Arc<AtomicBool>,
) {
    thread::spawn(move || {
        while !stop.load(Ordering::Relaxed) && !shutdown.load(Ordering::Relaxed) {
            thread::sleep(Duration::from_millis(50));
            let selection = {
                let Ok(mut runtime) = runtime.lock() else {
                    break;
                };
                runtime.as_mut().and_then(|runtime| {
                    runtime.due_selection(Instant::now(), playlist_item_is_available)
                })
            };
            let Some(selection) = selection else {
                continue;
            };
            let update_result = desired_cfg
                .lock()
                .map_err(|_| anyhow::anyhow!("output desired config lock poisoned"))
                .and_then(|mut config| {
                    playlist::apply_selection_to_config(&mut config, &selection)
                });
            if let Err(error) = update_result {
                warn!(output = %output_name, %error, "failed to advance output playlist");
                continue;
            }
            if control_tx.send(ControlCommand::Reconfigure).is_err() {
                break;
            }
        }
    });
}

fn handle_output_playlist_request(
    request: &OutputPlaylistRequest,
    workers: &mut BTreeMap<String, OutputWorker>,
) -> Result<()> {
    let worker = workers
        .get_mut(&request.output)
        .ok_or_else(|| anyhow::anyhow!("output '{}' has no running worker", request.output))?;
    let mut runtime_guard = worker
        .playlist_runtime
        .lock()
        .map_err(|_| anyhow::anyhow!("output playlist runtime lock poisoned"))?;
    let runtime = runtime_guard
        .as_mut()
        .ok_or_else(|| anyhow::anyhow!("output '{}' is not bound to a playlist", request.output))?;
    let mut config = worker
        .desired_cfg
        .lock()
        .map_err(|_| anyhow::anyhow!("output desired config lock poisoned"))?;
    apply_output_playlist_action(
        runtime,
        &mut config,
        &request.action,
        Instant::now(),
        playlist_item_is_available,
    )?;
    drop(config);
    drop(runtime_guard);

    if !matches!(&request.action, OutputPlaylistAction::Stop) {
        worker.control_tx.send(ControlCommand::Reconfigure).with_context(|| {
            format!("output '{}' runtime is not accepting commands", request.output)
        })?;
    }
    Ok(())
}

fn apply_output_playlist_action<F>(
    runtime: &mut PlaylistRuntime,
    config: &mut Config,
    action: &OutputPlaylistAction,
    now: Instant,
    playable: F,
) -> Result<()>
where
    F: Fn(&we_core::playlist::PlaylistItem) -> bool + Copy,
{
    let selection = match action {
        OutputPlaylistAction::Play(name) => {
            let first = runtime.play(name, now).map_err(anyhow::Error::msg)?;
            let first_is_playable = config
                .playlists
                .definitions
                .get(name)
                .and_then(|playlist| playlist.items.get(first.index))
                .is_some_and(playable);
            if first_is_playable {
                Some(first)
            } else {
                runtime.advance(AdvanceDirection::Next, now, playable)
            }
        }
        OutputPlaylistAction::Next => runtime.advance(AdvanceDirection::Next, now, playable),
        OutputPlaylistAction::Previous => {
            runtime.advance(AdvanceDirection::Previous, now, playable)
        }
        OutputPlaylistAction::Stop => {
            runtime.stop();
            config.playlists.active = None;
            return Ok(());
        }
    };

    let selection = selection.ok_or_else(|| anyhow::anyhow!("playlist has no playable item"))?;
    config.playlists.active = runtime.active_name().map(str::to_string);
    playlist::apply_selection_to_config(config, &selection)
}

fn playlist_selection_is_available(selection: &PlaylistSelection) -> bool {
    Path::new(&selection.source).join("project.json").is_file()
}

fn playlist_item_is_available(item: &we_core::playlist::PlaylistItem) -> bool {
    Path::new(&item.source).join("project.json").is_file()
}

fn stop_worker(workers: &mut BTreeMap<String, OutputWorker>, output: &str) {
    let Some(mut worker) = workers.remove(output) else {
        return;
    };
    worker.stop_scheduler.store(true, Ordering::Relaxed);
    let _ = worker.control_tx.send(ControlCommand::Stop);
    if let Some(handle) = worker.handle.take() {
        let _ = handle.join();
    }
}

fn stop_all_workers(workers: &mut BTreeMap<String, OutputWorker>) {
    let outputs = workers.keys().cloned().collect::<Vec<_>>();
    for output in outputs {
        stop_worker(workers, &output);
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::time::Instant;

    use crate::config::Config;
    use crate::ipc::OutputPlaylistAction;
    use crate::runtime::playlist::PlaylistRuntime;
    use we_core::{
        config::OutputBinding,
        playlist::{Playlist, PlaylistConfig, PlaylistItem},
    };

    use super::{
        apply_output_playlist_action, build_output_specs, reconcile_workers, OutputAction,
    };

    #[test]
    fn discovered_outputs_get_independent_workers_with_stable_names() {
        let config = Config::default();
        let specs = build_output_specs(&config, &["DP-1".to_string(), "HDMI-A-1".to_string()])
            .expect("output specs");

        assert_eq!(specs.keys().cloned().collect::<Vec<_>>(), vec!["DP-1", "HDMI-A-1"]);
        assert_ne!(specs["DP-1"].worker_id, specs["HDMI-A-1"].worker_id);
    }

    #[test]
    fn output_only_config_skips_unbound_displays_when_global_source_is_empty() {
        let mut config = Config::default();
        config.renderer.source.clear();
        config
            .outputs
            .insert("DP-1".to_string(), OutputBinding::wallpaper("alpha", "/wallpapers/alpha"));

        let specs = build_output_specs(&config, &["DP-1".to_string(), "HDMI-A-1".to_string()])
            .expect("output specs");

        assert_eq!(specs.keys().cloned().collect::<Vec<_>>(), vec!["DP-1"]);
        assert_eq!(specs["DP-1"].config.renderer.source, "/wallpapers/alpha");
    }

    #[test]
    fn changing_one_output_restarts_only_that_worker() {
        let mut config = Config::default();
        config
            .outputs
            .insert("DP-1".to_string(), OutputBinding::wallpaper("alpha", "/wallpapers/alpha"));
        config
            .outputs
            .insert("HDMI-A-1".to_string(), OutputBinding::wallpaper("beta", "/wallpapers/beta"));
        let discovered = ["DP-1".to_string(), "HDMI-A-1".to_string()];
        let initial = build_output_specs(&config, &discovered).expect("initial specs");
        let current = initial
            .iter()
            .map(|(name, spec)| (name.clone(), spec.fingerprint.clone()))
            .collect::<BTreeMap<_, _>>();

        config
            .outputs
            .insert("DP-1".to_string(), OutputBinding::wallpaper("gamma", "/wallpapers/gamma"));
        let next = build_output_specs(&config, &discovered).expect("next specs");
        assert_eq!(
            reconcile_workers(&current, &next),
            vec![OutputAction::Restart("DP-1".to_string())]
        );
    }

    #[test]
    fn hotplug_stops_removed_output_and_starts_only_the_new_output() {
        let config = Config::default();
        let initial = build_output_specs(&config, &["DP-1".to_string(), "HDMI-A-1".to_string()])
            .expect("initial specs");
        let current = initial
            .iter()
            .map(|(name, spec)| (name.clone(), spec.fingerprint.clone()))
            .collect::<BTreeMap<_, _>>();
        let next = build_output_specs(&config, &["HDMI-A-1".to_string(), "eDP-1".to_string()])
            .expect("next specs");

        assert_eq!(
            reconcile_workers(&current, &next),
            vec![OutputAction::Stop("DP-1".to_string()), OutputAction::Start("eDP-1".to_string()),]
        );
    }

    #[test]
    fn different_playlist_bindings_create_independent_playback_specs() {
        let mut config = Config::default();
        config.outputs.insert("DP-1".to_string(), OutputBinding::playlist("Focus"));
        config.outputs.insert("HDMI-A-1".to_string(), OutputBinding::playlist("Ambient"));

        let specs = build_output_specs(&config, &["DP-1".to_string(), "HDMI-A-1".to_string()])
            .expect("output specs");

        assert_eq!(specs["DP-1"].playlist_name(), Some("Focus"));
        assert_eq!(specs["HDMI-A-1"].playlist_name(), Some("Ambient"));
        assert_ne!(specs["DP-1"].worker_id, specs["HDMI-A-1"].worker_id);
    }

    #[test]
    fn ambiguous_output_binding_is_rejected() {
        let mut config = Config::default();
        config.outputs.insert(
            "DP-1".to_string(),
            OutputBinding {
                wallpaper_id: Some("alpha".to_string()),
                source: Some("/wallpapers/alpha".to_string()),
                playlist: Some("Focus".to_string()),
            },
        );

        assert!(build_output_specs(&config, &["DP-1".to_string()]).is_err());
    }

    #[test]
    fn output_playlist_action_advances_or_stops_only_its_runtime_config() {
        let mut playlists = PlaylistConfig::default();
        playlists.active = Some("Focus".to_string());
        playlists.definitions.insert(
            "Focus".to_string(),
            Playlist {
                items: vec![
                    PlaylistItem {
                        wallpaper_id: "one".to_string(),
                        source: "/wallpapers/one".to_string(),
                        duration_ms: None,
                    },
                    PlaylistItem {
                        wallpaper_id: "two".to_string(),
                        source: "/wallpapers/two".to_string(),
                        duration_ms: None,
                    },
                ],
                ..Playlist::default()
            },
        );
        let now = Instant::now();
        let mut runtime = PlaylistRuntime::new(playlists.clone(), 7);
        let first = runtime.ensure_started(now).expect("first playlist item");
        let mut config = Config::default();
        config.playlists = playlists;
        super::playlist::apply_selection_to_config(&mut config, &first)
            .expect("apply first selection");

        apply_output_playlist_action(
            &mut runtime,
            &mut config,
            &OutputPlaylistAction::Next,
            now,
            |_| true,
        )
        .expect("advance output playlist");
        assert_eq!(config.renderer.source, "/wallpapers/two");
        assert_eq!(runtime.current_selection().expect("current item").wallpaper_id, "two");

        let source_before_stop = config.renderer.source.clone();
        apply_output_playlist_action(
            &mut runtime,
            &mut config,
            &OutputPlaylistAction::Stop,
            now,
            |_| true,
        )
        .expect("stop output playlist");
        assert_eq!(runtime.active_name(), None);
        assert_eq!(config.renderer.source, source_before_stop);
    }
}
