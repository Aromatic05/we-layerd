use std::{
    ffi::OsStr,
    io,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::Duration,
};

const LAYERD_EXECUTABLE: &str = "we-layerd";
const SYSTEM_BINARY_DIRS: [&str; 2] = ["/usr/bin", "/usr/local/bin"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DaemonStatus {
    NotRunning,
    EmptyResponse,
    Running(String),
}

pub fn layerd_is_available() -> bool {
    resolve_layerd_executable().is_some()
}

pub fn try_switch(config_path: &Path) -> bool {
    let Ok(mut command) = layerd_command() else {
        return false;
    };
    command.arg("switch").arg("--config").arg(config_path);
    command_succeeds_quietly(&mut command)
}

pub fn send_control(action: &str) -> bool {
    let Ok(mut command) = layerd_command() else {
        return false;
    };
    command.arg("ctl").arg(action);
    command_succeeds_quietly(&mut command)
}

pub fn play_playlist(name: &str) -> bool {
    let Ok(mut command) = layerd_command() else {
        return false;
    };
    command.arg("playlist").arg("play").arg(name);
    command_succeeds_quietly(&mut command)
}

pub fn send_playlist_action(action: &str) -> bool {
    let Ok(mut command) = layerd_command() else {
        return false;
    };
    command.arg("playlist").arg(action);
    command_succeeds_quietly(&mut command)
}

pub fn start(config_path: &Path) -> std::io::Result<Child> {
    layerd_command()?.arg("run").arg("--config").arg(config_path).spawn()
}

pub fn restart(config_path: &Path, child: &mut Option<Child>) -> Result<Child, String> {
    let _ = stop(child);

    for _ in 0..40 {
        if !daemon_is_running() {
            return start(config_path).map_err(|error| error.to_string());
        }
        std::thread::sleep(Duration::from_millis(50));
    }

    Err("timed out waiting for the previous daemon to stop".to_string())
}

pub async fn fetch_status() -> Result<DaemonStatus, String> {
    let output = layerd_command()
        .map_err(|error| error.to_string())?
        .arg("ctl")
        .arg("status")
        .output()
        .map_err(|error| error.to_string())?;

    Ok(classify_daemon_status(output.status.success(), &output.stdout))
}

pub async fn fetch_outputs() -> Result<Vec<String>, String> {
    let output = layerd_command()
        .map_err(|error| error.to_string())?
        .arg("outputs")
        .output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    serde_json::from_slice(&output.stdout).map_err(|error| error.to_string())
}

pub fn reap(child: &mut Option<Child>) -> Result<(), String> {
    let Some(process) = child.as_mut() else {
        return Ok(());
    };

    match process.try_wait() {
        Ok(Some(_)) => {
            *child = None;
            Ok(())
        }
        Ok(None) => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}

pub fn stop(child: &mut Option<Child>) -> bool {
    let mut stopped = send_control("stop");
    if let Some(mut child) = child.take() {
        for _ in 0..3 {
            if child.try_wait().map_or(true, |status| status.is_some()) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        let _ = child.kill();
        let _ = child.wait();
        stopped = true;
    }
    stopped || !daemon_is_running()
}

fn daemon_is_running() -> bool {
    layerd_command()
        .ok()
        .and_then(|mut command| command.arg("ctl").arg("status").output().ok())
        .is_some_and(|output| output.status.success())
}

fn layerd_command() -> io::Result<Command> {
    resolve_layerd_executable().map(Command::new).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "we-layerd was not found in PATH, ~/.local/bin, /usr/bin, or /usr/local/bin",
        )
    })
}

fn resolve_layerd_executable() -> Option<PathBuf> {
    let path = std::env::var_os("PATH");
    let home = std::env::var_os("HOME");
    let system_dirs = SYSTEM_BINARY_DIRS.map(Path::new);
    resolve_executable_from(LAYERD_EXECUTABLE, path.as_deref(), home.as_deref(), &system_dirs)
}

fn resolve_executable_from(
    name: &str,
    path: Option<&OsStr>,
    home: Option<&OsStr>,
    system_dirs: &[&Path],
) -> Option<PathBuf> {
    executable_candidates(name, path, home, system_dirs)
        .into_iter()
        .find(|candidate| is_executable_file(candidate))
}

fn executable_candidates(
    name: &str,
    path: Option<&OsStr>,
    home: Option<&OsStr>,
    system_dirs: &[&Path],
) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(path) = path {
        for directory in std::env::split_paths(path) {
            push_unique(&mut candidates, directory.join(name));
        }
    }
    if let Some(home) = home {
        push_unique(&mut candidates, PathBuf::from(home).join(".local/bin").join(name));
    }
    for directory in system_dirs {
        push_unique(&mut candidates, directory.join(name));
    }
    candidates
}

