use std::{
    collections::BTreeMap,
    fs,
    path::Path,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};

use anyhow::{anyhow, Context, Result};
use tracing::{info, warn};
use we_core::{
    config::OutputBinding,
    install_layout::{expand_tilde, resolve_renderer_library},
};
use we_renderer::RendererLibrary;

use crate::{
    backend::{
        self,
        layer_shell::state::LayerShellState,
        traits::{BackendContext, BackendKind},
        wayland_common::{connection, registry},
    },
    config::Config,
    hooks::{self, WallpaperAppliedContext, WallpaperAppliedTrigger},
    ipc::{self, ControlCommand, OutputPlaylistAction, PlaylistCommand, RuntimeLoopExit},
    runtime::{
        control::RuntimePhase,
        playlist::{self, AdvanceDirection, PlaylistRuntime, PlaylistSelection},
        status::RuntimeStatusSnapshot,
    },
};

pub fn run(config_path: Option<&Path>) -> Result<()> {
    let mut cfg = Config::load(config_path)?;
    let playlist_state_path = playlist::state_path_for_config(config_path);
    let now = Instant::now();
    let mut playlist_runtime =
        match playlist_state_path.as_deref().map(playlist::load_snapshot).transpose() {
            Ok(Some(Some(snapshot))) => {
                PlaylistRuntime::restore(cfg.playlists.clone(), snapshot, now)
            }
            Ok(_) => PlaylistRuntime::new(cfg.playlists.clone(), playlist::random_seed()),
            Err(error) => {
                warn!(error = %error, "ignoring invalid playlist runtime state");
                PlaylistRuntime::new(cfg.playlists.clone(), playlist::random_seed())
            }
        };
    if let Some(selection) = select_initial_playlist_item(&mut playlist_runtime, now) {
        playlist::apply_selection_to_config(&mut cfg, &selection)?;
    }
    persist_playlist_runtime(playlist_state_path.as_deref(), &playlist_runtime);
    let playlist_runtime = Arc::new(Mutex::new(playlist_runtime));

    let (control_tx, control_rx) = mpsc::channel::<ControlCommand>();
    let (output_playlist_tx, output_playlist_rx) = mpsc::channel::<ipc::OutputPlaylistRequest>();
    let desired_cfg = Arc::new(Mutex::new(cfg.clone()));
    let current_cfg = Arc::new(Mutex::new(cfg.clone()));
    let runtime_cfg_toml = Arc::new(Mutex::new(cfg.to_toml_pretty()?));
    let runtime_state = Arc::new(Mutex::new(RuntimeState::new(&cfg)));
    let shutdown_requested = Arc::new(AtomicBool::new(false));
    let scheduler_stop = Arc::new(AtomicBool::new(false));
    install_runtime_ctrlc_handler(control_tx.clone(), shutdown_requested.clone())?;

    let status_state = runtime_state.clone();
    let status_playlist = playlist_runtime.clone();
    let command_tx = control_tx.clone();
    let switch_tx = control_tx.clone();
    let playlist_tx = control_tx.clone();

    let _control_server = ipc::ControlServer::start(
        control_tx,
        {
            let runtime_cfg_toml = runtime_cfg_toml.clone();
            move || {
                let mut status = runtime_cfg_toml
                    .lock()
                    .map(|guard| guard.clone())
                    .unwrap_or_else(|_| "<status unavailable>".to_string());
                if let Ok(guard) = status_state.lock() {
                    status.push_str("\n\n");
                    status.push_str(&guard.render_status_toml());
                }
                if let Ok(guard) = status_playlist.lock() {
                    status.push_str("\n\n");
                    status.push_str(&guard.render_status_toml());
                }
                status
            }
        },
        {
            let runtime_state = runtime_state.clone();
            let playlist_runtime = playlist_runtime.clone();
            let scheduler_stop = scheduler_stop.clone();
            let playlist_state_path = playlist_state_path.clone();
            move |cmd| {
                let handled = handle_runtime_control_command(cmd, &command_tx, &runtime_state)?;
                let now = Instant::now();
                if let Ok(mut runtime) = playlist_runtime.lock() {
                    match cmd {
                        ControlCommand::Pause => runtime.pause(now),
                        ControlCommand::Resume => runtime.resume(now),
                        ControlCommand::Stop => scheduler_stop.store(true, Ordering::Relaxed),
                        _ => {}
                    }
                    persist_playlist_runtime(playlist_state_path.as_deref(), &runtime);
                }
                Ok(handled)
            }
        },
        {
            let desired_cfg = desired_cfg.clone();
            let runtime_cfg_toml = runtime_cfg_toml.clone();
            let runtime_state = runtime_state.clone();
            let playlist_runtime = playlist_runtime.clone();
            let playlist_state_path = playlist_state_path.clone();
            let switch_tx = switch_tx.clone();
            move |config_path| {
                let mut next_cfg = Config::load(Some(config_path))?;
                let now = Instant::now();
                if let Ok(mut runtime) = playlist_runtime.lock() {
                    runtime.configure(next_cfg.playlists.clone(), now);
                    if let Some(selection) = select_initial_playlist_item(&mut runtime, now) {
                        playlist::apply_selection_to_config(&mut next_cfg, &selection)?;
                    }
                    persist_playlist_runtime(playlist_state_path.as_deref(), &runtime);
                }
                schedule_config_reconfigure(
                    next_cfg,
                    &desired_cfg,
                    &runtime_cfg_toml,
                    &runtime_state,
                    &switch_tx,
                )
            }
        },
        {
            let desired_cfg = desired_cfg.clone();
            let runtime_cfg_toml = runtime_cfg_toml.clone();
            let runtime_state = runtime_state.clone();
            let playlist_runtime = playlist_runtime.clone();
            let playlist_state_path = playlist_state_path.clone();
            let output_playlist_tx = output_playlist_tx.clone();
            move |command| {
                if let PlaylistCommand::Output { output, action } = &command {
                    if let OutputPlaylistAction::Play(name) = action {
                        let mut next_cfg = desired_cfg
                            .lock()
                            .map_err(|_| anyhow!("failed to read desired output config"))?
                            .clone();
                        if !next_cfg.playlists.definitions.contains_key(name) {
                            return Err(anyhow!("playlist '{name}' does not exist"));
                        }
                        next_cfg
                            .outputs
                            .insert(output.clone(), OutputBinding::playlist(name.clone()));
                        schedule_config_reconfigure(
                            next_cfg,
                            &desired_cfg,
                            &runtime_cfg_toml,
                            &runtime_state,
                            &playlist_tx,
                        )?;
                    }
                    let (reply_tx, reply_rx) = mpsc::channel();
                    output_playlist_tx
                        .send(ipc::OutputPlaylistRequest {
                            output: output.clone(),
                            action: action.clone(),
                            reply: reply_tx,
                        })
                        .context("failed to send output playlist command to runtime")?;
                    return reply_rx
                        .recv_timeout(Duration::from_secs(2))
                        .map_err(|_| anyhow!("timed out waiting for output playlist command"))?
                        .map_err(anyhow::Error::msg);
                }
                let now = Instant::now();
                let (selection, active_name) = {
                    let mut runtime = playlist_runtime
                        .lock()
                        .map_err(|_| anyhow!("playlist runtime lock poisoned"))?;
                    let selection = match &command {
                        PlaylistCommand::Play(name) => {
                            let first = runtime.play(name, now).map_err(anyhow::Error::msg)?;
                            if playlist_selection_is_available(&first) {
                                Some(first)
                            } else {
                                runtime.advance(
                                    AdvanceDirection::Next,
                                    now,
                                    playlist_item_is_available,
                                )
                            }
                        }
                        PlaylistCommand::Next => {
                            runtime.advance(AdvanceDirection::Next, now, playlist_item_is_available)
                        }
                        PlaylistCommand::Previous => runtime.advance(
                            AdvanceDirection::Previous,
                            now,
                            playlist_item_is_available,
                        ),
                        PlaylistCommand::Stop => {
                            runtime.stop();
                            None
                        }
                        PlaylistCommand::Output { .. } => {
                            unreachable!("output playlist commands are handled before global runtime mutation")
                        }
                    };
                    persist_playlist_runtime(playlist_state_path.as_deref(), &runtime);
                    (selection, runtime.active_name().map(str::to_string))
                };

                if matches!(command, PlaylistCommand::Stop) {
                    update_playlist_active_config(None, &desired_cfg, &runtime_cfg_toml)?;
                    return Ok(());
                }

                let selection =
                    selection.ok_or_else(|| anyhow!("playlist has no playable item"))?;
                schedule_playlist_selection(
                    selection,
                    active_name,
                    &desired_cfg,
                    &runtime_cfg_toml,
                    &runtime_state,
                    &playlist_tx,
                )
            }
        },
    )?;

    {
        let desired_cfg = desired_cfg.clone();
        let runtime_cfg_toml = runtime_cfg_toml.clone();
        let runtime_state = runtime_state.clone();
        let playlist_runtime = playlist_runtime.clone();
        let playlist_state_path = playlist_state_path.clone();
        let shutdown_requested = shutdown_requested.clone();
        let scheduler_stop = scheduler_stop.clone();
        let scheduler_tx = switch_tx.clone();
        thread::spawn(move || {
            while !shutdown_requested.load(Ordering::Relaxed)
                && !scheduler_stop.load(Ordering::Relaxed)
            {
                thread::sleep(Duration::from_millis(50));
                let now = Instant::now();
                let (selection, active_name) = {
                    let Ok(mut runtime) = playlist_runtime.lock() else {
                        break;
                    };
                    let selection = runtime.due_selection(now, playlist_item_is_available);
                    if selection.is_some() {
                        persist_playlist_runtime(playlist_state_path.as_deref(), &runtime);
                    }
                    (selection, runtime.active_name().map(str::to_string))
                };
                let Some(selection) = selection else {
                    continue;
                };
                if let Err(error) = schedule_playlist_selection(
                    selection,
                    active_name,
                    &desired_cfg,
                    &runtime_cfg_toml,
                    &runtime_state,
                    &scheduler_tx,
                ) {
                    warn!(error = %error, "failed to advance playlist");
                }
            }
        });
    }

    loop {
        let next_cfg = desired_cfg
            .lock()
            .map(|guard| guard.clone())
            .map_err(|_| anyhow!("failed to read desired runtime config"))?;

        if let Ok(mut guard) = current_cfg.lock() {
            *guard = next_cfg.clone();
        }
        if let Ok(mut guard) = runtime_cfg_toml.lock() {
            *guard = next_cfg.to_toml_pretty()?;
        }

        let generation = {
            let mut state =
                runtime_state.lock().map_err(|_| anyhow!("runtime state lock poisoned"))?;
            state.begin_session(&next_cfg)
        };

        let exit = match run_runtime_loop(
            &next_cfg,
            desired_cfg.clone(),
            &shutdown_requested,
            &runtime_state,
            generation,
            &control_rx,
            &output_playlist_rx,
        ) {
            Ok(exit) => exit,
            Err(err) => {
                if let Ok(mut state) = runtime_state.lock() {
                    state.fail(generation, err.to_string());
                }
                return Err(err);
            }
        };

        if let Ok(mut state) = runtime_state.lock() {
            state.mark_stopping(generation);
            state.mark_idle(generation);
        }

        match exit {
            RuntimeLoopExit::Stop => {
                scheduler_stop.store(true, Ordering::Relaxed);
                break;
            }
            RuntimeLoopExit::RestartCurrent | RuntimeLoopExit::Reconfigure => continue,
        }
    }

    Ok(())
}

