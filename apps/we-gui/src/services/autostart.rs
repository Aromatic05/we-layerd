use std::{
    env, fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

const UNIT_NAME: &str = "we-layerd.service";
const GUI_UNIT_NAME: &str = "we-gui.service";
const MANAGED_UNIT_MARKER: &str =
    "# Managed by we-gui. Do not edit while using the GUI autostart setting.";
const SYSTEM_MANAGED_UNIT_MARKER: &str = "X-Managed-By=NixOS services.we-layerd.enable";

#[derive(Debug, Clone, Copy)]
pub struct AutostartStatus {
    pub enabled: bool,
    pub system_managed: bool,
}

pub fn status() -> Result<AutostartStatus, String> {
    let unit = systemctl(&["cat", UNIT_NAME])?;
    let system_managed = if unit.status.success() {
        String::from_utf8_lossy(&unit.stdout).contains(SYSTEM_MANAGED_UNIT_MARKER)
    } else if unit_missing(&unit) {
        false
    } else {
        return Err(output_error("failed to inspect the systemd user unit", &unit));
    };

    let output = systemctl(&["is-enabled", UNIT_NAME])?;
    if output.status.success() {
        return Ok(AutostartStatus { enabled: true, system_managed });
    }
    if unit_missing(&output) {
        return Ok(AutostartStatus { enabled: false, system_managed });
    }

    match stdout_state(&output).as_str() {
        "disabled" | "masked" | "static" | "indirect" => {
            Ok(AutostartStatus { enabled: false, system_managed })
        }
        _ => Err(output_error("failed to query systemd user autostart state", &output)),
    }
}

pub fn set_enabled(enabled: bool, config_path: &Path) -> Result<(), String> {
    if status()?.system_managed {
        return Err("we-layerd autostart is managed by the system configuration".to_string());
    }
    if enabled {
        ensure_unit(config_path)?;
        run_systemctl(&["enable", UNIT_NAME], "failed to enable we-layerd at login")
    } else {
        let output = systemctl(&["disable", UNIT_NAME])?;
        if !output.status.success() && !unit_missing(&output) {
            return Err(output_error("failed to disable we-layerd at login", &output));
        }
        remove_managed_unit()
    }
}

pub fn restart_if_active() -> Result<bool, String> {
    if !is_active()? {
        return Ok(false);
    }
    run_systemctl(&["restart", UNIT_NAME], "failed to restart systemd-managed we-layerd")?;
    Ok(true)
}

/// we-gui's own login unit. It starts the tray without opening a window, so
/// the mute/pause menu is available from login even after the GUI is closed.
pub fn gui_status() -> Result<bool, String> {
    let output = systemctl(&["is-enabled", GUI_UNIT_NAME])?;
    if output.status.success() {
        return Ok(true);
    }
    if unit_missing(&output) {
        return Ok(false);
    }
    match stdout_state(&output).as_str() {
        "disabled" | "masked" | "static" | "indirect" => Ok(false),
        _ => Err(output_error("failed to query the we-gui autostart state", &output)),
    }
}

pub fn gui_set_enabled(enabled: bool) -> Result<(), String> {
    if enabled {
        if !packaged_gui_unit_available()? {
            let executable = gui_executable()?;
            let unit_path = generated_path_for(GUI_UNIT_NAME)?;
            let unit = render_gui_unit(&executable);
            if fs::read_to_string(&unit_path).ok().as_deref() != Some(unit.as_str()) {
                if let Some(parent) = unit_path.parent() {
                    fs::create_dir_all(parent).map_err(|error| {
                        format!(
                            "failed to create systemd user unit directory {}: {error}",
                            parent.display()
                        )
                    })?;
                }
                fs::write(&unit_path, unit)
                    .map_err(|error| format!("failed to write {}: {error}", unit_path.display()))?;
            }
            run_systemctl(&["daemon-reload"], "failed to reload the systemd user manager")?;
        }
        run_systemctl(&["enable", GUI_UNIT_NAME], "failed to enable we-gui at login")
    } else {
        let output = systemctl(&["disable", GUI_UNIT_NAME])?;
        if !output.status.success() && !unit_missing(&output) {
            return Err(output_error("failed to disable we-gui at login", &output));
        }
        remove_managed_unit_named(GUI_UNIT_NAME)
    }
}

/// A unit installed by the distribution must not be shadowed by our generated one.
fn packaged_gui_unit_available() -> Result<bool, String> {
    let output = systemctl(&["cat", GUI_UNIT_NAME])?;
    if output.status.success() {
        return Ok(!String::from_utf8_lossy(&output.stdout).contains(MANAGED_UNIT_MARKER));
    }
    if unit_missing(&output) {
        return Ok(false);
    }
    Err(output_error("failed to inspect the we-gui systemd user unit", &output))
}

/// The launcher path for we-gui. Inside an AppImage `current_exe()` points into
/// the temporary mount, which is gone by the next login, so use the AppImage.
/// `AppRun` forwards unknown arguments such as `--tray` to we-gui.
fn gui_executable() -> Result<PathBuf, String> {
    if let Some(appimage) = env::var_os("APPIMAGE").filter(|value| !value.is_empty()) {
        return Ok(PathBuf::from(appimage));
    }
    env::current_exe().map_err(|error| format!("failed to resolve the we-gui executable: {error}"))
}

fn render_gui_unit(executable: &Path) -> String {
    format!(
        "{MANAGED_UNIT_MARKER}\n[Unit]\nDescription=we-gui wallpaper picker (tray)\nPartOf=graphical-session.target\nAfter=graphical-session-pre.target\n\n[Service]\nType=simple\nExecStart={} --tray\nRestart=on-failure\nRestartSec=2\n\n[Install]\nWantedBy=graphical-session.target\n",
        unit_quote(executable),
    )
}

fn is_active() -> Result<bool, String> {
    let output = systemctl(&["is-active", UNIT_NAME])?;
    if output.status.success() {
        return Ok(true);
    }
    if unit_missing(&output) {
        return Ok(false);
    }

    match stdout_state(&output).as_str() {
        "inactive" | "failed" | "unknown" => Ok(false),
        // These states still mean systemd owns the unit. Let systemd arbitrate the restart
        // rather than replacing it with a GUI-owned process.
        "activating" | "reloading" | "deactivating" => Ok(true),
        _ => Err(output_error("failed to query systemd user service state", &output)),
    }
}

fn ensure_unit(config_path: &Path) -> Result<(), String> {
    let appimage = env::var_os("APPIMAGE").filter(|value| !value.is_empty()).map(PathBuf::from);
    if appimage.is_none() && packaged_unit_available()? {
        return Ok(());
    }

    let executable = match appimage {
        Some(path) => Launcher::AppImage(path),
        None => Launcher::Binary(super::runtime::layerd_executable().ok_or_else(|| {
            "we-layerd executable was not found for systemd autostart".to_string()
        })?),
    };
    let unit_path = generated_unit_path()?;
    let unit = render_unit(&executable, config_path);
    if fs::read_to_string(&unit_path).ok().as_deref() != Some(unit.as_str()) {
        if let Some(parent) = unit_path.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                format!(
                    "failed to create systemd user unit directory {}: {error}",
                    parent.display()
                )
            })?;
        }
        fs::write(&unit_path, unit)
            .map_err(|error| format!("failed to write {}: {error}", unit_path.display()))?;
    }
    run_systemctl(&["daemon-reload"], "failed to reload the systemd user manager")
}

