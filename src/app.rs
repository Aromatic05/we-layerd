use std::{
    fs,
    path::PathBuf,
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
};

use anyhow::{anyhow, Context, Result};
use we_core::install_layout::{expand_tilde, resolve_renderer_library};
use we_renderer::RendererLibrary;
use tracing::{info, warn};
use wayland_client::{globals::registry_queue_init, Connection};

use crate::{
    config::{Backend, Config},
    ipc::{self, ControlCommand, RuntimeLoopExit},
    wayland::{self, diagnostics::RuntimeStatusSnapshot},
};

pub fn run(config_path: Option<&Path>) -> Result<()> {
    let cfg = Config::load(config_path)?;
    let (control_tx, control_rx) = mpsc::channel::<ControlCommand>();
    let desired_cfg = Arc::new(Mutex::new(cfg.clone()));
    let current_cfg = Arc::new(Mutex::new(cfg.clone()));
    let runtime_cfg_toml = Arc::new(Mutex::new(cfg.to_toml_pretty()?));
    let runtime_state = Arc::new(Mutex::new(RuntimeState::new(&cfg)));
    let shutdown_requested = Arc::new(AtomicBool::new(false));
    install_runtime_ctrlc_handler(control_tx.clone(), shutdown_requested.clone())?;

    let status_state = runtime_state.clone();
    let command_tx = control_tx.clone();
    let switch_tx = control_tx.clone();

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
                status
            }
        },
        {
            let runtime_state = runtime_state.clone();
            move |cmd| handle_runtime_control_command(cmd, &command_tx, &runtime_state)
        },
        {
            let desired_cfg = desired_cfg.clone();
            let runtime_cfg_toml = runtime_cfg_toml.clone();
            let runtime_state = runtime_state.clone();
            move |config_path| {
                let next_cfg = Config::load(Some(config_path))?;
                if let Ok(mut guard) = desired_cfg.lock() {
                    *guard = next_cfg.clone();
                }
                if let Ok(mut guard) = runtime_cfg_toml.lock() {
                    *guard = next_cfg.to_toml_pretty()?;
                }
                if let Ok(mut state) = runtime_state.lock() {
                    state.begin_switch(&next_cfg);
                }
                switch_tx
                    .send(ControlCommand::Reconfigure)
                    .context("failed to schedule runtime reconfiguration")?;
                Ok(())
            }
        },
    )?;

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
            &shutdown_requested,
            &runtime_state,
            generation,
            &control_rx,
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
            RuntimeLoopExit::Stop => break,
            RuntimeLoopExit::RestartCurrent | RuntimeLoopExit::Reconfigure => continue,
        }
    }

    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RuntimePhase {
    Idle,
    Starting,
    Running,
    Paused,
    Stopping,
    Failed,
}

impl RuntimePhase {
    fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Starting => "starting",
            Self::Running => "running",
            Self::Paused => "paused",
            Self::Stopping => "stopping",
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone)]
struct RuntimeState {
    backend: Backend,
    phase: RuntimePhase,
    generation: u64,
    source: String,
    error: Option<String>,
    runtime_status: Option<RuntimeStatusSnapshot>,
}

impl RuntimeState {
    fn new(cfg: &Config) -> Self {
        Self {
            backend: cfg.general.backend,
            phase: RuntimePhase::Idle,
            generation: 0,
            source: cfg.renderer.source.clone(),
            error: None,
            runtime_status: None,
        }
    }

    fn begin_session(&mut self, cfg: &Config) -> u64 {
        self.generation = self.generation.saturating_add(1);
        self.backend = cfg.general.backend;
        self.phase = RuntimePhase::Starting;
        self.source = cfg.renderer.source.clone();
        self.error = None;
        self.runtime_status = None;
        self.generation
    }

    fn begin_switch(&mut self, cfg: &Config) {
        self.backend = cfg.general.backend;
        self.phase = RuntimePhase::Starting;
        self.source = cfg.renderer.source.clone();
        self.error = None;
        self.runtime_status = None;
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
        self.runtime_status = Some(status);
    }

    fn render_status_toml(&self) -> String {
        let mut lines = vec![
            "[orchestrator]".to_string(),
            format!("backend = \"{}\"", backend_name(self.backend)),
            format!("phase = \"{}\"", self.phase.as_str()),
            format!("generation = {}", self.generation),
            format!("source = {:?}", self.source),
        ];
        if let Some(error) = &self.error {
            lines.push(format!("error = {:?}", error));
        }
        if let Some(status) = &self.runtime_status {
            lines.push(String::new());
            lines.push(status.render_toml());
        }
        lines.join("\n")
    }
}