fn select_initial_playlist_item(
    runtime: &mut PlaylistRuntime,
    now: Instant,
) -> Option<PlaylistSelection> {
    let selection = runtime.ensure_started(now)?;
    if playlist_selection_is_available(&selection) {
        return Some(selection);
    }
    runtime.advance(AdvanceDirection::Next, now, playlist_item_is_available)
}

fn playlist_selection_is_available(selection: &PlaylistSelection) -> bool {
    Path::new(&selection.source).join("project.json").is_file()
}

fn playlist_item_is_available(item: &we_core::playlist::PlaylistItem) -> bool {
    Path::new(&item.source).join("project.json").is_file()
}

fn persist_playlist_runtime(path: Option<&Path>, runtime: &PlaylistRuntime) {
    let Some(path) = path else {
        return;
    };
    let snapshot = runtime.snapshot();
    if let Err(error) = playlist::persist_snapshot(path, snapshot.as_ref()) {
        warn!(error = %error, "failed to persist playlist runtime state");
    }
}

fn update_playlist_active_config(
    active_name: Option<String>,
    desired_cfg: &Arc<Mutex<Config>>,
    runtime_cfg_toml: &Arc<Mutex<String>>,
) -> Result<()> {
    let mut next_cfg = desired_cfg
        .lock()
        .map_err(|_| anyhow!("failed to update desired playlist config"))?
        .clone();
    next_cfg.playlists.active = active_name;
    if let Ok(mut guard) = desired_cfg.lock() {
        *guard = next_cfg.clone();
    }
    if let Ok(mut guard) = runtime_cfg_toml.lock() {
        *guard = next_cfg.to_toml_pretty()?;
    }
    Ok(())
}

