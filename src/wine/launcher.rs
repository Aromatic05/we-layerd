use std::{
    fs, io,
    os::unix::process::CommandExt,
    path::Path,
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

use anyhow::{anyhow, Context, Result};
use tracing::{info, warn};

use crate::config::{WineCommandMode, WineConfig};

pub struct WineProcessHandle {
    child: Arc<Mutex<Option<Child>>>,
    pgid: Arc<Mutex<Option<i32>>>,
    stop: Arc<AtomicBool>,
    owner: bool,
}

impl Clone for WineProcessHandle {
    fn clone(&self) -> Self {
        Self {
            child: self.child.clone(),
            pgid: self.pgid.clone(),
            stop: self.stop.clone(),
            owner: false,
        }
    }
}

impl WineProcessHandle {
    pub fn spawn(config: &WineConfig) -> Result<Self> {
        let child = spawn_child(config)?;
        let pgid = child.id() as i32;
        info!(pid = child.id(), pgid, "spawned wine wallpaper process");
        suppress_bootstrap_windows(child.id() as i32);
        Ok(Self {
            child: Arc::new(Mutex::new(Some(child))),
            pgid: Arc::new(Mutex::new(Some(pgid))),
            stop: Arc::new(AtomicBool::new(false)),
            owner: true,
        })
    }

    pub fn install_exit_monitor(
        &self,
        config: WineConfig,
        restart_on_exit: bool,
        on_spawn: Option<Arc<dyn Fn(u32) + Send + Sync>>,
    ) {
        let child = self.child.clone();
        let pgid = self.pgid.clone();
        let stop = self.stop.clone();
        std::thread::spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                let mut guard = match child.lock() {
                    Ok(guard) => guard,
                    Err(_) => {
                        warn!("wine child process lock poisoned");
                        break;
                    }
                };

                if let Some(proc) = guard.as_mut() {
                    match proc.try_wait() {
                        Ok(Some(status)) => {
                            warn!(?status, "wine process exited");
                            *guard = None;
                            if let Ok(mut pgid_guard) = pgid.lock() {
                                *pgid_guard = None;
                            }

                            if stop.load(Ordering::Relaxed) {
                                drop(guard);
                                break;
                            }

                            let should_restart = restart_on_exit && !status.success();
                            if should_restart {
                                match spawn_child(&config) {
                                    Ok(new_child) => {
                                        let new_pgid = new_child.id() as i32;
                                        if let Some(cb) = &on_spawn {
                                            cb(new_child.id());
                                        }
                                        info!(
                                            pid = new_child.id(),
                                            pgid = new_pgid,
                                            "restarted wine process"
                                        );
                                        suppress_bootstrap_windows(new_child.id() as i32);
                                        if let Ok(mut pgid_guard) = pgid.lock() {
                                            *pgid_guard = Some(new_pgid);
                                        }
                                        *guard = Some(new_child);
                                    }
                                    Err(err) => {
                                        warn!(error = %err, "failed to restart wine process");
                                    }
                                }
                            } else if status.success() {
                                info!("wine process exited successfully; skip auto-restart");
                            }
                        }
                        Ok(None) => {}
                        Err(err) => {
                            warn!(error = %err, "failed to poll wine process status");
                        }
                    }
                }

                drop(guard);
                std::thread::sleep(Duration::from_secs(2));
            }
        });
    }

    pub fn pid(&self) -> Option<u32> {
        self.child.lock().ok().and_then(|guard| guard.as_ref().map(std::process::Child::id))
    }

    pub fn terminate(&self) -> Result<()> {
        self.stop.store(true, Ordering::Relaxed);

        let pgid = {
            let guard =
                self.pgid.lock().map_err(|_| anyhow!("wine process group lock poisoned"))?;
            *guard
        };

        if let Some(pgid) = pgid {
            terminate_process_group(pgid)?;
        }

        let mut guard =
            self.child.lock().map_err(|_| anyhow!("wine child process lock poisoned"))?;

        if let Some(child) = guard.as_mut() {
            let _ = child.wait();
            info!("wine process terminated");
        }

        *guard = None;
        if let Ok(mut pgid_guard) = self.pgid.lock() {
            *pgid_guard = None;
        }
        Ok(())
    }
}