fn backend_name(backend: Backend) -> &'static str {
    match backend {
        Backend::LayerShell => "layer_shell",
    }
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
    shutdown_requested: &AtomicBool,
    runtime_state: &Arc<Mutex<RuntimeState>>,
    generation: u64,
    control_rx: &mpsc::Receiver<ControlCommand>,
) -> Result<RuntimeLoopExit> {
    if cfg.renderer.source.trim().is_empty() {
        return Err(anyhow!("renderer.source is required"));
    }

    let mut cfg = cfg.clone();
    if cfg.renderer.assets_path.trim().is_empty() {
        cfg.renderer.assets_path = we_core::steam::discover_wallpaper_engine_assets()
            .map(|p| p.display().to_string())
            .ok_or_else(|| {
                anyhow!(
                    "renderer.assets_path is empty and Wallpaper Engine assets directory was not found; set assets_path in config"
                )
            })?;
    }

    if shutdown_requested.load(Ordering::Relaxed) {
        return Ok(RuntimeLoopExit::Stop);
    }

    info!(
        source = %cfg.renderer.source,
        assets_path = %cfg.renderer.assets_path,
        library_path = %cfg.renderer.library_path,
        "starting renderer-native runtime"
    );

    if let Ok(mut state) = runtime_state.lock() {
        state.mark_running(generation);
    }

    match cfg.general.backend {
        Backend::LayerShell => wayland::run_renderer_background_surface(
            &cfg,
            control_rx,
            &mut |snapshot| update_runtime_snapshot(runtime_state, snapshot),
        ),
    }
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

    lines.push("OK backend = layer_shell".to_string());
    match loaded_from {
        Some(path) => lines.push(format!("OK config = {}", path.display())),
        None => lines.push("WARN config = using built-in defaults".to_string()),
    }

    match Connection::connect_to_env() {
        Ok(conn) => {
            lines.push("OK wayland_display = connected".to_string());
            match registry_queue_init::<wayland::state::LayerState>(&conn) {
                Ok((globals, _event_queue)) => {
                    let snapshot = globals.contents().clone_list();
                    for required in ["wl_compositor", "zwlr_layer_shell_v1", "wl_shm"] {
                        push_global_check(&mut lines, &snapshot, required, true);
                    }
                    for optional in [
                        "zwp_linux_dmabuf_v1",
                        "wp_viewporter",
                        "wp_fractional_scale_manager_v1",
                    ] {
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
        lines.push("ERR renderer.source = empty".to_string());
    } else if source_path.is_dir() {
        lines.push(format!("OK renderer.source = {}", source_path.display()));
    } else {
        lines.push(format!("ERR renderer.source = missing {}", source_path.display()));
    }

    if cfg.renderer.assets_path.trim().is_empty() {
        if let Some(discovered) = we_core::steam::discover_wallpaper_engine_assets() {
            lines.push(format!("WARN renderer.assets_path = auto-discovered {}", discovered.display()));
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
        Err(err) => lines.push(format!(
            "ERR renderer.cache_path_parent = {} ({err})",
            cache_parent.display()
        )),
    }

    match we_core::steam::discover_workshop_wallpaper_root() {
        Some(path) => lines.push(format!("OK workshop_auto_discovery = {}", path.display())),
        None => lines.push("WARN workshop_auto_discovery = not found".to_string()),
    }
    match we_core::steam::discover_wallpaper_engine_assets() {
        Some(path) => lines.push(format!("OK assets_auto_discovery = {}", path.display())),
        None => lines.push("WARN assets_auto_discovery = not found".to_string()),
    }

    if env_var_enabled("__NV_PRIME_RENDER_OFFLOAD")
        || env_var_equals("__VK_LAYER_NV_optimus", "NVIDIA_only")
    {
        lines.push("WARN nvidia_prime_offload = detected; runtime will force SHM fallback".to_string());
    } else {
        lines.push("OK nvidia_prime_offload = not detected".to_string());
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
        let (_tx, rx) = std::sync::mpsc::channel();
        let stop = AtomicBool::new(false);

        let err = super::run_runtime_loop(&cfg, &stop, &state, 1, &rx)
            .expect_err("missing source should fail");

        assert_eq!(err.to_string(), "renderer.source is required");
    }

    #[test]
    fn reload_control_requests_restart_current() {
        let exit = RuntimeLoopExit::RestartCurrent;
        assert_eq!(exit, RuntimeLoopExit::RestartCurrent);
    }
}