fn schedule_playlist_selection(
    selection: PlaylistSelection,
    active_name: Option<String>,
    desired_cfg: &Arc<Mutex<Config>>,
    runtime_cfg_toml: &Arc<Mutex<String>>,
    runtime_state: &Arc<Mutex<RuntimeState>>,
    control_tx: &mpsc::Sender<ControlCommand>,
) -> Result<()> {
    let mut next_cfg =
        desired_cfg.lock().map_err(|_| anyhow!("failed to read desired playlist config"))?.clone();
    next_cfg.playlists.active = active_name;
    playlist::apply_selection_to_config(&mut next_cfg, &selection)?;
    schedule_config_reconfigure(next_cfg, desired_cfg, runtime_cfg_toml, runtime_state, control_tx)
}

fn schedule_config_reconfigure(
    next_cfg: Config,
    desired_cfg: &Arc<Mutex<Config>>,
    runtime_cfg_toml: &Arc<Mutex<String>>,
    runtime_state: &Arc<Mutex<RuntimeState>>,
    control_tx: &mpsc::Sender<ControlCommand>,
) -> Result<()> {
    if let Ok(mut guard) = desired_cfg.lock() {
        *guard = next_cfg.clone();
    }
    if let Ok(mut guard) = runtime_cfg_toml.lock() {
        *guard = next_cfg.to_toml_pretty()?;
    }
    if let Ok(mut state) = runtime_state.lock() {
        state.begin_switch(&next_cfg);
    }
    control_tx
        .send(ControlCommand::Reconfigure)
        .context("failed to schedule runtime reconfiguration")
}

