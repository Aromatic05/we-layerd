use std::{env, path::PathBuf};

const STEAM_COMMON_PATHS: &[&str] = &[
    ".local/share/Steam/steamapps/common",
    ".var/app/com.valvesoftware.Steam/.local/share/Steam/steamapps/common",
    "snap/steam/common/.local/share/Steam/steamapps/common",
];

const STEAM_WORKSHOP_PATHS: &[&str] = &[
    ".local/share/Steam/steamapps/workshop/content",
    ".var/app/com.valvesoftware.Steam/.local/share/Steam/steamapps/workshop/content",
    "snap/steam/common/.local/share/Steam/steamapps/workshop/content",
];

pub const WALLPAPER_ENGINE_APP_ID: u32 = 431960;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WallpaperEngineInstallState {
    NotInstalled,
    FirstRunRequired { app_dir: PathBuf },
    Installed { app_dir: PathBuf, exe_path: PathBuf },
}

pub fn discover_wallpaper_engine_exe() -> Option<PathBuf> {
    let names = ["wallpaper64.exe", "wallpaper32.exe"];
    for root in steam_common_roots() {
        let app_dir = root.join("wallpaper_engine");
        for name in names {
            let candidate = app_dir.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

pub fn detect_wallpaper_engine_install_state(wallpaper_exe: &str) -> WallpaperEngineInstallState {
    if wallpaper_exe.trim().is_empty() {
        for root in steam_common_roots() {
            let app_dir = root.join("wallpaper_engine");
            if !app_dir.is_dir() {
                continue;
            }

            let exe64 = app_dir.join("wallpaper64.exe");
            if exe64.is_file() {
                return WallpaperEngineInstallState::Installed { app_dir, exe_path: exe64 };
            }

            let exe32 = app_dir.join("wallpaper32.exe");
            if exe32.is_file() {
                return WallpaperEngineInstallState::Installed { app_dir, exe_path: exe32 };
            }

            if app_dir.join("installer.exe").is_file() {
                return WallpaperEngineInstallState::FirstRunRequired { app_dir };
            }
        }
    } else {
        let exe_path = PathBuf::from(wallpaper_exe);
        let mut app_dir = exe_path.clone();
        app_dir.pop();
        let is_supported_exe = exe_path
            .file_name()
            .and_then(|name| name.to_str())
            .map(|name| {
                matches!(name.to_ascii_lowercase().as_str(), "wallpaper64.exe" | "wallpaper32.exe")
            })
            .unwrap_or(false);
        if exe_path.is_file() && is_supported_exe {
            return WallpaperEngineInstallState::Installed { app_dir, exe_path };
        }
    }
    WallpaperEngineInstallState::NotInstalled
}

pub fn discover_workshop_wallpaper_root() -> Option<PathBuf> {
    for root in steam_workshop_roots() {
        let candidate = root.join(WALLPAPER_ENGINE_APP_ID.to_string());
        if candidate.is_dir() {
            return Some(candidate);
        }
    }
    None
}

pub fn discover_wallpaper_engine_app_dir() -> Option<PathBuf> {
    for root in steam_common_roots() {
        let app_dir = root.join("wallpaper_engine");
        if app_dir.is_dir() {
            return Some(app_dir);
        }
    }
    None
}

pub fn default_config_path() -> Option<PathBuf> {
    let home = env::var_os("HOME")?;
    Some(PathBuf::from(home).join(".config/we-layerd/config.toml"))
}

fn steam_common_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(home) = env::var_os("HOME") {
        let base = PathBuf::from(home);
        for rel in STEAM_COMMON_PATHS {
            roots.push(base.join(rel));
        }
    }
    roots
}

fn steam_workshop_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(home) = env::var_os("HOME") {
        let base = PathBuf::from(home);
        for rel in STEAM_WORKSHOP_PATHS {
            roots.push(base.join(rel));
        }
    }
    roots
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::{detect_wallpaper_engine_install_state, WallpaperEngineInstallState};

    fn unique_temp_path(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time before unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("we-layerd-{name}-{}-{nanos}", std::process::id()))
    }

    #[test]
    fn detect_install_state_accepts_custom_wallpaper32_exe() {
        let app_dir = unique_temp_path("wallpaper-engine");
        fs::create_dir_all(&app_dir).expect("failed to create app dir");
        let exe_path = app_dir.join("wallpaper32.exe");
        fs::write(&exe_path, b"").expect("failed to create exe");

        let state = detect_wallpaper_engine_install_state(exe_path.to_str().expect("utf-8 path"));
        match state {
            WallpaperEngineInstallState::Installed {
                app_dir: detected_dir,
                exe_path: detected_exe,
            } => {
                assert_eq!(detected_dir, app_dir);
                assert_eq!(detected_exe, exe_path);
            }
            other => panic!("expected installed state, got {other:?}"),
        }

        let _ = fs::remove_file(exe_path);
        let _ = fs::remove_dir(app_dir);
    }
}
