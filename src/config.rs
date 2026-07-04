use std::{fs, path::Path};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Config {
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
        }
    }
}

impl RendererConfig {
    pub fn options_json_diagnostics(&self) -> (bool, usize, bool) {
        let present = self.options_json.is_some();
        let len = self.options_json.as_ref().map(|value| value.len()).unwrap_or(0);
        let valid = self
            .options_json
            .as_ref()
            .map(|value| serde_json::from_str::<serde_json::Value>(value).is_ok())
            .unwrap_or(true);
        (present, len, valid)
    }

    pub fn validate_options_json(&self) -> Result<()> {
        if let Some(raw) = &self.options_json {
            serde_json::from_str::<serde_json::Value>(raw)
                .context("renderer.options_json must be valid JSON")?;
        }
        Ok(())
    }
}

impl Config {
    pub fn load(path: Option<&Path>) -> Result<Self> {
        match path {
            Some(path) => {
                let raw = fs::read_to_string(path)
                    .with_context(|| format!("failed to read config file: {}", path.display()))?;
                toml::from_str(&raw)
                    .with_context(|| format!("invalid TOML in config file: {}", path.display()))
            }
            None => Ok(Self::default()),
        }
    }

    pub fn to_toml_pretty(&self) -> Result<String> {
        toml::to_string_pretty(self).context("failed to serialize config")
    }
}

#[cfg(test)]
mod tests {
    use super::{Backend, Config, ScaleMode};

    #[test]
    fn default_config_uses_renderer_native_defaults() {
        let cfg = Config::default();
        assert_eq!(cfg.general.backend, Backend::LayerShell);
        assert!(cfg.general.interactive);
        assert_eq!(cfg.general.scale_mode, ScaleMode::Cover);
        assert!(cfg.renderer.library_path.is_empty());
        assert_eq!(cfg.renderer.fps, 60);
        assert!(cfg.renderer.prefer_dmabuf);
        assert!(cfg.renderer.allow_shm_fallback);
    }

    #[test]
    fn config_accepts_renderer_block() {
        let raw = r#"
            [renderer]
            source = "/tmp/workshop/item"
            assets_path = "/tmp/wallpaper_engine/assets"
            muted = true
            options_json = "{\"hello\":true}"
        "#;

        let cfg: Config = toml::from_str(raw).expect("valid renderer config");

        assert_eq!(cfg.renderer.source, "/tmp/workshop/item");
        assert_eq!(cfg.renderer.assets_path, "/tmp/wallpaper_engine/assets");
        assert!(cfg.renderer.muted);
        assert_eq!(cfg.renderer.options_json.as_deref(), Some("{\"hello\":true}"));
    }
}