#[derive(Debug, Clone)]
struct RuntimeState {
    backend: BackendKind,
    phase: RuntimePhase,
    generation: u64,
    source: String,
    error: Option<String>,
    runtime_statuses: std::collections::BTreeMap<String, RuntimeStatusSnapshot>,
}

impl RuntimeState {
    fn new(cfg: &Config) -> Self {
        Self {
            backend: resolve_backend(cfg),
            phase: RuntimePhase::Idle,
            generation: 0,
            source: cfg.renderer.source.clone(),
            error: None,
            runtime_statuses: Default::default(),
        }
    }

    fn begin_session(&mut self, cfg: &Config) -> u64 {
        self.generation = self.generation.saturating_add(1);
        self.backend = resolve_backend(cfg);
        self.phase = RuntimePhase::Starting;
        self.source = cfg.renderer.source.clone();
        self.error = None;
        self.runtime_statuses.clear();
        self.generation
    }

    fn begin_switch(&mut self, cfg: &Config) {
        self.backend = resolve_backend(cfg);
        self.phase = RuntimePhase::Starting;
        self.source = cfg.renderer.source.clone();
        self.error = None;
        self.runtime_statuses.clear();
    }

    fn mark_running(&mut self, generation: u64) {
        if generation == self.generation {
            self.phase = RuntimePhase::Running;
            self.error = None;
        }
    }

    fn mark_paused(&mut self) {
        self.phase = RuntimePhase::Paused;
    }

    fn mark_resumed(&mut self) {
        self.phase = RuntimePhase::Running;
    }

    fn mark_stopping(&mut self, generation: u64) {
        if generation == self.generation {
            self.phase = RuntimePhase::Stopping;
        }
    }

    fn mark_idle(&mut self, generation: u64) {
        if generation == self.generation {
            self.phase = RuntimePhase::Idle;
            self.error = None;
        }
    }

    fn fail(&mut self, generation: u64, error: String) {
        if generation == self.generation {
            self.phase = RuntimePhase::Failed;
            self.error = Some(error);
        }
    }

    fn update_runtime_status(&mut self, status: RuntimeStatusSnapshot) {
        if status.remove_output {
            self.runtime_statuses.remove(&status.output_name);
            return;
        }
        self.runtime_statuses.insert(status.output_name.clone(), status);
    }

    fn render_status_toml(&self) -> String {
        let mut lines = vec![
            "[orchestrator]".to_string(),
            format!("backend = \"{}\"", backend_name(self.backend)),
            format!("phase = \"{}\"", self.phase.as_str()),
            "multi_output = true".to_string(),
            format!("generation = {}", self.generation),
            format!("source = {:?}", self.source),
        ];
        if let Some(error) = &self.error {
            lines.push(format!("error = {:?}", error));
        }
        if self.runtime_statuses.len() == 1 {
            let status = self.runtime_statuses.values().next().expect("one runtime status");
            lines.push(String::new());
            lines.push(status.render_toml());
        } else if !self.runtime_statuses.is_empty() {
            for (output_name, status) in &self.runtime_statuses {
                lines.push(String::new());
                lines.push(status.render_output_toml(output_name));
            }
        }
        lines.join("\n")
    }
}

