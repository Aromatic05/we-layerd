use std::{
    env,
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
};

use anyhow::{anyhow, Context, Result};
use tracing::{info, warn};

use crate::{
    cgroup::RuntimeCgroup,
    config::{Config, IsolationMode, RuntimeMode, RuntimeWallpaperType},
    display_isolation,
    gnome::{self, RegisteredWindow, ResolvedBackend},
    ipc::{self, ControlCommand, PlaybackState, RuntimeLoopExit},
    wayland,
    wine::launcher::{spawn_transient_command, WineProcessHandle},
    wm_visibility::DebugWindowVisibility,
    x11::{capture_xcomposite, window_finder, window_input},
};

pub fn run(config_path: Option<&Path>) -> Result<()> {
    let cfg = Config::load(config_path)?;
    let runtime_cfg = effective_runtime_config(&cfg);
    let runtime_cgroup = RuntimeCgroup::new(cfg.cgroup.clone());
    let (control_tx, control_rx) = mpsc::channel::<ControlCommand>();
    let desired_cfg = Arc::new(Mutex::new(cfg.clone()));
    let current_cfg = Arc::new(Mutex::new(cfg.clone()));
    let runtime_cfg_toml = Arc::new(Mutex::new(runtime_cfg.to_toml_pretty()?));
    let runtime_state = Arc::new(Mutex::new(RuntimeState::new(&runtime_cfg)));
    let shutdown_requested = Arc::new(AtomicBool::new(false));
    install_runtime_ctrlc_handler(control_tx.clone(), shutdown_requested.clone())?;

    let status_cgroup = runtime_cgroup.clone();
    let status_state = runtime_state.clone();
    let debug_visibility = DebugWindowVisibility::new(
        cfg.general.hide_debug_window,
        cfg.general.hidden_workspace_name.clone(),
        &runtime_cfg.capture,
    );
    let handler_visibility = debug_visibility.clone();
    let handler_tx = control_tx.clone();
    let handler_runtime_state = runtime_state.clone();
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
                status.push_str("\n\n");
                status.push_str(&status_cgroup.render_status_toml());
                status
            }
        },
        move |cmd| match cmd {
            ControlCommand::HideWindow => {
                handler_visibility.hide()?;
                Ok(true)
            }
            ControlCommand::ShowWindow => {
                handler_visibility.show()?;
                Ok(true)
            }
            _ => handle_playback_control_command(cmd, &handler_tx, &handler_runtime_state),
        },
        {
            let runtime_cfg_toml = runtime_cfg_toml.clone();
            let desired_cfg = desired_cfg.clone();
            let current_cfg = current_cfg.clone();
            let runtime_state = runtime_state.clone();
            move |config_path| {
                let next_cfg = Config::load(Some(config_path))?;
                let next_runtime_cfg = effective_runtime_config(&next_cfg);
                let current_cfg_value = current_cfg
                    .lock()
                    .map(|guard| guard.clone())
                    .map_err(|_| anyhow!("failed to read current runtime config"))?;
                let current_runtime_cfg = effective_runtime_config(&current_cfg_value);

                if can_hot_switch_between(&current_runtime_cfg, &next_runtime_cfg) {
                    if let Ok(mut state) = runtime_state.lock() {
                        state.begin_switch(&next_runtime_cfg);
                    }
                    switch_wallpaper(&next_cfg)?;
                    switch_tx
                        .send(ControlCommand::Resume)
                        .context("failed to resume runtime after wallpaper switch")?;
                    if let Ok(mut guard) = runtime_cfg_toml.lock() {
                        *guard = next_runtime_cfg.to_toml_pretty()?;
                    }
                    if let Ok(mut guard) = current_cfg.lock() {
                        *guard = next_cfg.clone();
                    }
                    if let Ok(mut guard) = desired_cfg.lock() {
                        *guard = next_cfg;
                    }
                    if let Ok(mut state) = runtime_state.lock() {
                        state.finish_hot_switch(&next_runtime_cfg);
                    }
                    return Ok(());
                }

                if let Ok(mut guard) = desired_cfg.lock() {
                    *guard = next_cfg;
                }
                if let Ok(mut state) = runtime_state.lock() {
                    state.begin_switch(&next_runtime_cfg);
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
        let runtime_cfg = effective_runtime_config(&next_cfg);

        if let Ok(mut guard) = current_cfg.lock() {
            *guard = next_cfg.clone();
        }
        if let Ok(mut guard) = runtime_cfg_toml.lock() {
            *guard = runtime_cfg.to_toml_pretty()?;
        }

        let generation = {
            let mut state =
                runtime_state.lock().map_err(|_| anyhow!("runtime state lock poisoned"))?;
            state.begin_session(&runtime_cfg)
        };

        let exit = match run_runtime_session(
            &runtime_cfg,
            &runtime_cgroup,
            &debug_visibility,
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
enum RuntimeKind {
    Idle,
    Window,
    Video,
}

impl RuntimeKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Window => "window",
            Self::Video => "video",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RuntimePhase {
    Idle,
    Starting,
    AwaitingWindow,
    Running,
    Switching,
    Stopping,
    Failed,
}

impl RuntimePhase {
    fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Starting => "starting",
            Self::AwaitingWindow => "awaiting_window",
            Self::Running => "running",
            Self::Switching => "switching",
            Self::Stopping => "stopping",
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RuntimeTarget {
    xid: u32,
    pid: u32,
    title: String,
    wm_class: String,
}

impl From<&RegisteredWindow> for RuntimeTarget {
    fn from(value: &RegisteredWindow) -> Self {
        Self {
            xid: value.xid,
            pid: value.pid,
            title: value.title.clone(),
            wm_class: value.wm_class.clone(),
        }
    }
}

#[derive(Debug, Clone)]
struct DesiredRuntime {
    kind: RuntimeKind,
    video_file: Option<String>,
}

impl DesiredRuntime {
    fn from_config(cfg: &Config) -> Self {
        match cfg.runtime.as_ref() {
            Some(runtime) if runtime.mode == RuntimeMode::VideoNative => {
                Self { kind: RuntimeKind::Video, video_file: runtime.video_file.clone() }
            }
            Some(_) => Self { kind: RuntimeKind::Window, video_file: None },
            None => Self { kind: RuntimeKind::Window, video_file: None },
        }
    }
}

#[derive(Debug, Clone)]
struct RuntimeState {
    backend: ResolvedBackend,
    kind: RuntimeKind,
    phase: RuntimePhase,
    playback: PlaybackState,
    generation: u64,
    target: Option<RuntimeTarget>,
    video_file: Option<String>,
    desired: DesiredRuntime,
    error: Option<String>,
}

impl RuntimeState {
    fn new(cfg: &Config) -> Self {
        Self {
            backend: gnome::resolve_backend(cfg),
            kind: RuntimeKind::Idle,
            phase: RuntimePhase::Idle,
            playback: PlaybackState::Playing,
            generation: 0,
            target: None,
            video_file: None,
            desired: DesiredRuntime::from_config(cfg),
            error: None,
        }
    }

    fn begin_session(&mut self, cfg: &Config) -> u64 {
        self.generation = self.generation.saturating_add(1);
        self.backend = gnome::resolve_backend(cfg);
        self.desired = DesiredRuntime::from_config(cfg);
        self.kind = RuntimeKind::Idle;
        self.phase = RuntimePhase::Starting;
        self.playback = PlaybackState::Playing;
        self.target = None;
        self.video_file = None;
        self.error = None;
        self.generation
    }

    fn begin_switch(&mut self, cfg: &Config) {
        self.backend = gnome::resolve_backend(cfg);
        self.desired = DesiredRuntime::from_config(cfg);
        self.phase = RuntimePhase::Switching;
        self.error = None;
    }

    fn finish_hot_switch(&mut self, cfg: &Config) {
        self.backend = gnome::resolve_backend(cfg);
        self.desired = DesiredRuntime::from_config(cfg);
        self.kind = RuntimeKind::Window;
        self.phase = RuntimePhase::Running;
        self.playback = PlaybackState::Playing;
        self.video_file = None;
        self.error = None;
    }

    fn mark_awaiting_window(&mut self, generation: u64) {
        if generation != self.generation {
            return;
        }
        self.kind = RuntimeKind::Window;
        self.phase = RuntimePhase::AwaitingWindow;
        self.target = None;
        self.video_file = None;
        self.error = None;
    }

    fn mark_running_window(&mut self, generation: u64, target: Option<RuntimeTarget>) {
        if generation != self.generation {
            return;
        }
        self.kind = RuntimeKind::Window;
        self.phase = RuntimePhase::Running;
        self.target = target;
        self.video_file = None;
        self.error = None;
    }

    fn mark_running_video(&mut self, generation: u64, video_file: String) {
        if generation != self.generation {
            return;
        }
        self.kind = RuntimeKind::Video;
        self.phase = RuntimePhase::Running;
        self.target = None;
        self.video_file = Some(video_file);
        self.error = None;
    }

    fn mark_stopping(&mut self, generation: u64) {
        if generation != self.generation {
            return;
        }
        self.phase = RuntimePhase::Stopping;
    }

    fn mark_idle(&mut self, generation: u64) {
        if generation != self.generation {
            return;
        }
        self.kind = RuntimeKind::Idle;
        self.phase = RuntimePhase::Idle;
        self.playback = PlaybackState::Playing;
        self.target = None;
        self.video_file = None;
        self.error = None;
    }

    fn mark_playback(&mut self, playback: PlaybackState) {
        self.playback = playback;
    }

    fn fail(&mut self, generation: u64, error: String) {
        if generation != self.generation {
            return;
        }
        self.phase = RuntimePhase::Failed;
        self.error = Some(error);
    }

    fn render_status_toml(&self) -> String {
        let mut lines = vec![
            "[orchestrator]".to_string(),
            format!("backend = \"{}\"", backend_name(self.backend)),
            format!("kind = \"{}\"", self.kind.as_str()),
            format!("phase = \"{}\"", self.phase.as_str()),
            format!("playback = \"{}\"", self.playback.as_str()),
            format!("generation = {}", self.generation),
            format!("desired_kind = \"{}\"", self.desired.kind.as_str()),
        ];
        if let Some(video_file) = &self.desired.video_file {
            lines.push(format!("desired_video_file = {:?}", video_file));
        }
        if let Some(video_file) = &self.video_file {
            lines.push(format!("video_file = {:?}", video_file));
        }
        if let Some(target) = &self.target {
            lines.push(format!("target_xid = {}", target.xid));
            lines.push(format!("target_pid = {}", target.pid));
            lines.push(format!("target_title = {:?}", target.title));
            lines.push(format!("target_wm_class = {:?}", target.wm_class));
        }
        if let Some(error) = &self.error {
            lines.push(format!("error = {:?}", error));
        }
        lines.join("\n")
    }
}

fn backend_name(backend: ResolvedBackend) -> &'static str {
    match backend {
        ResolvedBackend::LayerShell => "layer_shell",
        ResolvedBackend::GnomeShell => "gnome_shell",
    }
}

fn run_runtime_session(
    runtime_cfg: &Config,
    runtime_cgroup: &RuntimeCgroup,
    debug_visibility: &DebugWindowVisibility,
    shutdown_requested: &AtomicBool,
    runtime_state: &Arc<Mutex<RuntimeState>>,
    generation: u64,
    control_rx: &mpsc::Receiver<ControlCommand>,
) -> Result<RuntimeLoopExit> {
    if let Some(runtime) = &runtime_cfg.runtime {
        if runtime.mode == RuntimeMode::VideoNative {
            return run_video_native(
                runtime.video_file.as_deref(),
                runtime_cfg,
                runtime_state,
                generation,
                control_rx,
            );
        }
    }

    if shutdown_requested.load(Ordering::Relaxed) {
        return Ok(RuntimeLoopExit::Stop);
    }

    let mut runtime_cfg = runtime_cfg.clone();
    let _display_isolation = display_isolation::start_for_config(&mut runtime_cfg)?;
    if shutdown_requested.load(Ordering::Relaxed) {
        return Ok(RuntimeLoopExit::Stop);
    }

    info!(cfg = ?runtime_cfg, ?runtime_cfg.capture, "starting we-layerd run mode");
    ensure_runtime_access()?;
    let wine = WineProcessHandle::spawn(&runtime_cfg.wine)?;
    if shutdown_requested.load(Ordering::Relaxed) {
        return Ok(RuntimeLoopExit::Stop);
    }

    let cgroup_on_spawn = runtime_cgroup.clone();
    let on_spawn: Arc<dyn Fn(u32) + Send + Sync> = Arc::new(move |pid| {
        cgroup_on_spawn.on_wine_spawn(pid);
    });
    wine.install_exit_monitor(
        runtime_cfg.wine.clone(),
        runtime_cfg.general.restart_wine_on_exit,
        Some(on_spawn.clone()),
    );
    let wine_pid = wine.pid();
    if let Some(pid) = wine_pid {
        on_spawn(pid);
    }
    info!(pid = wine_pid, "wine launcher enabled");

    if let Ok(mut state) = runtime_state.lock() {
        state.mark_awaiting_window(generation);
    }

    let capture_window =
        match window_finder::find_window_for_process(&runtime_cfg.capture, wine_pid)? {
            Some(found) => {
                info!(window = found.window, scanned = found.scanned_windows, "using X11 window");
                apply_debug_window_setup(&runtime_cfg, debug_visibility, found.window);
                if let Some(path) = runtime_cfg.capture.debug_save_frame_png.as_deref() {
                    let frame = capture_xcomposite::capture_single_frame(found.window)?;
                    capture_xcomposite::save_frame_png(&frame, Path::new(path))?;
                    info!(path, "saved debug XComposite frame");
                }
                let target = RuntimeTarget {
                    xid: found.window,
                    pid: found.metadata.pid.unwrap_or_default(),
                    title: found.metadata.title.clone(),
                    wm_class: found.metadata.wm_class.clone(),
                };
                if let Ok(mut state) = runtime_state.lock() {
                    state.mark_running_window(generation, Some(target));
                }
                Some(found)
            }
            None => {
                warn!("no X11 window found yet, continuing with Wayland layer loop");
                None
            }
        };

    let exit = if matches!(gnome::resolve_backend(&runtime_cfg), ResolvedBackend::GnomeShell) {
        let runtime_state = runtime_state.clone();
        gnome::run_window_bridge(
            &runtime_cfg,
            &runtime_cfg.capture,
            capture_window,
            wine_pid,
            control_rx,
            move |registered| {
                if let Ok(mut state) = runtime_state.lock() {
                    match registered {
                        Some(window) => {
                            state
                                .mark_running_window(generation, Some(RuntimeTarget::from(window)));
                        }
                        None => state.mark_awaiting_window(generation),
                    }
                }
            },
        )?
    } else {
        if let Ok(mut state) = runtime_state.lock() {
            state.mark_running_window(generation, None);
        }
        wayland::layer_shell::run_single_background_surface(
            wayland::layer_shell::LayerRunConfig {
                capture_window: capture_window.as_ref().map(|found| found.window),
                output_window_map: runtime_cfg.capture.output_window_map.clone(),
                fps_limit: runtime_cfg.general.fps_limit,
                show_fps: runtime_cfg.general.show_fps,
                fps_report_interval_secs: runtime_cfg.general.fps_report_interval_secs,
                scale_mode: runtime_cfg.general.scale_mode,
                auto_refind_window: runtime_cfg.general.refind_window_on_capture_error,
                capture_match: runtime_cfg.capture.clone(),
                disable_debug_window_input: runtime_cfg.general.disable_debug_window_input,
                wine_pid,
            },
            Some(control_rx),
        )?
    };

    Ok(exit)
}

fn effective_runtime_config(cfg: &Config) -> Config {
    let mut runtime_cfg = cfg.clone();
    let mut capture_match = cfg.capture.clone();
    if capture_match.title_contains.is_none() {
        if let Some(title_hint) = extract_play_in_window_hint(&cfg.wine.args) {
            info!(title = %title_hint, "auto-derived capture.title_contains from wine args");
            capture_match.title_contains = Some(title_hint);
        }
    }
    runtime_cfg.capture = capture_match;
    if runtime_cfg.wine.args.iter().any(|arg| arg == "-playInWindow") {
        apply_xwayland_root_size(&mut runtime_cfg);
    }
    runtime_cfg
}

fn apply_xwayland_root_size(cfg: &mut Config) {
    if !cfg.wine.args.iter().any(|arg| arg == "-playInWindow") {
        return;
    }

    let Ok((root_width, root_height)) = window_input::current_root_size() else {
        return;
    };

    let mut adjusted = Vec::new();
    if let Some((before, after)) =
        replace_numeric_wine_arg(&mut cfg.wine.args, "-width", root_width)
    {
        adjusted.push(format!("-width: {before} -> {after}"));
    }
    if let Some((before, after)) =
        replace_numeric_wine_arg(&mut cfg.wine.args, "-height", root_height)
    {
        adjusted.push(format!("-height: {before} -> {after}"));
    }

    if !adjusted.is_empty() {
        info!(
            root_width,
            root_height,
            changes = adjusted.join(", "),
            "applied XWayland root-size wallpaper dimensions"
        );
    }
}

fn replace_numeric_wine_arg(
    args: &mut [String],
    flag: &str,
    replacement: u32,
) -> Option<(u32, u32)> {
    let index = args.iter().position(|arg| arg == flag)?;
    let value = args.get(index + 1)?.parse::<u32>().ok()?;
    let slot = args.get_mut(index + 1)?;
    if replacement == value {
        return None;
    }
    *slot = replacement.to_string();
    Some((value, replacement))
}

fn switch_wallpaper(cfg: &Config) -> Result<()> {
    ensure_switchable_runtime(cfg)?;
    let pid = spawn_transient_command(&cfg.wine)?;
    info!(pid, "sent openWallpaper command to running Wine wallpaper process");
    Ok(())
}

fn can_hot_switch_between(current_cfg: &Config, next_cfg: &Config) -> bool {
    ensure_switchable_runtime(current_cfg).is_ok()
        && ensure_switchable_runtime(next_cfg).is_ok()
        && current_cfg.isolation == next_cfg.isolation
}

fn ensure_switchable_runtime(cfg: &Config) -> Result<()> {
    let runtime = cfg
        .runtime
        .as_ref()
        .ok_or_else(|| anyhow!("runtime block is required for wallpaper switching"))?;
    if runtime.mode != RuntimeMode::WineLayerd {
        return Err(anyhow!("only scene/web wallpapers can be switched without restarting Wine"));
    }
    if !matches!(runtime.wallpaper_type, RuntimeWallpaperType::Scene | RuntimeWallpaperType::Web) {
        return Err(anyhow!("only scene/web wallpapers can be switched without restarting Wine"));
    }
    Ok(())
}

fn run_video_native(
    video_file: Option<&str>,
    cfg: &Config,
    runtime_state: &Arc<Mutex<RuntimeState>>,
    generation: u64,
    control_rx: &mpsc::Receiver<ControlCommand>,
) -> Result<RuntimeLoopExit> {
    let video = video_file
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| anyhow!("runtime.video_file is required when runtime.mode=video_native"))?;

    if let Ok(mut state) = runtime_state.lock() {
        state.mark_running_video(generation, video.to_string());
    }

    if matches!(gnome::resolve_backend(cfg), ResolvedBackend::GnomeShell) {
        info!(video, "starting GNOME video mode via shell extension");
        return gnome::run_video_bridge(cfg, Path::new(video), control_rx);
    }

    info!(video, "starting native video mode via ffmpeg + wgpu");
    wayland::layer_shell::run_video_background_surface(
        Path::new(video),
        cfg.general.fps_limit,
        cfg.general.show_fps,
        cfg.general.fps_report_interval_secs,
        cfg.general.scale_mode,
        Some(control_rx),
    )
}

fn handle_playback_control_command(
    cmd: ControlCommand,
    control_tx: &mpsc::Sender<ControlCommand>,
    runtime_state: &Arc<Mutex<RuntimeState>>,
) -> Result<bool> {
    let playback = match cmd {
        ControlCommand::Pause => PlaybackState::Paused,
        ControlCommand::Resume => PlaybackState::Playing,
        _ => return Ok(false),
    };

    control_tx
        .send(cmd)
        .with_context(|| format!("failed to forward {} command to runtime", cmd.as_str()))?;
    if let Ok(mut state) = runtime_state.lock() {
        state.mark_playback(playback);
    }
    Ok(true)
}

pub fn doctor() -> Result<()> {
    let cfg = Config::default();
    for key in ["WAYLAND_DISPLAY", "DISPLAY", "XAUTHORITY"] {
        match std::env::var(key) {
            Ok(value) => info!(%key, %value, "environment variable set"),
            Err(_) => warn!(%key, "environment variable not set"),
        }
    }

    match ensure_runtime_access() {
        Ok(()) => info!("X11 runtime access probe: OK"),
        Err(err) => warn!(error = %err, "X11 runtime access probe failed"),
    }

    match wayland::layer_shell::probe_layer_shell_support() {
        Ok(true) => info!("zwlr_layer_shell_v1 global is available"),
        Ok(false) => warn!("zwlr_layer_shell_v1 global not found on this compositor"),
        Err(err) => warn!(error = %err, "failed to query Wayland layer-shell support"),
    }

    if gnome::is_gnome_session() {
        match gnome::doctor(&cfg) {
            Ok(()) => {}
            Err(err) => warn!(error = %err, "GNOME extension D-Bus probe failed"),
        }
    }

    Ok(())
}

fn extract_play_in_window_hint(args: &[String]) -> Option<String> {
    let idx = args.iter().position(|arg| arg == "-playInWindow")?;
    args.get(idx + 1).cloned().filter(|s| !s.trim().is_empty())
}

fn ensure_runtime_access() -> Result<()> {
    capture_xcomposite::probe_xcomposite_support().map_err(|err| {
        let display = env::var("DISPLAY").ok();
        let xauthority = env::var("XAUTHORITY").ok();
        let hint = if err
            .to_string()
            .to_ascii_lowercase()
            .contains("invalid mit-magic-cookie-1 key")
        {
            "X11 authentication failed. Start we-layerd from the same graphical user session as XWayland, or export the matching X11 cookie file via XAUTHORITY before launching."
        } else {
            "X11 setup failed. Check DISPLAY, XAUTHORITY, and whether this process can access the active XWayland/X11 session."
        };

        err.context(format!(
            "{hint} DISPLAY={} XAUTHORITY={}",
            display.as_deref().unwrap_or("<unset>"),
            xauthority.as_deref().unwrap_or("<unset>")
        ))
    })
}

fn apply_debug_window_setup(cfg: &Config, debug_visibility: &DebugWindowVisibility, window: u32) {
    if let Err(err) = window_input::apply_wallpaper_window_hints(window) {
        warn!(error = %err, window, "failed to apply wallpaper window hints");
    }
    if cfg.general.disable_debug_window_input {
        if let Err(err) = window_input::set_mouse_passthrough(window) {
            warn!(error = %err, window, "failed to set debug window mouse passthrough");
        }
    }
    if debug_visibility.auto_hide && cfg.isolation.mode == IsolationMode::None {
        if let Err(err) = debug_visibility.hide() {
            warn!(error = %err, "failed to auto-hide debug window");
        }
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
    use super::{
        can_hot_switch_between, handle_playback_control_command, request_runtime_shutdown,
        RuntimeMode, RuntimePhase, RuntimeState, RuntimeWallpaperType,
    };
    use std::sync::atomic::AtomicBool;

    use crate::{
        config::{Config, IsolationMode, RuntimeConfig},
        ipc::{ControlCommand, PlaybackState},
    };

    #[test]
    fn hot_switch_requires_window_runtime_on_both_sides() {
        let current = Config {
            runtime: Some(RuntimeConfig {
                mode: RuntimeMode::WineLayerd,
                wallpaper_type: RuntimeWallpaperType::Scene,
                video_file: None,
            }),
            ..Config::default()
        };

        let mut next = current.clone();
        next.runtime = Some(RuntimeConfig {
            mode: RuntimeMode::VideoNative,
            wallpaper_type: RuntimeWallpaperType::Video,
            video_file: Some("/tmp/demo.mp4".to_string()),
        });

        assert!(!can_hot_switch_between(&current, &next));
        assert!(can_hot_switch_between(&current, &current));
    }

    #[test]
    fn hot_switch_rejects_isolation_boundary_changes() {
        let current = switchable_scene_config();
        let mut next = current.clone();
        next.isolation.mode = IsolationMode::GamescopeHeadless;
        assert!(!can_hot_switch_between(&current, &next));

        let mut next = current.clone();
        next.isolation.mode = IsolationMode::GamescopeHeadless;
        next.isolation.width = Some(2560);
        assert!(!can_hot_switch_between(&current, &next));
    }

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
    fn stale_generation_does_not_override_current_phase() {
        let cfg = Config::default();
        let mut state = RuntimeState::new(&cfg);
        let generation = state.begin_session(&cfg);
        state.mark_running_window(generation, None);
        let current_generation = state.begin_session(&cfg);

        state.mark_stopping(generation);

        assert_eq!(state.generation, current_generation);
        assert_eq!(state.phase, RuntimePhase::Starting);
    }

    #[test]
    fn playback_status_reports_pause_state() {
        let cfg = Config::default();
        let mut state = RuntimeState::new(&cfg);
        let generation = state.begin_session(&cfg);
        state.mark_running_window(generation, None);

        state.mark_playback(PlaybackState::Paused);

        assert_eq!(state.playback, PlaybackState::Paused);
        assert!(state.render_status_toml().contains("playback = \"paused\""));
    }

    #[test]
    fn playback_control_forwards_command_and_updates_status() {
        let cfg = Config::default();
        let runtime_state = std::sync::Arc::new(std::sync::Mutex::new(RuntimeState::new(&cfg)));
        let (tx, rx) = std::sync::mpsc::channel();

        assert!(
            handle_playback_control_command(ControlCommand::Pause, &tx, &runtime_state).unwrap()
        );
        assert_eq!(rx.try_recv().expect("pause command"), ControlCommand::Pause);
        assert_eq!(runtime_state.lock().expect("runtime state").playback, PlaybackState::Paused);

        assert!(
            handle_playback_control_command(ControlCommand::Resume, &tx, &runtime_state).unwrap()
        );
        assert_eq!(rx.try_recv().expect("resume command"), ControlCommand::Resume);
        assert_eq!(runtime_state.lock().expect("runtime state").playback, PlaybackState::Playing);
    }

    #[test]
    fn hot_switch_finishes_in_playing_state_after_pause() {
        let cfg = switchable_scene_config();
        let mut state = RuntimeState::new(&cfg);
        let generation = state.begin_session(&cfg);
        state.mark_running_window(generation, None);
        state.mark_playback(PlaybackState::Paused);

        state.begin_switch(&cfg);
        state.finish_hot_switch(&cfg);

        assert_eq!(state.playback, PlaybackState::Playing);
    }

    fn switchable_scene_config() -> Config {
        Config {
            runtime: Some(RuntimeConfig {
                mode: RuntimeMode::WineLayerd,
                wallpaper_type: RuntimeWallpaperType::Scene,
                video_file: None,
            }),
            ..Config::default()
        }
    }
}
