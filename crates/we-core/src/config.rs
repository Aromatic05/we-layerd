use std::{fs, path::Path};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::wallpaper::WallpaperType;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default)]
    pub general: GeneralConfig,
    #[serde(default)]
    pub renderer: RendererConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneralConfig {
    #[serde(default)]
    pub backend: Backend,
    #[serde(default = "default_interactive")]
    pub interactive: bool,
    #[serde(default)]
    pub show_fps: bool,
    #[serde(default = "default_fps_report_interval_secs")]
    pub fps_report_interval_secs: u64,
    #[serde(default)]
    pub scale_mode: ScaleMode,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Backend {
    #[default]
    LayerShell,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ScaleMode {
    Fit,
    Cover,
    Stretch,
}

impl Default for ScaleMode {
    fn default() -> Self {
        Self::Cover
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RendererConfig {
    #[serde(default = "default_renderer_library_path")]
    pub library_path: String,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub assets_path: String,
    #[serde(default = "default_renderer_cache_path")]
    pub cache_path: String,
    #[serde(default = "default_prefer_dmabuf")]
    pub prefer_dmabuf: bool,
    #[serde(default = "default_allow_shm_fallback")]
    pub allow_shm_fallback: bool,
    #[serde(default = "default_renderer_fps")]
    pub fps: u32,
    #[serde(default = "default_renderer_speed")]
    pub speed: f32,
    #[serde(default = "default_renderer_volume")]
    pub volume: f32,
    #[serde(default)]
    pub muted: bool,
}

#[derive(Debug, Clone)]
pub struct LaunchSettings {
    pub wallpaper_exe: String,
    pub workshop_path: String,
    pub launcher: WindowsLauncher,
    pub wine_command: String,
    pub proton_path: Option<String>,
    pub fps_limit: u32,
    pub show_fps: bool,
    pub isolation_mode: IsolationMode,
    pub isolation_command: String,
    pub isolation_width: Option<u32>,
    pub isolation_height: Option<u32>,
    pub isolation_startup_timeout_secs: u64,
    pub width: u32,
    pub height: u32,
    pub x: i32,
    pub y: i32,
    pub play_in_window_title: String,
    pub borderless: bool,
    pub wm_class_contains: String,
    pub cgroup_enabled: bool,
    pub cgroup_mode: CgroupMode,
    pub cgroup_memory_max: Option<String>,
    pub cgroup_cpu_max: Option<String>,
    pub hide_debug_window: bool,
    pub hidden_workspace_name: String,
    pub disable_debug_window_input: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum IsolationMode {
    None,
    GamescopeHeadless,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CgroupMode {
    Detect,
    LimitWine,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WindowsLauncher {
    Wine,
    Proton,
}

fn default_interactive() -> bool {
    true
}

fn default_fps_report_interval_secs() -> u64 {
    1
}

fn default_renderer_library_path() -> String {
    "libwallpaper-engine-renderer.so".to_string()
}

fn default_renderer_cache_path() -> String {
    "~/.cache/we-layerd/renderer".to_string()
}

fn default_prefer_dmabuf() -> bool {
    true
}

fn default_allow_shm_fallback() -> bool {
    true
}

fn default_renderer_fps() -> u32 {
    60
}

fn default_renderer_speed() -> f32 {
    1.0
}

fn default_renderer_volume() -> f32 {
    1.0
}

impl Default for RendererConfig {
    fn default() -> Self {
        Self {
            library_path: default_renderer_library_path(),
            source: String::new(),
            assets_path: String::new(),
            cache_path: default_renderer_cache_path(),
            prefer_dmabuf: default_prefer_dmabuf(),
            allow_shm_fallback: default_allow_shm_fallback(),
            fps: default_renderer_fps(),
            speed: default_renderer_speed(),
            volume: default_renderer_volume(),
            muted: false,
        }
    }
}

impl Default for GeneralConfig {
    fn default() -> Self {
        Self {
            backend: Backend::default(),
            interactive: default_interactive(),
            show_fps: false,
            fps_report_interval_secs: default_fps_report_interval_secs(),
            scale_mode: ScaleMode::default(),
        }
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        Self { general: GeneralConfig::default(), renderer: RendererConfig::default() }
    }
}

impl Default for LaunchSettings {
    fn default() -> Self {
        Self {
            wallpaper_exe: String::new(),
            workshop_path: String::new(),
            launcher: WindowsLauncher::Wine,
            wine_command: "wine".to_string(),
            proton_path: None,
            fps_limit: 60,
            show_fps: false,
            isolation_mode: IsolationMode::None,
            isolation_command: "gamescope".to_string(),
            isolation_width: None,
            isolation_height: None,
            isolation_startup_timeout_secs: 10,
            width: 2560,
            height: 1600,
            x: 0,
            y: 0,
            play_in_window_title: "WE-DEBUG-WINDOW".to_string(),
            borderless: true,
            wm_class_contains: "wallpaper64".to_string(),
            cgroup_enabled: false,
            cgroup_mode: CgroupMode::Detect,
            cgroup_memory_max: None,
            cgroup_cpu_max: None,
            hide_debug_window: true,
            hidden_workspace_name: "top".to_string(),
            disable_debug_window_input: false,
        }
    }
}

pub fn build_config(
    settings: &LaunchSettings,
    _wallpaper_type: WallpaperType,
    project_json: &Path,
    _video_file: Option<&Path>,
) -> AppConfig {
    let mut cfg = AppConfig::default();
    cfg.general.show_fps = settings.show_fps;
    cfg.renderer.fps = settings.fps_limit.clamp(1, 360);
    cfg.renderer.source = project_json.parent().unwrap_or(project_json).display().to_string();
    cfg.renderer.assets_path = derive_assets_path(&settings.wallpaper_exe);
    cfg
}

fn derive_assets_path(wallpaper_exe: &str) -> String {
    let exe_path = Path::new(wallpaper_exe);
    exe_path
        .parent()
        .map(|parent| parent.join("assets"))
        .unwrap_or_else(|| Path::new("assets").to_path_buf())
        .display()
        .to_string()
}

pub fn save_config(path: &Path, config: &AppConfig) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }

    let toml = toml::to_string_pretty(config).context("failed to serialize config")?;
    fs::write(path, toml).with_context(|| format!("failed to write {}", path.display()))
}

pub fn load_launch_settings(path: &Path) -> Result<LaunchSettings> {
    let raw =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    let cfg: AppConfig =
        toml::from_str(&raw).with_context(|| format!("invalid TOML in {}", path.display()))?;

    let mut settings = LaunchSettings::default();
    settings.show_fps = cfg.general.show_fps;
    settings.fps_limit = cfg.renderer.fps.max(1);
    settings.workshop_path = derive_workshop_root(&cfg.renderer.source);
    settings.wallpaper_exe = derive_wallpaper_exe(&cfg.renderer.assets_path);
    Ok(settings)
}

fn derive_workshop_root(source: &str) -> String {
    let source_path = Path::new(source);
    source_path.parent().map(|path| path.display().to_string()).unwrap_or_default()
}

fn derive_wallpaper_exe(assets_path: &str) -> String {
    let assets = Path::new(assets_path);
    assets
        .parent()
        .map(|parent| parent.join("wallpaper64.exe"))
        .unwrap_or_else(|| Path::new("wallpaper64.exe").to_path_buf())
        .display()
        .to_string()
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::{Path, PathBuf},
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::{build_config, load_launch_settings, LaunchSettings};
    use crate::wallpaper::WallpaperType;

    fn unique_temp_path(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time before unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("we-layerd-{name}-{}-{nanos}", std::process::id()))
    }

    #[test]
    fn build_config_writes_renderer_native_source_and_assets() {
        let mut settings = LaunchSettings::default();
        settings.wallpaper_exe =
            "/steam/steamapps/common/wallpaper_engine/wallpaper64.exe".to_string();
        settings.fps_limit = 144;

        let cfg = build_config(
            &settings,
            WallpaperType::Scene,
            Path::new("/tmp/item/project.json"),
            None,
        );

        assert_eq!(cfg.renderer.source, "/tmp/item");
        assert_eq!(cfg.renderer.assets_path, "/steam/steamapps/common/wallpaper_engine/assets");
        assert_eq!(cfg.renderer.fps, 144);
    }

    #[test]
    fn load_launch_settings_reads_renderer_native_config() {
        let path = unique_temp_path("renderer-config.toml");
        let toml = r#"
[general]
backend = "layer_shell"
interactive = true
show_fps = true
fps_report_interval_secs = 1
scale_mode = "cover"

[renderer]
library_path = "libwallpaper-engine-renderer.so"
source = "/tmp/workshop/content/431960/1234"
assets_path = "/opt/wallpaper_engine/assets"
cache_path = "~/.cache/we-layerd/renderer"
prefer_dmabuf = true
allow_shm_fallback = true
fps = 120
speed = 1.0
volume = 1.0
muted = false
"#;

        fs::write(&path, toml).expect("failed to write temp config");

        let settings = load_launch_settings(&path).expect("renderer config should load");
        assert_eq!(settings.fps_limit, 120);
        assert!(settings.show_fps);
        assert_eq!(settings.workshop_path, "/tmp/workshop/content/431960");
        assert_eq!(settings.wallpaper_exe, "/opt/wallpaper_engine/wallpaper64.exe");

        let _ = fs::remove_file(path);
    }
}