fn backend_name(backend: BackendKind) -> &'static str {
    backend.as_config_str()
}

fn resolve_backend(cfg: &Config) -> BackendKind {
    cfg.general.backend.kind()
}

fn env_var_enabled(name: &str) -> bool {
    std::env::var(name).map(|value| !value.is_empty() && value != "0").unwrap_or(false)
}

fn env_var_equals(name: &str, expected: &str) -> bool {
    std::env::var(name).map(|value| value == expected).unwrap_or(false)
}

fn update_runtime_snapshot(
    runtime_state: &Arc<Mutex<RuntimeState>>,
    snapshot: RuntimeStatusSnapshot,
) {
    if let Ok(mut state) = runtime_state.lock() {
        state.update_runtime_status(snapshot);
    }
}

fn run_runtime_loop(
    cfg: &Config,
    desired_cfg: Arc<Mutex<Config>>,
    shutdown_requested: &Arc<AtomicBool>,
    runtime_state: &Arc<Mutex<RuntimeState>>,
    generation: u64,
    control_rx: &mpsc::Receiver<ControlCommand>,
    output_playlist_rx: &mpsc::Receiver<ipc::OutputPlaylistRequest>,
) -> Result<RuntimeLoopExit> {
    let mut cfg = cfg.clone();
    cfg.renderer.options_json = Some(we_core::config::merge_scene_source_options(
        cfg.renderer.options_json.as_deref(),
        None,
        cfg.general.force_scene_audio_loop,
    )?);
    cfg.renderer.validate_options_json()?;
    if cfg.renderer.assets_path.trim().is_empty() {
        cfg.renderer.assets_path = we_core::steam::discover_wallpaper_engine_path()
            .map(|p| p.join("assets").display().to_string())
            .ok_or_else(|| {
                anyhow!(
                    "renderer.assets_path is empty and Wallpaper Engine assets directory was not found; set assets_path in config"
                )
            })?;
    }

    if shutdown_requested.load(Ordering::Relaxed) {
        return Ok(RuntimeLoopExit::Stop);
    }

    let backend = resolve_backend(&cfg);
    if cfg.renderer.source.trim().is_empty()
        && !(backend == BackendKind::LayerShell && !cfg.outputs.is_empty())
    {
        return Err(anyhow!(
            "renderer.source is required when no per-output bindings are configured"
        ));
    }

    info!(
        backend = backend_name(backend),
        source = %cfg.renderer.source,
        assets_path = %cfg.renderer.assets_path,
        library_path = %cfg.renderer.library_path,
        "starting renderer-native runtime"
    );

    if let Ok(mut state) = runtime_state.lock() {
        state.mark_running(generation);
    }

    let mut backend_impl = backend::create_backend(backend);
    let hook_cfg = desired_cfg.clone();
    let mut hook_triggers = BTreeMap::<String, (String, WallpaperAppliedTrigger)>::new();
    let mut status_sink = |snapshot: RuntimeStatusSnapshot| {
        let output_key = snapshot.output_name.clone();
        let source = if snapshot.output_source.is_empty() {
            cfg.renderer.source.clone()
        } else {
            snapshot.output_source.clone()
        };
        let wallpaper_applied = if snapshot.remove_output {
            hook_triggers.remove(&output_key);
            false
        } else {
            let entry = hook_triggers
                .entry(output_key)
                .or_insert_with(|| (source.clone(), WallpaperAppliedTrigger::default()));
            if entry.0 != source {
                *entry = (source.clone(), WallpaperAppliedTrigger::default());
            }
            entry.1.observe(&snapshot)
        };
        update_runtime_snapshot(runtime_state, snapshot);
        if wallpaper_applied {
            if let Ok(current_cfg) = hook_cfg.lock() {
                hooks::spawn_wallpaper_applied(
                    current_cfg.hooks.wallpaper_applied.as_ref(),
                    WallpaperAppliedContext { source: &source, backend, generation },
                );
            }
        }
    };
    backend_impl.run(BackendContext {
        cfg: &cfg,
        desired_cfg,
        shutdown_requested: shutdown_requested.clone(),
        control_rx,
        output_playlist_rx: Some(output_playlist_rx),
        status_sink: &mut status_sink,
    })
}

