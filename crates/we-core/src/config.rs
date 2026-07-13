use std::{collections::BTreeMap, fs, path::Path};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::wallpaper::settings::{RenderResolution, WallpaperFillMode, WallpaperSettings};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppConfig {
    #[serde(default)]
    pub general: GeneralConfig,
    #[serde(default)]
    pub renderer: RendererConfig,
    #[serde(default)]
    pub wallpapers: BTreeMap<String, WallpaperSettings>,
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

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ScaleMode {
    Fit,
    #[default]
    Cover,
    Stretch,
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
    #[serde(default)]
    pub options_json: Option<String>,
    #[serde(default)]
    pub render_width: Option<u32>,
    #[serde(default)]
    pub render_height: Option<u32>,
    #[serde(default)]
    pub fill_mode: WallpaperFillMode,
    #[serde(default)]
    pub rotation_degrees: u32,
}

#[derive(Debug, Clone)]
pub struct LaunchSettings {
    pub assets_path: String,
    pub workshop_path: String,
    pub renderer_library_path: String,
    pub renderer_cache_path: String,
    pub prefer_dmabuf: bool,
    pub allow_shm_fallback: bool,
    pub interactive: bool,
    pub fps_limit: u32,
    pub show_fps: bool,
    pub scale_mode: ScaleMode,
    pub options_json: Option<String>,
    pub wallpapers: BTreeMap<String, WallpaperSettings>,
}

fn default_interactive() -> bool {
    true
}

fn default_fps_report_interval_secs() -> u64 {
    1
}

fn default_renderer_library_path() -> String {
    String::new()
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
            options_json: None,
            render_width: None,
            render_height: None,
            fill_mode: WallpaperFillMode::Cover,
            rotation_degrees: 0,
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

impl Default for LaunchSettings {
    fn default() -> Self {
        Self {
            assets_path: String::new(),
            workshop_path: String::new(),
            renderer_library_path: default_renderer_library_path(),
            renderer_cache_path: default_renderer_cache_path(),
            prefer_dmabuf: default_prefer_dmabuf(),
            allow_shm_fallback: default_allow_shm_fallback(),
            interactive: true,
            fps_limit: 60,
            show_fps: false,
            scale_mode: ScaleMode::Cover,
            options_json: None,
            wallpapers: BTreeMap::new(),
        }
    }
}

pub fn build_config(settings: &LaunchSettings, project_json: &Path) -> AppConfig {
    let mut cfg = AppConfig::default();
    cfg.general.interactive = settings.interactive;
    cfg.general.show_fps = settings.show_fps;
    cfg.general.scale_mode = settings.scale_mode;
    cfg.renderer.library_path = settings.renderer_library_path.clone();
    cfg.renderer.cache_path = settings.renderer_cache_path.clone();
    cfg.renderer.prefer_dmabuf = settings.prefer_dmabuf;
    cfg.renderer.allow_shm_fallback = settings.allow_shm_fallback;
    cfg.renderer.fps = settings.fps_limit.clamp(1, 360);
    cfg.renderer.options_json = settings.options_json.clone();
    cfg.renderer.source = project_json.parent().unwrap_or(project_json).display().to_string();
    cfg.renderer.assets_path = settings.assets_path.clone();
    cfg
}

pub fn build_config_for_wallpaper(
    settings: &LaunchSettings,
    wallpaper_id: &str,
    project_json: &Path,
) -> AppConfig {
    let mut config = build_config(settings, project_json);
    let wallpaper = settings.wallpapers.get(wallpaper_id).cloned().unwrap_or_default();
    config.renderer.fps = wallpaper.fps.clamp(1, 360);
    config.renderer.speed = wallpaper.speed;
    config.renderer.volume = wallpaper.volume;
    config.renderer.muted = wallpaper.muted;
    config.renderer.fill_mode = wallpaper.fill_mode;
    config.renderer.rotation_degrees = wallpaper.rotation_degrees.degrees();
    match wallpaper.render_resolution {
        RenderResolution::Automatic => {
            config.renderer.render_width = None;
            config.renderer.render_height = None;
        }
        RenderResolution::Fixed { width, height } => {
            config.renderer.render_width = Some(width.max(1));
            config.renderer.render_height = Some(height.max(1));
        }
    }
    config.renderer.options_json = Some(
        serde_json::json!({
            "version": 1,
            "scene": { "userProperties": wallpaper.user_properties },
        })
        .to_string(),
    );
    config.wallpapers = settings.wallpapers.clone();
    config
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

    Ok(LaunchSettings {
        assets_path: cfg.renderer.assets_path.clone(),
        workshop_path: derive_workshop_root(&cfg.renderer.source),
        renderer_library_path: cfg.renderer.library_path,
        renderer_cache_path: cfg.renderer.cache_path,
        prefer_dmabuf: cfg.renderer.prefer_dmabuf,
        allow_shm_fallback: cfg.renderer.allow_shm_fallback,
        interactive: cfg.general.interactive,
        fps_limit: cfg.renderer.fps.max(1),
        show_fps: cfg.general.show_fps,
        scale_mode: cfg.general.scale_mode,
        options_json: cfg.renderer.options_json,
        wallpapers: cfg.wallpapers,
    })
}

fn derive_workshop_root(source: &str) -> String {
    let source_path = Path::new(source);
    source_path.parent().map(|path| path.display().to_string()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::{Path, PathBuf},
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::{build_config, build_config_for_wallpaper, load_launch_settings, LaunchSettings, ScaleMode};
    use crate::wallpaper::settings::{RenderResolution, Rotation, WallpaperFillMode, WallpaperSettings};

    fn unique_temp_path(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time before unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("we-layerd-{name}-{}-{nanos}", std::process::id()))
    }

    #[test]
    fn build_config_writes_renderer_native_source_and_assets() {
        let settings = LaunchSettings {
            assets_path: "/steam/steamapps/common/wallpaper_engine/assets".to_string(),
            fps_limit: 144,
            interactive: false,
            scale_mode: ScaleMode::Fit,
            options_json: Some("{\"demo\":true}".to_string()),
            ..LaunchSettings::default()
        };

        let cfg = build_config(&settings, Path::new("/tmp/item/project.json"));

        assert_eq!(cfg.renderer.source, "/tmp/item");
        assert_eq!(cfg.renderer.assets_path, "/steam/steamapps/common/wallpaper_engine/assets");
        assert_eq!(cfg.renderer.fps, 144);
        assert_eq!(cfg.renderer.options_json.as_deref(), Some("{\"demo\":true}"));
        assert!(!cfg.general.interactive);
        assert_eq!(cfg.general.scale_mode, ScaleMode::Fit);
    }

    #[test]
    fn build_config_for_wallpaper_uses_only_the_selected_wallpaper_profile() {
        let mut settings = LaunchSettings::default();
        let mut user_properties = std::collections::BTreeMap::new();
        user_properties.insert("language".to_string(), serde_json::json!("3"));
        settings.wallpapers.insert(
            "alpha".to_string(),
            WallpaperSettings {
                fps: 144,
                speed: 1.5,
                volume: 0.4,
                muted: true,
                render_resolution: RenderResolution::Fixed { width: 2560, height: 1440 },
                fill_mode: WallpaperFillMode::Fit,
                rotation_degrees: Rotation::Deg90,
                user_properties,
                ..WallpaperSettings::default()
            },
        );

        let cfg = build_config_for_wallpaper(&settings, "alpha", Path::new("/tmp/alpha/project.json"));
        assert_eq!(cfg.renderer.fps, 144);
        assert_eq!(cfg.renderer.speed, 1.5);
        assert_eq!(cfg.renderer.volume, 0.4);
        assert!(cfg.renderer.muted);
        assert_eq!(cfg.renderer.render_width, Some(2560));
        assert_eq!(cfg.renderer.render_height, Some(1440));
        assert_eq!(cfg.renderer.fill_mode, WallpaperFillMode::Fit);
        assert_eq!(cfg.renderer.rotation_degrees, 90);
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(
                cfg.renderer.options_json.as_deref().expect("source options"),
            )
            .expect("valid source options"),
            serde_json::json!({
                "version": 1,
                "scene": { "userProperties": { "language": "3" } }
            })
        );
    }

    #[test]
    fn load_launch_settings_reads_renderer_native_config() {
        let path = unique_temp_path("renderer-config.toml");
        let toml = r#"
[general]
backend = "layer_shell"
interactive = false
show_fps = true
fps_report_interval_secs = 1
scale_mode = "stretch"

[renderer]
library_path = "/opt/libwallpaper-engine-renderer.so"
source = "/tmp/workshop/content/431960/1234"
assets_path = "/opt/wallpaper_engine/assets"
cache_path = "~/.cache/we-layerd/custom"
prefer_dmabuf = false
allow_shm_fallback = true
fps = 120
speed = 1.0
volume = 1.0
muted = false
options_json = "{\"keep\":true}"
"#;

        fs::write(&path, toml).expect("failed to write temp config");

        let settings = load_launch_settings(&path).expect("renderer config should load");
        assert_eq!(settings.fps_limit, 120);
        assert!(settings.show_fps);
        assert!(!settings.interactive);
        assert_eq!(settings.scale_mode, ScaleMode::Stretch);
        assert_eq!(settings.renderer_library_path, "/opt/libwallpaper-engine-renderer.so");
        assert_eq!(settings.renderer_cache_path, "~/.cache/we-layerd/custom");
        assert!(!settings.prefer_dmabuf);
        assert!(settings.allow_shm_fallback);
        assert_eq!(settings.options_json.as_deref(), Some("{\"keep\":true}"));
        assert_eq!(settings.workshop_path, "/tmp/workshop/content/431960");
        assert_eq!(settings.assets_path, "/opt/wallpaper_engine/assets");

        let _ = fs::remove_file(path);
    }

    #[test]
    fn launch_settings_default_to_auto_renderer_resolution() {
        let settings = LaunchSettings::default();
        assert!(settings.renderer_library_path.is_empty());
    }
}