pub fn spawn_transient_command(config: &WineConfig) -> Result<u32> {
    let mut child = spawn_child(config)?;
    let pid = child.id();
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(pid)
}

impl Drop for WineProcessHandle {
    fn drop(&mut self) {
        if !self.owner {
            return;
        }

        if let Err(err) = self.terminate() {
            warn!(error = %err, "failed to cleanup wine process on drop");
        }
    }
}

fn spawn_child(config: &WineConfig) -> Result<Child> {
    maybe_disable_wine_x11_window_decorations(config)?;

    let mut cmd = Command::new(&config.command);
    let mut working_dir: Option<&Path> = None;
    let wine_args = normalize_args_for_wine_process(&config.args);
    match config.command_mode {
        WineCommandMode::ExeWithArgs => {
            if config.wallpaper_exe.is_empty() {
                return Err(anyhow!(
                    "wine.wallpaper_exe is empty; set the Wallpaper Engine executable path"
                ));
            }

            let exe_path = Path::new(&config.wallpaper_exe);
            if !exe_path.exists() {
                return Err(anyhow!("Wallpaper executable does not exist: {}", exe_path.display()));
            }
            working_dir = exe_path.parent();
            cmd.arg(&config.wallpaper_exe).args(&wine_args);
            if should_force_borderless(config) {
                cmd.arg("-borderless");
            }
        }
        WineCommandMode::CommandOnly => {
            if !config.wallpaper_exe.trim().is_empty() {
                working_dir = Path::new(&config.wallpaper_exe).parent();
            }
            cmd.args(&wine_args);
        }
    }

    if let Some(dir) = working_dir {
        cmd.current_dir(dir);
    }

    if !config.env.is_empty() {
        cmd.envs(config.env.iter());
    }

    cmd.stdout(Stdio::inherit()).stderr(Stdio::inherit());

    unsafe {
        cmd.pre_exec(|| {
            if libc::setpgid(0, 0) != 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }

    cmd.spawn().with_context(|| {
        format!("failed to spawn wine command '{}' for {}", config.command, config.wallpaper_exe)
    })
}

fn normalize_args_for_wine_process(args: &[String]) -> Vec<String> {
    let mut normalized = args.to_vec();
    let mut index = 0;

    while index + 1 < normalized.len() {
        if normalized[index] == "-file" {
            normalized[index + 1] = wine_path_arg(&normalized[index + 1]);
            index += 2;
        } else {
            index += 1;
        }
    }

    normalized
}

fn wine_path_arg(value: &str) -> String {
    if is_windows_path(value) || value.contains("://") || !value.starts_with('/') {
        return value.to_string();
    }

    format!("Z:{}", value.replace('/', "\\"))
}

fn is_windows_path(value: &str) -> bool {
    let bytes = value.as_bytes();
    matches!(bytes.get(1), Some(b':')) || value.starts_with("\\\\")
}

fn maybe_disable_wine_x11_window_decorations(config: &WineConfig) -> Result<()> {
    if config.command_mode != WineCommandMode::ExeWithArgs {
        return Ok(());
    }

    if !should_force_borderless(config) {
        return Ok(());
    }

    let mut cmd = Command::new(&config.command);
    cmd.args([
        "reg",
        "add",
        "HKCU\\Software\\Wine\\X11 Driver",
        "/v",
        "Decorated",
        "/t",
        "REG_SZ",
        "/d",
        "N",
        "/f",
    ]);

    if !config.env.is_empty() {
        cmd.envs(config.env.iter());
    }

    let output = cmd.output().with_context(|| {
        format!("failed to run '{}' to disable Wine X11 decorations", config.command)
    })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        return Err(anyhow!(
            "failed to disable Wine X11 decorations via registry (status: {:?}, stdout: {}, stderr: {})",
            output.status.code(),
            stdout,
            stderr
        ));
    }

    info!("disabled Wine X11 window decorations for borderless launch");
    Ok(())
}

fn should_force_borderless(config: &WineConfig) -> bool {
    config.args.iter().any(|arg| arg == "-borderless")
        || config.args.iter().any(|arg| arg == "-playInWindow")
}

fn terminate_process_group(pgid: i32) -> Result<()> {
    if !signal_process_group(-pgid, libc::SIGTERM)? {
        return Ok(());
    }

    for _ in 0..20 {
        if !signal_process_group(-pgid, 0)? {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(100));
    }

    signal_process_group(-pgid, libc::SIGKILL)?;
    Ok(())
}

fn signal_process_group(pgid: i32, signal: i32) -> Result<bool> {
    let rc = unsafe { libc::kill(pgid, signal) };
    if rc == 0 {
        return Ok(true);
    }
    let err = io::Error::last_os_error();
    if err.raw_os_error() == Some(libc::ESRCH) {
        return Ok(false);
    }
    Err(err).with_context(|| format!("failed to send signal {} to process group {}", signal, pgid))
}

fn suppress_bootstrap_windows(root_pid: i32) {
    std::thread::spawn(move || {
        let names = ["explorer.exe", "ui32.exe"];
        // Let Wine/X11 finish creating the real wallpaper window before pruning helpers.
        std::thread::sleep(Duration::from_secs(5));
        for _ in 0..12 {
            if !is_process_alive(root_pid) {
                break;
            }
            let _ = kill_named_processes_for_current_user(&names);
            std::thread::sleep(Duration::from_millis(500));
        }
    });
}

fn kill_named_processes_for_current_user(names: &[&str]) -> Result<()> {
    let uid = unsafe { libc::geteuid() };
    let entries = fs::read_dir("/proc").context("failed to read /proc")?;
    for entry in entries.flatten() {
        let Some(pid_str) = entry.file_name().to_str().map(ToString::to_string) else {
            continue;
        };
        let Ok(pid) = pid_str.parse::<i32>() else {
            continue;
        };
        if !is_process_owned_by_uid(pid, uid) {
            continue;
        }
        let cmdline_path = format!("/proc/{pid}/cmdline");
        let cmd = fs::read(&cmdline_path)
            .ok()
            .map(|raw| String::from_utf8_lossy(&raw).to_ascii_lowercase())
            .unwrap_or_default();
        let comm_path = format!("/proc/{pid}/comm");
        let comm =
            fs::read_to_string(&comm_path).ok().map(|s| s.to_ascii_lowercase()).unwrap_or_default();
        if names.iter().any(|n| cmd.contains(n) || comm.contains(n)) {
            let _ = unsafe { libc::kill(pid, libc::SIGTERM) };
            std::thread::sleep(Duration::from_millis(60));
            let _ = unsafe { libc::kill(pid, libc::SIGKILL) };
        }
    }
    Ok(())
}

fn is_process_owned_by_uid(pid: i32, uid: u32) -> bool {
    let status_path = format!("/proc/{pid}/status");
    let Ok(raw) = fs::read_to_string(status_path) else {
        return false;
    };
    for line in raw.lines() {
        if let Some(rest) = line.strip_prefix("Uid:") {
            let real_uid = rest.split_whitespace().next().and_then(|n| n.parse::<u32>().ok());
            return real_uid == Some(uid);
        }
    }
    false
}

fn is_process_alive(pid: i32) -> bool {
    let rc = unsafe { libc::kill(pid, 0) };
    if rc == 0 {
        return true;
    }
    let err = io::Error::last_os_error();
    err.raw_os_error() == Some(libc::EPERM)
}

#[cfg(test)]
mod tests {
    use super::normalize_args_for_wine_process;

    #[test]
    fn normalize_file_arg_to_wine_path() {
        let args = vec![
            "-control".to_string(),
            "openWallpaper".to_string(),
            "-file".to_string(),
            "/home/user/workshop/827982449/project.json".to_string(),
        ];

        let normalized = normalize_args_for_wine_process(&args);

        assert_eq!(
            normalized,
            vec![
                "-control",
                "openWallpaper",
                "-file",
                r"Z:\home\user\workshop\827982449\project.json",
            ]
        );
    }
}