fn handle_runtime_control_command(
    cmd: ControlCommand,
    control_tx: &mpsc::Sender<ControlCommand>,
    runtime_state: &Arc<Mutex<RuntimeState>>,
) -> Result<bool> {
    match cmd {
        ControlCommand::Pause => {
            control_tx.send(cmd).with_context(|| {
                format!("failed to forward {} command to runtime", cmd.as_str())
            })?;
            if let Ok(mut state) = runtime_state.lock() {
                state.mark_paused();
            }
            Ok(true)
        }
        ControlCommand::Resume => {
            control_tx.send(cmd).with_context(|| {
                format!("failed to forward {} command to runtime", cmd.as_str())
            })?;
            if let Ok(mut state) = runtime_state.lock() {
                state.mark_resumed();
            }
            Ok(true)
        }
        ControlCommand::Reload => Ok(false),
        ControlCommand::Stop => Ok(false),
        ControlCommand::Reconfigure => Ok(false),
    }
}

pub fn doctor(config_path: Option<&Path>) -> Result<()> {
    let (cfg, loaded_from) = load_doctor_config(config_path)?;
    let mut lines = Vec::new();
    let (options_json_present, options_json_len, options_json_valid) =
        cfg.renderer.options_json_diagnostics();
    let backend = resolve_backend(&cfg);

    lines.push(format!("OK backend = {}", backend_name(backend)));
    match loaded_from {
        Some(path) => lines.push(format!("OK config = {}", path.display())),
        None => lines.push("WARN config = using built-in defaults".to_string()),
    }

    match connection::connect_to_env() {
        Ok(conn) => {
            lines.push("OK wayland_display = connected".to_string());
            match registry::init_registry::<LayerShellState>(&conn) {
                Ok((globals, _event_queue)) => {
                    let snapshot = globals.contents().clone_list();
                    let mut required = vec!["wl_compositor", "wl_shm"];
                    match backend {
                        BackendKind::LayerShell => required.push("zwlr_layer_shell_v1"),
                        BackendKind::Gnome => required.push("xdg_wm_base"),
                    }
                    for interface in required {
                        push_global_check(&mut lines, &snapshot, interface, true);
                    }
                    for optional in
                        ["zwp_linux_dmabuf_v1", "wp_viewporter", "wp_fractional_scale_manager_v1"]
                    {
                        push_global_check(&mut lines, &snapshot, optional, false);
                    }
                }
                Err(err) => lines.push(format!("ERR wayland_registry = {err}")),
            }
        }
        Err(err) => lines.push(format!("ERR wayland_display = {err}")),
    }

    match resolve_renderer_library(&cfg.renderer.library_path) {
        Ok(path) => {
            lines.push(format!("OK renderer_library = {}", path.display()));
            match RendererLibrary::load(&path) {
                Ok(_) => lines.push("OK renderer_symbols = loaded".to_string()),
                Err(err) => lines.push(format!("ERR renderer_symbols = {err}")),
            }
        }
        Err(err) => lines.push(format!("ERR renderer_library = {err}")),
    }

    let source_path = expand_tilde(&cfg.renderer.source);
    if cfg.renderer.source.trim().is_empty() {
        if cfg.outputs.is_empty() {
            lines.push("ERR renderer.source = empty".to_string());
        } else {
            lines.push(format!("OK renderer.source = per-output bindings ({})", cfg.outputs.len()));
        }
    } else if source_path.is_dir() {
        lines.push(format!("OK renderer.source = {}", source_path.display()));
    } else {
        lines.push(format!("ERR renderer.source = missing {}", source_path.display()));
    }

    for (output, binding) in &cfg.outputs {
        if binding.is_ambiguous() {
            lines.push(format!(
                "ERR output.{output} = wallpaper and playlist bindings are mutually exclusive"
            ));
            continue;
        }
        if let Some(source) = binding.source.as_deref() {
            let source = expand_tilde(source);
            if source.is_dir() {
                lines.push(format!("OK output.{output}.source = {}", source.display()));
            } else {
                lines.push(format!("ERR output.{output}.source = missing {}", source.display()));
            }
        }
        if let Some(playlist) = binding.playlist.as_deref() {
            if cfg.playlists.definitions.contains_key(playlist) {
                lines.push(format!("OK output.{output}.playlist = {playlist}"));
            } else {
                lines.push(format!("ERR output.{output}.playlist = missing {playlist}"));
            }
        }
    }

    if cfg.renderer.assets_path.trim().is_empty() {
        if let Some(discovered) = we_core::steam::discover_wallpaper_engine_path() {
            lines.push(format!(
                "WARN renderer.assets_path = auto-discovered {}",
                discovered.join("assets").display()
            ));
        } else {
            lines.push("ERR renderer.assets_path = empty and auto-discovery failed".to_string());
        }
    } else {
        let assets_path = expand_tilde(&cfg.renderer.assets_path);
        if assets_path.is_dir() {
            lines.push(format!("OK renderer.assets_path = {}", assets_path.display()));
        } else {
            lines.push(format!("ERR renderer.assets_path = missing {}", assets_path.display()));
        }
    }

    let cache_path = expand_tilde(&cfg.renderer.cache_path);
    let cache_parent = cache_path.parent().map(Path::to_path_buf).unwrap_or(cache_path.clone());
    match fs::create_dir_all(&cache_parent) {
        Ok(()) => lines.push(format!("OK renderer.cache_path_parent = {}", cache_parent.display())),
        Err(err) => lines
            .push(format!("ERR renderer.cache_path_parent = {} ({err})", cache_parent.display())),
    }

    match we_core::steam::discover_workshop_wallpaper_root() {
        Some(path) => lines.push(format!("OK workshop_auto_discovery = {}", path.display())),
        None => lines.push("WARN workshop_auto_discovery = not found".to_string()),
    }
    match we_core::steam::discover_wallpaper_engine_path() {
        Some(path) => lines.push(format!("OK assets_auto_discovery = {}", path.display())),
        None => lines.push("WARN assets_auto_discovery = not found".to_string()),
    }
    lines.push(format!("OK renderer.options_json_present = {options_json_present}"));
    lines.push(format!("OK renderer.options_json_len = {options_json_len}"));
    if options_json_valid {
        lines.push("OK renderer.options_json_valid = true".to_string());
    } else {
        lines.push("ERR renderer.options_json_valid = false".to_string());
    }

    if env_var_enabled("__NV_PRIME_RENDER_OFFLOAD")
        || env_var_equals("__VK_LAYER_NV_optimus", "NVIDIA_only")
    {
        lines.push(
            "WARN nvidia_prime_offload = detected; runtime will force SHM fallback".to_string(),
        );
    } else {
        lines.push("OK nvidia_prime_offload = not detected".to_string());
    }

    if backend == BackendKind::Gnome {
        let backend_impl = backend::create_backend(backend);
        lines.push(format!("OK backend.kind = {}", backend_name(backend_impl.kind())));
        let capabilities = backend_impl.capabilities();
        lines.push(format!(
            "OK backend.needs_external_extension = {}",
            capabilities.needs_external_extension
        ));
    }

    println!("{}", lines.join("\n"));
    Ok(())
}