fn packaged_unit_available() -> Result<bool, String> {
    let output = systemctl(&["cat", UNIT_NAME])?;
    if output.status.success() {
        let unit = String::from_utf8_lossy(&output.stdout);
        return Ok(!unit.contains(MANAGED_UNIT_MARKER));
    }
    if unit_missing(&output) {
        return Ok(false);
    }
    Err(output_error("failed to inspect the systemd user unit", &output))
}

fn generated_unit_path() -> Result<PathBuf, String> {
    generated_path_for(UNIT_NAME)
}

fn generated_path_for(unit_name: &str) -> Result<PathBuf, String> {
    let config_home = env::var_os("XDG_CONFIG_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .ok_or_else(|| "HOME and XDG_CONFIG_HOME are both unavailable".to_string())?;
    Ok(config_home.join("systemd/user").join(unit_name))
}

fn remove_managed_unit() -> Result<(), String> {
    remove_managed_unit_named(UNIT_NAME)
}

fn remove_managed_unit_named(unit_name: &str) -> Result<(), String> {
    let path = generated_path_for(unit_name)?;
    let Some(contents) = fs::read_to_string(&path).ok() else {
        return Ok(());
    };
    if !contents.contains(MANAGED_UNIT_MARKER) {
        return Ok(());
    }
    fs::remove_file(&path)
        .map_err(|error| format!("failed to remove {}: {error}", path.display()))?;
    run_systemctl(&["daemon-reload"], "failed to reload the systemd user manager")
}

#[derive(Debug)]
enum Launcher {
    Binary(PathBuf),
    AppImage(PathBuf),
}

fn render_unit(launcher: &Launcher, config_path: &Path) -> String {
    let executable = match launcher {
        Launcher::Binary(path) => unit_quote(path),
        Launcher::AppImage(path) => format!("{} --cli", unit_quote(path)),
    };
    format!(
        "{MANAGED_UNIT_MARKER}\n[Unit]\nDescription=we-layerd wallpaper daemon\nPartOf=graphical-session.target\nAfter=graphical-session-pre.target\nConditionPathExists={}\n\n[Service]\nType=simple\nExecStart={executable} run --config {}\nRestart=on-failure\nRestartSec=2\n\n[Install]\nWantedBy=graphical-session.target\n",
        unit_quote(config_path),
        unit_quote(config_path),
    )
}

fn unit_quote(path: &Path) -> String {
    let value = path.to_string_lossy();
    let escaped =
        value.replace('\\', "\\\\").replace('"', "\\\"").replace('%', "%%").replace('$', "$$");
    format!("\"{escaped}\"")
}

fn systemctl(args: &[&str]) -> Result<Output, String> {
    Command::new("systemctl")
        .arg("--user")
        .args(args)
        .output()
        .map_err(|error| format!("failed to run systemctl --user: {error}"))
}

fn run_systemctl(args: &[&str], context: &str) -> Result<(), String> {
    let output = systemctl(args)?;
    if output.status.success() {
        Ok(())
    } else {
        Err(output_error(context, &output))
    }
}

fn stdout_state(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn unit_missing(output: &Output) -> bool {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    stdout.contains("not-found")
        || stderr.contains("could not be found")
        || stderr.contains("does not exist")
        || stderr.contains("No files found")
        || stderr.contains("not found")
}

fn output_error(context: &str, output: &Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let stdout = stdout_state(output);
    let detail = if !stderr.is_empty() { stderr } else { stdout };
    if detail.is_empty() {
        context.to_string()
    } else {
        format!("{context}: {detail}")
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{render_unit, unit_quote, Launcher, MANAGED_UNIT_MARKER};

    #[test]
    fn generated_appimage_unit_uses_cli_launcher_and_config() {
        let unit = render_unit(
            &Launcher::AppImage("/home/test/My Wallpapers.AppImage".into()),
            Path::new("/home/test/.config/we-layerd/config.toml"),
        );
        assert!(unit.starts_with(MANAGED_UNIT_MARKER));
        assert!(unit.contains(
            "ExecStart=\"/home/test/My Wallpapers.AppImage\" --cli run --config \"/home/test/.config/we-layerd/config.toml\""
        ));
        assert!(unit.contains("PartOf=graphical-session.target"));
        assert!(unit.contains("WantedBy=graphical-session.target"));
    }

    #[test]
    fn generated_binary_unit_uses_resolved_daemon() {
        let unit = render_unit(
            &Launcher::Binary("/home/test/.local/bin/we-layerd".into()),
            Path::new("/home/test/.config/we-layerd/config.toml"),
        );
        assert!(unit.contains(
            "ExecStart=\"/home/test/.local/bin/we-layerd\" run --config \"/home/test/.config/we-layerd/config.toml\""
        ));
    }

    #[test]
    fn systemd_paths_escape_specifiers() {
        assert_eq!(unit_quote(Path::new("/home/100%/$demo")), "\"/home/100%%/$$demo\"");
    }

    #[test]
    fn generated_gui_unit_starts_the_tray_without_a_window() {
        let unit = super::render_gui_unit(Path::new("/home/test/.local/bin/we-gui"));
        assert!(unit.starts_with(MANAGED_UNIT_MARKER));
        assert!(unit.contains("ExecStart=\"/home/test/.local/bin/we-gui\" --tray"));
        assert!(!unit.contains("ConditionPathExists"));
        assert!(unit.contains("PartOf=graphical-session.target"));
        assert!(unit.contains("WantedBy=graphical-session.target"));
    }
}