fn push_unique(candidates: &mut Vec<PathBuf>, candidate: PathBuf) {
    if !candidates.contains(&candidate) {
        candidates.push(candidate);
    }
}

fn is_executable_file(path: &Path) -> bool {
    let Ok(metadata) = path.metadata() else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }

    #[cfg(not(unix))]
    {
        true
    }
}

fn command_succeeds_quietly(command: &mut Command) -> bool {
    command
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

fn classify_daemon_status(succeeded: bool, stdout: &[u8]) -> DaemonStatus {
    if !succeeded {
        return DaemonStatus::NotRunning;
    }

    let text = String::from_utf8_lossy(stdout).trim().to_string();
    if text.is_empty() {
        DaemonStatus::EmptyResponse
    } else {
        DaemonStatus::Running(text)
    }
}

#[cfg(test)]
mod tests {
    use std::{
        ffi::OsStr,
        fs,
        os::unix::fs::PermissionsExt,
        path::{Path, PathBuf},
        process::Command,
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::{
        classify_daemon_status, command_succeeds_quietly, executable_candidates,
        resolve_executable_from, DaemonStatus,
    };

    const QUIET_CHILD_ENV: &str = "WE_GUI_TEST_QUIET_CONTROL_CHILD";
    const NOISY_CHILD_OUTPUT: &str = "we-gui-control-probe-noise";

    #[test]
    fn failed_status_query_means_daemon_is_not_running() {
        assert_eq!(classify_daemon_status(false, b"ignored error"), DaemonStatus::NotRunning);
    }

    #[test]
    fn successful_status_query_preserves_runtime_snapshot() {
        assert_eq!(
            classify_daemon_status(true, b" phase=running\n"),
            DaemonStatus::Running("phase=running".to_string())
        );
        assert_eq!(classify_daemon_status(true, b"\n"), DaemonStatus::EmptyResponse);
    }

    #[test]
    fn executable_candidates_use_path_before_standard_fallbacks() {
        let candidates = executable_candidates(
            "we-layerd",
            Some(OsStr::new("/custom/bin:/second/bin")),
            Some(OsStr::new("/home/tester")),
            &[Path::new("/usr/bin"), Path::new("/usr/local/bin")],
        );

        assert_eq!(
            candidates,
            [
                "/custom/bin/we-layerd",
                "/second/bin/we-layerd",
                "/home/tester/.local/bin/we-layerd",
                "/usr/bin/we-layerd",
                "/usr/local/bin/we-layerd",
            ]
            .map(PathBuf::from)
        );
    }

    #[cfg(unix)]
    #[test]
    fn executable_resolution_falls_back_to_home_local_bin() {
        let suffix = SystemTime::now().duration_since(UNIX_EPOCH).expect("system clock").as_nanos();
        let root = std::env::temp_dir().join(format!("we-gui-layerd-resolution-{suffix}"));
        let path_dir = root.join("path-bin");
        let home = root.join("home");
        let local_bin = home.join(".local/bin");
        fs::create_dir_all(&path_dir).expect("create PATH directory");
        fs::create_dir_all(&local_bin).expect("create local bin directory");

        let non_executable = path_dir.join("we-layerd");
        fs::write(&non_executable, "not executable").expect("write PATH candidate");
        let fallback = local_bin.join("we-layerd");
        fs::write(&fallback, "#!/bin/sh\n").expect("write fallback executable");
        let mut permissions = fs::metadata(&fallback).expect("fallback metadata").permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&fallback, permissions).expect("make fallback executable");

        let resolved = resolve_executable_from(
            "we-layerd",
            Some(path_dir.as_os_str()),
            Some(home.as_os_str()),
            &[],
        );

        assert_eq!(resolved, Some(fallback));
        fs::remove_dir_all(root).expect("remove resolution fixture");
    }

    #[cfg(unix)]
    #[test]
    fn quiet_control_command_does_not_forward_child_stderr() {
        let output = Command::new(std::env::current_exe().expect("current test executable"))
            .arg("quiet_control_command_child")
            .arg("--nocapture")
            .env(QUIET_CHILD_ENV, "1")
            .output()
            .expect("run child test process");

        assert!(output.status.success());
        assert!(!String::from_utf8_lossy(&output.stdout).contains(NOISY_CHILD_OUTPUT));
        assert!(!String::from_utf8_lossy(&output.stderr).contains(NOISY_CHILD_OUTPUT));
    }

    #[cfg(unix)]
    #[test]
    fn quiet_control_command_child() {
        if std::env::var_os(QUIET_CHILD_ENV).is_none() {
            return;
        }

        let succeeded = command_succeeds_quietly(
            Command::new("sh").arg("-c").arg(format!("printf {NOISY_CHILD_OUTPUT} >&2; exit 1")),
        );

        assert!(!succeeded);
    }
}