fn load_doctor_config(config_path: Option<&Path>) -> Result<(Config, Option<PathBuf>)> {
    if let Some(path) = config_path {
        return Config::load(Some(path)).map(|cfg| (cfg, Some(path.to_path_buf())));
    }

    if let Some(default_path) = we_core::steam::default_config_path() {
        if default_path.is_file() {
            return Config::load(Some(&default_path)).map(|cfg| (cfg, Some(default_path)));
        }
    }

    Ok((Config::default(), None))
}

fn push_global_check(
    lines: &mut Vec<String>,
    globals: &[wayland_client::globals::Global],
    interface: &str,
    required: bool,
) {
    let found = globals.iter().find(|global| global.interface == interface);
    match (required, found) {
        (_, Some(global)) => lines.push(format!("OK global {interface} = v{}", global.version)),
        (true, None) => lines.push(format!("ERR global {interface} = missing")),
        (false, None) => lines.push(format!("WARN global {interface} = missing")),
    }
}

fn request_runtime_shutdown(
    control_tx: &mpsc::Sender<ControlCommand>,
    shutdown_requested: &AtomicBool,
) -> Result<bool, mpsc::SendError<ControlCommand>> {
    if shutdown_requested.swap(true, Ordering::SeqCst) {
        return Ok(false);
    }

    control_tx.send(ControlCommand::Stop).map(|()| true)
}

fn install_runtime_ctrlc_handler(
    control_tx: mpsc::Sender<ControlCommand>,
    shutdown_requested: Arc<AtomicBool>,
) -> Result<()> {
    ctrlc::set_handler(move || match request_runtime_shutdown(&control_tx, &shutdown_requested) {
        Ok(true) => {
            warn!("received Ctrl+C, requesting runtime shutdown");
        }
        Ok(false) => {
            warn!("received Ctrl+C while runtime shutdown is already in progress");
        }
        Err(_) => {
            warn!("received Ctrl+C, but runtime control channel is closed");
            std::process::exit(130);
        }
    })
    .context("failed to register Ctrl+C handler")
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicBool;

    use super::{
        handle_runtime_control_command, request_runtime_shutdown, RuntimePhase, RuntimeState,
    };
    use crate::{
        config::Config,
        ipc::{ControlCommand, RuntimeLoopExit},
    };

    #[test]
    fn ctrlc_request_sends_single_stop() {
        let (tx, rx) = std::sync::mpsc::channel();
        let requested = AtomicBool::new(false);

        assert_eq!(request_runtime_shutdown(&tx, &requested), Ok(true));
        assert_eq!(request_runtime_shutdown(&tx, &requested), Ok(false));

        assert_eq!(rx.try_recv().expect("stop command"), ControlCommand::Stop);
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn playback_control_forwards_command_and_updates_status() {
        let cfg = Config::default();
        let runtime_state = std::sync::Arc::new(std::sync::Mutex::new(RuntimeState::new(&cfg)));
        let (tx, rx) = std::sync::mpsc::channel();

        assert!(handle_runtime_control_command(ControlCommand::Pause, &tx, &runtime_state).unwrap());
        assert_eq!(rx.try_recv().expect("pause command"), ControlCommand::Pause);
        assert_eq!(runtime_state.lock().expect("runtime state").phase, RuntimePhase::Paused);

        assert!(
            handle_runtime_control_command(ControlCommand::Resume, &tx, &runtime_state).unwrap()
        );
        assert_eq!(rx.try_recv().expect("resume command"), ControlCommand::Resume);
        assert_eq!(runtime_state.lock().expect("runtime state").phase, RuntimePhase::Running);
    }

    #[test]
    fn status_toml_reports_renderer_source() {
        let mut cfg = Config::default();
        cfg.renderer.source = "/tmp/workshop/item".to_string();
        let mut state = RuntimeState::new(&cfg);
        let generation = state.begin_session(&cfg);
        state.mark_running(generation);

        let status = state.render_status_toml();

        assert!(status.contains("phase = \"running\""));
        assert!(status.contains("source = \"/tmp/workshop/item\""));
    }

    #[test]
    fn placeholder_runtime_requires_renderer_paths() {
        let mut cfg = Config::default();
        cfg.renderer.source.clear();
        cfg.renderer.assets_path = "/tmp/assets".to_string();
        let state = std::sync::Arc::new(std::sync::Mutex::new(RuntimeState::new(&cfg)));
        let desired = std::sync::Arc::new(std::sync::Mutex::new(cfg.clone()));
        let (_tx, rx) = std::sync::mpsc::channel();
        let (_output_tx, output_rx) = std::sync::mpsc::channel();
        let stop = std::sync::Arc::new(AtomicBool::new(false));

        let err = super::run_runtime_loop(&cfg, desired, &stop, &state, 1, &rx, &output_rx)
            .expect_err("missing source should fail");

        assert_eq!(
            err.to_string(),
            "renderer.source is required when no per-output bindings are configured"
        );
    }

    #[test]
    fn reload_control_requests_restart_current() {
        let exit = RuntimeLoopExit::RestartCurrent;
        assert_eq!(exit, RuntimeLoopExit::RestartCurrent);
    }

    #[test]
    fn invalid_options_json_is_rejected_before_runtime_start() {
        let mut cfg = Config::default();
        cfg.renderer.source = "/tmp/workshop/item".to_string();
        cfg.renderer.assets_path = "/tmp/assets".to_string();
        cfg.renderer.options_json = Some("{invalid".to_string());
        let state = std::sync::Arc::new(std::sync::Mutex::new(RuntimeState::new(&cfg)));
        let desired = std::sync::Arc::new(std::sync::Mutex::new(cfg.clone()));
        let (_tx, rx) = std::sync::mpsc::channel();
        let (_output_tx, output_rx) = std::sync::mpsc::channel();
        let stop = std::sync::Arc::new(AtomicBool::new(false));

        let err = super::run_runtime_loop(&cfg, desired, &stop, &state, 1, &rx, &output_rx)
            .expect_err("invalid options_json should fail");

        assert!(err.to_string().contains("renderer.options_json must be valid JSON"));
    }
}
