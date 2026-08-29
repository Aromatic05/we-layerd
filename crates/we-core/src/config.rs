use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    io::Write,
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

use crate::{
    playlist::PlaylistConfig,
    profile::ProfileConfig,
    wallpaper::settings::{RenderResolution, WallpaperFillMode, WallpaperSettings},
};

static CONFIG_TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppConfig {
    #[serde(default)]
    pub general: GeneralConfig,
    #[serde(default)]
    pub renderer: RendererConfig,
    #[serde(default, skip_serializing_if = "HooksConfig::is_empty")]
    pub hooks: HooksConfig,
    #[serde(default)]
    pub wallpapers: BTreeMap<String, WallpaperSettings>,
    #[serde(default)]
    pub playlists: PlaylistConfig,
    #[serde(default)]
    pub outputs: BTreeMap<String, OutputBinding>,
    #[serde(default)]
    pub profiles: ProfileConfig,
    #[serde(default)]
    pub integrations: IntegrationsConfig,
    #[serde(default)]
    pub rules: RuntimeRulesConfig,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct OutputBinding {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wallpaper_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub playlist: Option<String>,
}

impl OutputBinding {
    pub fn wallpaper(wallpaper_id: impl Into<String>, source: impl Into<String>) -> Self {
        Self {
            wallpaper_id: Some(wallpaper_id.into()),
            source: Some(source.into()),
            playlist: None,
        }
    }

    pub fn playlist(name: impl Into<String>) -> Self {
        Self { wallpaper_id: None, source: None, playlist: Some(name.into()) }
    }

    pub fn is_ambiguous(&self) -> bool {
        self.playlist.is_some() && (self.wallpaper_id.is_some() || self.source.is_some())
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct HooksConfig {
    #[serde(default)]
    pub wallpaper_applied: Option<HookCommand>,
}

impl HooksConfig {
    pub fn is_empty(&self) -> bool {
        self.wallpaper_applied.is_none()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HookCommand {
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
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
    #[serde(default)]
    pub force_scene_audio_loop: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IntegrationsConfig {
    #[serde(default = "default_media_integration")]
    pub media: bool,
    #[serde(default)]
    pub audio_spectrum: bool,
    #[serde(default = "default_audio_source")]
    pub audio_source: String,
    #[serde(default = "default_audio_sample_rate")]
    pub audio_sample_rate: u32,
    #[serde(default = "default_audio_update_hz")]
    pub audio_update_hz: u32,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeRuleAction {
    #[default]
    Keep,
    Mute,
    Pause,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimeRulesConfig {
    #[serde(default)]
    pub focused: RuntimeRuleAction,
    #[serde(default)]
    pub maximized: RuntimeRuleAction,
    #[serde(default)]
    pub fullscreen: RuntimeRuleAction,
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
    /// Hard upper bound applied to every wallpaper profile.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_fps: Option<u32>,
    #[serde(default = "default_renderer_speed")]
    pub speed: f32,
    #[serde(default = "default_renderer_volume")]
    pub volume: f32,
    #[serde(default)]
    pub muted: bool,
    #[serde(default = "default_renderer_msaa_samples")]
    pub msaa_samples: u32,
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
    pub msaa_samples: u32,
    pub show_fps: bool,
    pub scale_mode: ScaleMode,
    pub force_scene_audio_loop: bool,
    pub options_json: Option<String>,
    pub hooks: HooksConfig,
    pub wallpapers: BTreeMap<String, WallpaperSettings>,
    pub playlists: PlaylistConfig,
    pub outputs: BTreeMap<String, OutputBinding>,
    pub profiles: ProfileConfig,
    pub integrations: IntegrationsConfig,
    pub rules: RuntimeRulesConfig,
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

fn default_renderer_msaa_samples() -> u32 {
    1
}

fn default_media_integration() -> bool {
    true
}

fn default_audio_source() -> String {
    "@DEFAULT_MONITOR@".to_string()
}

fn default_audio_sample_rate() -> u32 {
    48_000
}

fn default_audio_update_hz() -> u32 {
    30
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
            max_fps: None,
            speed: default_renderer_speed(),
            volume: default_renderer_volume(),
            muted: false,
            msaa_samples: default_renderer_msaa_samples(),
            options_json: None,
            render_width: None,
            render_height: None,
            fill_mode: WallpaperFillMode::Cover,
            rotation_degrees: 0,
        }
    }
}

impl Default for IntegrationsConfig {
    fn default() -> Self {
        Self {
            media: default_media_integration(),
            audio_spectrum: false,
            audio_source: default_audio_source(),
            audio_sample_rate: default_audio_sample_rate(),
            audio_update_hz: default_audio_update_hz(),
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
            force_scene_audio_loop: false,
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
            msaa_samples: 1,
            show_fps: false,
            scale_mode: ScaleMode::Cover,
            force_scene_audio_loop: false,
            options_json: None,
            hooks: HooksConfig::default(),
            wallpapers: BTreeMap::new(),
            playlists: PlaylistConfig::default(),
            outputs: BTreeMap::new(),
            profiles: ProfileConfig::default(),
            integrations: IntegrationsConfig::default(),
            rules: RuntimeRulesConfig::default(),
        }
    }
}

pub fn build_config(settings: &LaunchSettings, project_json: &Path) -> AppConfig {
    let mut cfg = AppConfig::default();
    cfg.general.interactive = settings.interactive;
    cfg.general.show_fps = settings.show_fps;
    cfg.general.scale_mode = settings.scale_mode;
    cfg.general.force_scene_audio_loop = settings.force_scene_audio_loop;
    cfg.renderer.library_path = settings.renderer_library_path.clone();
    cfg.renderer.cache_path = settings.renderer_cache_path.clone();
    cfg.renderer.prefer_dmabuf = settings.prefer_dmabuf;
    cfg.renderer.allow_shm_fallback = settings.allow_shm_fallback;
    cfg.renderer.fps = settings.fps_limit.clamp(1, 360);
    cfg.renderer.max_fps = Some(settings.fps_limit.clamp(1, 360));
    cfg.renderer.msaa_samples = settings.msaa_samples.max(1);
    cfg.renderer.options_json = settings.options_json.clone();
    cfg.hooks = settings.hooks.clone();
    cfg.playlists = settings.playlists.clone();
    cfg.outputs = settings.outputs.clone();
    cfg.profiles = settings.profiles.clone();
    cfg.integrations = settings.integrations.clone();
    cfg.rules = settings.rules;
    cfg.renderer.source = project_json.parent().unwrap_or(project_json).display().to_string();
    cfg.renderer.assets_path =
        Path::new(&settings.assets_path).join("assets").display().to_string();
    cfg
}

pub fn build_config_for_wallpaper(
    settings: &LaunchSettings,
    wallpaper_id: &str,
    project_json: &Path,
) -> Result<AppConfig> {
    let mut config = build_config(settings, project_json);
    let wallpaper =
        settings.wallpapers.get(wallpaper_id).cloned().unwrap_or_else(|| WallpaperSettings {
            msaa_samples: settings.msaa_samples.max(1),
            ..WallpaperSettings::default()
        });
    let fps_limit = config.renderer.max_fps.unwrap_or(settings.fps_limit).clamp(1, 360);
    config.renderer.fps = wallpaper.fps.min(fps_limit).clamp(1, 360);
    config.renderer.speed = wallpaper.speed;
    config.renderer.volume = wallpaper.volume;
    config.renderer.muted = wallpaper.muted;
    config.renderer.msaa_samples = wallpaper.msaa_samples.max(1);
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
    config.renderer.options_json = Some(merge_scene_source_options(
        settings.options_json.as_deref(),
        Some(wallpaper.user_properties),
        settings.force_scene_audio_loop,
    )?);
    config.wallpapers = settings.wallpapers.clone();
    config.playlists = settings.playlists.clone();
    config.outputs = settings.outputs.clone();
    config.profiles = settings.profiles.clone();
    Ok(config)
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
        assets_path: Path::new(&cfg.renderer.assets_path)
            .parent()
            .unwrap_or_else(|| Path::new(""))
            .display()
            .to_string(),
        workshop_path: derive_workshop_root(&cfg.renderer.source),
        renderer_library_path: cfg.renderer.library_path,
        renderer_cache_path: cfg.renderer.cache_path,
        prefer_dmabuf: cfg.renderer.prefer_dmabuf,
        allow_shm_fallback: cfg.renderer.allow_shm_fallback,
        interactive: cfg.general.interactive,
        fps_limit: cfg.renderer.max_fps.unwrap_or(cfg.renderer.fps).clamp(1, 360),
        msaa_samples: cfg.renderer.msaa_samples.max(1),
        show_fps: cfg.general.show_fps,
        scale_mode: cfg.general.scale_mode,
        force_scene_audio_loop: cfg.general.force_scene_audio_loop,
        options_json: cfg.renderer.options_json,
        hooks: cfg.hooks,
        wallpapers: cfg.wallpapers,
        playlists: cfg.playlists,
        outputs: cfg.outputs,
        profiles: cfg.profiles,
        integrations: cfg.integrations,
        rules: cfg.rules,
    })
}

pub fn merge_scene_source_options(
    raw_options: Option<&str>,
    user_properties: Option<BTreeMap<String, serde_json::Value>>,
    force_scene_audio_loop: bool,
) -> Result<String> {
    let mut options = match raw_options {
        Some(raw) => serde_json::from_str::<serde_json::Value>(raw)
            .context("renderer.options_json must be valid JSON")?,
        None => serde_json::json!({}),
    };
    let Some(root) = options.as_object_mut() else {
        bail!("renderer.options_json must be a JSON object");
    };
    match root.get("version") {
        Some(version) if version.as_u64() == Some(1) => {}
        Some(version) => bail!("unsupported renderer.options_json version: {version}"),
        None => {
            root.insert("version".to_string(), serde_json::json!(1));
        }
    }

    let scene = root.entry("scene").or_insert_with(|| serde_json::json!({}));
    if !scene.is_object() {
        bail!("renderer.options_json scene field must be an object");
    }
    let scene = scene.as_object_mut().expect("scene options are an object");
    if let Some(user_properties) = user_properties {
        scene.insert("userProperties".to_string(), serde_json::json!(user_properties));
    }

    let audio = scene.entry("audio").or_insert_with(|| serde_json::json!({}));
    if !audio.is_object() {
        bail!("renderer.options_json scene.audio field must be an object");
    }
    audio
        .as_object_mut()
        .expect("audio options are an object")
        .insert("forceLoop".to_string(), serde_json::json!(force_scene_audio_loop));

    Ok(options.to_string())
}

pub fn save_force_scene_audio_loop(path: &Path, enabled: bool) -> Result<()> {
    let mut document = load_config_document(path)?;
    let Some(root) = document.as_table_mut() else {
        bail!("config root in {} must be a TOML table", path.display());
    };
    let general = root.entry("general").or_insert_with(|| toml::Value::Table(Default::default()));
    let Some(general) = general.as_table_mut() else {
        bail!("general config in {} must be a TOML table", path.display());
    };
    general.insert("force_scene_audio_loop".to_string(), toml::Value::Boolean(enabled));

    save_config_document(path, &document)
}

pub fn save_integrations_and_rules(
    path: &Path,
    integrations: &IntegrationsConfig,
    rules: &RuntimeRulesConfig,
) -> Result<()> {
    let mut document = load_config_document(path)?;
    let Some(root) = document.as_table_mut() else {
        bail!("config root in {} must be a TOML table", path.display());
    };
    root.insert(
        "integrations".to_string(),
        toml::Value::try_from(integrations).context("failed to serialize host integrations")?,
    );
    root.insert(
        "rules".to_string(),
        toml::Value::try_from(rules).context("failed to serialize runtime rules")?,
    );
    save_config_document(path, &document)
}

pub fn save_playlists(path: &Path, playlists: &PlaylistConfig) -> Result<()> {
    let mut document = load_config_document(path)?;
    let Some(root) = document.as_table_mut() else {
        bail!("config root in {} must be a TOML table", path.display());
    };
    let playlists = toml::Value::try_from(playlists).context("failed to serialize playlists")?;
    root.insert("playlists".to_string(), playlists);

    save_config_document(path, &document)
}

pub fn save_outputs(path: &Path, outputs: &BTreeMap<String, OutputBinding>) -> Result<()> {
    let mut document = load_config_document(path)?;
    let Some(root) = document.as_table_mut() else {
        bail!("config root in {} must be a TOML table", path.display());
    };
    let outputs = toml::Value::try_from(outputs).context("failed to serialize output bindings")?;
    root.insert("outputs".to_string(), outputs);

    save_config_document(path, &document)
}

pub fn save_profiles_and_outputs(
    path: &Path,
    profiles: &ProfileConfig,
    outputs: &BTreeMap<String, OutputBinding>,
) -> Result<()> {
    let mut document = load_config_document(path)?;
    let Some(root) = document.as_table_mut() else {
        bail!("config root in {} must be a TOML table", path.display());
    };
    root.insert(
        "profiles".to_string(),
        toml::Value::try_from(profiles).context("failed to serialize output profiles")?,
    );
    root.insert(
        "outputs".to_string(),
        toml::Value::try_from(outputs).context("failed to serialize output bindings")?,
    );
    save_config_document(path, &document)
}

pub fn save_playlists_profiles_and_outputs(
    path: &Path,
    playlists: &PlaylistConfig,
    profiles: &ProfileConfig,
    outputs: &BTreeMap<String, OutputBinding>,
) -> Result<()> {
    let mut document = load_config_document(path)?;
    let Some(root) = document.as_table_mut() else {
        bail!("config root in {} must be a TOML table", path.display());
    };
    root.insert(
        "playlists".to_string(),
        toml::Value::try_from(playlists).context("failed to serialize playlists")?,
    );
    root.insert(
        "profiles".to_string(),
        toml::Value::try_from(profiles).context("failed to serialize output profiles")?,
    );
    root.insert(
        "outputs".to_string(),
        toml::Value::try_from(outputs).context("failed to serialize output bindings")?,
    );
    save_config_document(path, &document)
}

pub fn save_wallpapers_playlists_profiles_and_outputs(
    path: &Path,
    wallpapers: &BTreeMap<String, WallpaperSettings>,
    playlists: &PlaylistConfig,
    profiles: &ProfileConfig,
    outputs: &BTreeMap<String, OutputBinding>,
) -> Result<()> {
    let mut document = load_config_document(path)?;
    let Some(root) = document.as_table_mut() else {
        bail!("config root in {} must be a TOML table", path.display());
    };
    root.insert(
        "wallpapers".to_string(),
        toml::Value::try_from(wallpapers).context("failed to serialize wallpaper profiles")?,
    );
    root.insert(
        "playlists".to_string(),
        toml::Value::try_from(playlists).context("failed to serialize playlists")?,
    );
    root.insert(
        "profiles".to_string(),
        toml::Value::try_from(profiles).context("failed to serialize output profiles")?,
    );
    root.insert(
        "outputs".to_string(),
        toml::Value::try_from(outputs).context("failed to serialize output bindings")?,
    );
    save_config_document(path, &document)
}

pub fn save_wallpapers(
    path: &Path,
    wallpapers: &BTreeMap<String, WallpaperSettings>,
) -> Result<()> {
    let mut document = load_config_document(path)?;
    let Some(root) = document.as_table_mut() else {
        bail!("config root in {} must be a TOML table", path.display());
    };
    root.insert(
        "wallpapers".to_string(),
        toml::Value::try_from(wallpapers).context("failed to serialize wallpaper profiles")?,
    );
    save_config_document(path, &document)
}

pub fn save_wallpapers_and_outputs(
    path: &Path,
    wallpapers: &BTreeMap<String, WallpaperSettings>,
    outputs: &BTreeMap<String, OutputBinding>,
) -> Result<()> {
    let mut document = load_config_document(path)?;
    let Some(root) = document.as_table_mut() else {
        bail!("config root in {} must be a TOML table", path.display());
    };
    root.insert(
        "wallpapers".to_string(),
        toml::Value::try_from(wallpapers).context("failed to serialize wallpaper profiles")?,
    );
    root.insert(
        "outputs".to_string(),
        toml::Value::try_from(outputs).context("failed to serialize output bindings")?,
    );
    save_config_document(path, &document)
}

pub fn save_wallpapers_playlists_and_outputs(
    path: &Path,
    wallpapers: &BTreeMap<String, WallpaperSettings>,
    playlists: &PlaylistConfig,
    outputs: &BTreeMap<String, OutputBinding>,
) -> Result<()> {
    let mut document = load_config_document(path)?;
    let Some(root) = document.as_table_mut() else {
        bail!("config root in {} must be a TOML table", path.display());
    };
    root.insert(
        "wallpapers".to_string(),
        toml::Value::try_from(wallpapers).context("failed to serialize wallpaper profiles")?,
    );
    root.insert(
        "playlists".to_string(),
        toml::Value::try_from(playlists).context("failed to serialize playlists")?,
    );
    root.insert(
        "outputs".to_string(),
        toml::Value::try_from(outputs).context("failed to serialize output bindings")?,
    );
    save_config_document(path, &document)
}

pub fn save_playlists_and_outputs(
    path: &Path,
    playlists: &PlaylistConfig,
    outputs: &BTreeMap<String, OutputBinding>,
) -> Result<()> {
    let mut document = load_config_document(path)?;
    let Some(root) = document.as_table_mut() else {
        bail!("config root in {} must be a TOML table", path.display());
    };
    root.insert(
        "playlists".to_string(),
        toml::Value::try_from(playlists).context("failed to serialize playlists")?,
    );
    root.insert(
        "outputs".to_string(),
        toml::Value::try_from(outputs).context("failed to serialize output bindings")?,
    );
    save_config_document(path, &document)
}

fn load_config_document(path: &Path) -> Result<toml::Value> {
    match fs::read_to_string(path) {
        Ok(raw) => toml::from_str::<toml::Value>(&raw)
            .with_context(|| format!("invalid TOML in {}", path.display())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(toml::Value::Table(Default::default()))
        }
        Err(error) => Err(error).with_context(|| format!("failed to read {}", path.display())),
    }
}

fn save_config_document(path: &Path, document: &toml::Value) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let serialized = toml::to_string_pretty(&document).context("failed to serialize config")?;
    let file_name = path.file_name().and_then(|name| name.to_str()).unwrap_or("config.toml");
    let sequence = CONFIG_TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let temporary =
        path.with_file_name(format!(".{file_name}.{}.{}.tmp", std::process::id(), sequence));
    let mut temporary_file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .with_context(|| format!("failed to create {}", temporary.display()))?;
    if let Err(error) =
        temporary_file.write_all(serialized.as_bytes()).and_then(|_| temporary_file.sync_all())
    {
        drop(temporary_file);
        let _ = fs::remove_file(&temporary);
        return Err(error).with_context(|| format!("failed to write {}", temporary.display()));
    }
    drop(temporary_file);
    if let Err(error) = fs::rename(&temporary, path) {
        let _ = fs::remove_file(&temporary);
        return Err(error).with_context(|| format!("failed to replace {}", path.display()));
    }
    Ok(())
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

    use super::{
        build_config, build_config_for_wallpaper, load_launch_settings, merge_scene_source_options,
        save_force_scene_audio_loop, save_integrations_and_rules, save_outputs, save_playlists,
        save_profiles_and_outputs, save_wallpapers_playlists_and_outputs, HookCommand, HooksConfig,
        IntegrationsConfig, LaunchSettings, OutputBinding, RuntimeRuleAction, RuntimeRulesConfig,
        ScaleMode,
    };
    use crate::playlist::{Playlist, PlaylistConfig, PlaylistItem, PlaylistMode};
    use crate::profile::{OutputProfile, ProfileConfig};
    use crate::wallpaper::settings::{
        RenderResolution, Rotation, WallpaperFillMode, WallpaperSettings,
    };

    fn unique_temp_path(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time before unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("we-layerd-{name}-{}-{nanos}", std::process::id()))
    }

    #[test]
    fn build_config_writes_renderer_native_source_and_assets() {
        let hooks = HooksConfig {
            wallpaper_applied: Some(HookCommand {
                command: "theme-sync".to_string(),
                args: vec!["--force".to_string()],
            }),
        };
        let settings = LaunchSettings {
            assets_path: "/steam/steamapps/common/wallpaper_engine".to_string(),
            fps_limit: 144,
            interactive: false,
            scale_mode: ScaleMode::Fit,
            options_json: Some("{\"demo\":true}".to_string()),
            hooks: hooks.clone(),
            ..LaunchSettings::default()
        };

        let cfg = build_config(&settings, Path::new("/tmp/item/project.json"));

        assert_eq!(cfg.renderer.source, "/tmp/item");
        assert_eq!(cfg.renderer.assets_path, "/steam/steamapps/common/wallpaper_engine/assets");
        assert_eq!(cfg.renderer.fps, 144);
        assert_eq!(cfg.renderer.options_json.as_deref(), Some("{\"demo\":true}"));
        assert!(!cfg.general.interactive);
        assert_eq!(cfg.general.scale_mode, ScaleMode::Fit);
        assert_eq!(cfg.hooks, hooks);
    }

    #[test]
    fn build_config_for_wallpaper_uses_only_the_selected_wallpaper_profile() {
        let mut settings = LaunchSettings { fps_limit: 144, ..LaunchSettings::default() };
        let mut user_properties = std::collections::BTreeMap::new();
        user_properties.insert("language".to_string(), serde_json::json!("3"));
        settings.wallpapers.insert(
            "alpha".to_string(),
            WallpaperSettings {
                fps: 144,
                speed: 1.5,
                volume: 0.4,
                muted: true,
                msaa_samples: 8,
                render_resolution: RenderResolution::Fixed { width: 2560, height: 1440 },
                fill_mode: WallpaperFillMode::Fit,
                rotation_degrees: Rotation::Deg90,
                user_properties,
            },
        );

        let cfg =
            build_config_for_wallpaper(&settings, "alpha", Path::new("/tmp/alpha/project.json"))
                .expect("wallpaper config");
        assert_eq!(cfg.renderer.fps, 144);
        assert_eq!(cfg.renderer.speed, 1.5);
        assert_eq!(cfg.renderer.volume, 0.4);
        assert!(cfg.renderer.muted);
        assert_eq!(cfg.renderer.msaa_samples, 8);
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
                "scene": {
                    "audio": { "forceLoop": false },
                    "userProperties": { "language": "3" }
                }
            })
        );
    }

    #[test]
    fn build_config_for_wallpaper_clamps_profile_fps_to_global_limit() {
        let mut settings = LaunchSettings { fps_limit: 30, ..LaunchSettings::default() };
        settings.wallpapers.insert(
            "alpha".to_string(),
            WallpaperSettings { fps: 144, ..WallpaperSettings::default() },
        );

        let cfg =
            build_config_for_wallpaper(&settings, "alpha", Path::new("/tmp/alpha/project.json"))
                .expect("wallpaper config");

        assert_eq!(cfg.renderer.fps, 30);
        assert_eq!(cfg.renderer.max_fps, Some(30));
    }

    #[test]
    fn build_config_for_wallpaper_merges_scene_options_and_audio_override() {
        let mut settings = LaunchSettings {
            force_scene_audio_loop: true,
            options_json: Some(
                serde_json::json!({
                    "version": 1,
                    "futureOption": { "keep": true },
                    "scene": {
                        "audio": { "futureAudioOption": 7 },
                        "graphviz": { "enabled": true },
                        "userProperties": { "stale": true }
                    }
                })
                .to_string(),
            ),
            ..LaunchSettings::default()
        };
        settings.wallpapers.insert("alpha".to_string(), WallpaperSettings::default());

        let cfg =
            build_config_for_wallpaper(&settings, "alpha", Path::new("/tmp/alpha/project.json"))
                .expect("wallpaper config");
        let options = serde_json::from_str::<serde_json::Value>(
            cfg.renderer.options_json.as_deref().expect("source options"),
        )
        .expect("valid source options");

        assert_eq!(options["futureOption"]["keep"], true);
        assert_eq!(options["scene"]["graphviz"]["enabled"], true);
        assert_eq!(options["scene"]["audio"]["futureAudioOption"], 7);
        assert_eq!(options["scene"]["audio"]["forceLoop"], true);
        assert_eq!(options["scene"]["userProperties"], serde_json::json!({}));
    }

    #[test]
    fn scene_options_reject_invalid_containers_and_future_versions_without_overwriting() {
        for raw in [
            "not-json",
            "[]",
            r#"{"version":2,"future":true}"#,
            r#"{"version":1,"scene":true}"#,
            r#"{"version":1,"scene":{"audio":true}}"#,
        ] {
            assert!(merge_scene_source_options(Some(raw), None, true).is_err());
        }
    }

    #[test]
    fn force_loop_preference_patch_preserves_unknown_config_sections() {
        let path = unique_temp_path("audio-preference");
        fs::write(&path, "[general]\ninteractive = true\n\n[gnome]\ncustom = \"keep\"\n")
            .expect("write config");

        save_force_scene_audio_loop(&path, true).expect("save audio preference");
        let document = fs::read_to_string(&path).expect("read config");
        let value = toml::from_str::<toml::Value>(&document).expect("valid TOML");
        assert_eq!(value["general"]["force_scene_audio_loop"].as_bool(), Some(true));
        assert_eq!(value["gnome"]["custom"].as_str(), Some("keep"));

        let _ = fs::remove_file(path);
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
force_scene_audio_loop = true

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
msaa_samples = 4
options_json = "{\"keep\":true}"
"#;

        fs::write(&path, toml).expect("failed to write temp config");

        let settings = load_launch_settings(&path).expect("renderer config should load");
        assert_eq!(settings.fps_limit, 120);
        assert!(settings.show_fps);
        assert!(!settings.interactive);
        assert!(settings.force_scene_audio_loop);
        assert_eq!(settings.msaa_samples, 4);
        assert_eq!(settings.scale_mode, ScaleMode::Stretch);
        assert_eq!(settings.renderer_library_path, "/opt/libwallpaper-engine-renderer.so");
        assert_eq!(settings.renderer_cache_path, "~/.cache/we-layerd/custom");
        assert!(!settings.prefer_dmabuf);
        assert!(settings.allow_shm_fallback);
        assert_eq!(settings.options_json.as_deref(), Some("{\"keep\":true}"));
        assert_eq!(settings.workshop_path, "/tmp/workshop/content/431960");
        assert_eq!(settings.assets_path, "/opt/wallpaper_engine");

        let _ = fs::remove_file(path);
    }

    #[test]
    fn launch_settings_default_to_auto_renderer_resolution() {
        let settings = LaunchSettings::default();
        assert!(settings.renderer_library_path.is_empty());
        assert!(settings.integrations.media);
        assert!(!settings.integrations.audio_spectrum);
        assert_eq!(settings.integrations.audio_source, "@DEFAULT_MONITOR@");
        assert_eq!(settings.rules.focused, RuntimeRuleAction::Keep);
        assert_eq!(settings.rules.maximized, RuntimeRuleAction::Keep);
        assert_eq!(settings.rules.fullscreen, RuntimeRuleAction::Keep);
    }

    #[test]
    fn integration_and_runtime_rules_round_trip_through_launch_settings() {
        let path = unique_temp_path("integrations-rules.toml");
        fs::write(
            &path,
            r#"
[renderer]
source = "/tmp/workshop/content/431960/42"
assets_path = "/tmp/wallpaper_engine/assets"

[integrations]
media = false
audio_spectrum = true
audio_source = "custom.monitor"
audio_sample_rate = 44100
audio_update_hz = 20

[rules]
focused = "mute"
maximized = "pause"
fullscreen = "pause"
"#,
        )
        .expect("write integration config");

        let settings = load_launch_settings(&path).expect("integration config should load");
        assert!(!settings.integrations.media);
        assert!(settings.integrations.audio_spectrum);
        assert_eq!(settings.integrations.audio_source, "custom.monitor");
        assert_eq!(settings.integrations.audio_sample_rate, 44_100);
        assert_eq!(settings.integrations.audio_update_hz, 20);
        assert_eq!(settings.rules.focused, RuntimeRuleAction::Mute);
        assert_eq!(settings.rules.maximized, RuntimeRuleAction::Pause);
        assert_eq!(settings.rules.fullscreen, RuntimeRuleAction::Pause);

        let rebuilt =
            build_config(&settings, Path::new("/tmp/workshop/content/431960/42/project.json"));
        assert_eq!(rebuilt.integrations, settings.integrations);
        assert_eq!(rebuilt.rules, settings.rules);

        let _ = fs::remove_file(path);
    }

    #[test]
    fn integration_patch_preserves_renderer_and_unknown_sections() {
        let path = unique_temp_path("integration-patch.toml");
        fs::write(
            &path,
            "[renderer]\nsource = \"/keep/source\"\nassets_path = \"/assets\"\n\n[custom]\nkeep = 7\n",
        )
        .expect("write config");
        let integrations = IntegrationsConfig {
            media: false,
            audio_spectrum: true,
            audio_source: "monitor.test".to_string(),
            audio_sample_rate: 48_000,
            audio_update_hz: 24,
        };
        let rules = RuntimeRulesConfig {
            focused: RuntimeRuleAction::Mute,
            maximized: RuntimeRuleAction::Keep,
            fullscreen: RuntimeRuleAction::Pause,
        };

        save_integrations_and_rules(&path, &integrations, &rules)
            .expect("save integrations and rules");

        let document = fs::read_to_string(&path).expect("read config");
        let value = toml::from_str::<toml::Value>(&document).expect("valid TOML");
        assert_eq!(value["renderer"]["source"].as_str(), Some("/keep/source"));
        assert_eq!(value["custom"]["keep"].as_integer(), Some(7));
        let loaded = load_launch_settings(&path).expect("reload settings");
        assert_eq!(loaded.integrations, integrations);
        assert_eq!(loaded.rules, rules);

        let _ = fs::remove_file(path);
    }

    #[test]
    fn playlist_patch_round_trips_and_preserves_unrelated_config() {
        let path = unique_temp_path("playlist-config.toml");
        fs::write(
            &path,
            "[general]\ninteractive = true\n\n[renderer]\nsource = \"/old/source\"\nassets_path = \"/assets\"\n\n[gnome]\ncustom = \"keep\"\n",
        )
        .expect("write config");

        let mut playlists =
            PlaylistConfig { active: Some("Focus".to_string()), ..PlaylistConfig::default() };
        playlists.definitions.insert(
            "Focus".to_string(),
            Playlist {
                mode: PlaylistMode::Shuffle,
                default_duration_ms: 45_000,
                items: vec![PlaylistItem {
                    wallpaper_id: "42".to_string(),
                    source: "/workshop/42".to_string(),
                    duration_ms: Some(12_000),
                }],
            },
        );

        save_playlists(&path, &playlists).expect("save playlists");

        let document = fs::read_to_string(&path).expect("read config");
        let value = toml::from_str::<toml::Value>(&document).expect("valid TOML");
        assert_eq!(value["renderer"]["source"].as_str(), Some("/old/source"));
        assert_eq!(value["gnome"]["custom"].as_str(), Some("keep"));

        let loaded = load_launch_settings(&path).expect("reload launch settings");
        assert_eq!(loaded.playlists, playlists);

        let _ = fs::remove_file(path);
    }

    #[test]
    fn output_binding_patch_round_trips_without_replacing_renderer_or_playlists() {
        let path = unique_temp_path("output-bindings.toml");
        fs::write(
            &path,
            "[renderer]\nsource = \"/keep/source\"\nassets_path = \"/assets\"\n\n[playlists]\nactive = \"Focus\"\n",
        )
        .expect("write config");
        let outputs = std::collections::BTreeMap::from([
            ("DP-1".to_string(), OutputBinding::wallpaper("42", "/workshop/42")),
            ("HDMI-A-1".to_string(), OutputBinding::playlist("Focus")),
        ]);

        save_outputs(&path, &outputs).expect("save output bindings");

        let document = fs::read_to_string(&path).expect("read config");
        let value = toml::from_str::<toml::Value>(&document).expect("valid TOML");
        assert_eq!(value["renderer"]["source"].as_str(), Some("/keep/source"));
        assert_eq!(value["playlists"]["active"].as_str(), Some("Focus"));
        let loaded = load_launch_settings(&path).expect("reload launch settings");
        assert_eq!(loaded.outputs, outputs);

        let _ = fs::remove_file(path);
    }

    #[test]
    fn wallpaper_and_output_patch_preserves_global_renderer_fallback() {
        let path = unique_temp_path("wallpaper-output-bindings.toml");
        fs::write(&path, "[renderer]\nsource = \"/keep/fallback\"\nassets_path = \"/assets\"\n")
            .expect("write config");
        let wallpapers = std::collections::BTreeMap::from([(
            "42".to_string(),
            WallpaperSettings { fps: 144, ..WallpaperSettings::default() },
        )]);
        let outputs = std::collections::BTreeMap::from([(
            "DP-1".to_string(),
            OutputBinding::wallpaper("42", "/workshop/42"),
        )]);

        let playlists = PlaylistConfig::default();
        save_wallpapers_playlists_and_outputs(&path, &wallpapers, &playlists, &outputs)
            .expect("save wallpaper profiles, playlists, and output bindings");

        let document = fs::read_to_string(&path).expect("read config");
        let value = toml::from_str::<toml::Value>(&document).expect("valid TOML");
        assert_eq!(value["renderer"]["source"].as_str(), Some("/keep/fallback"));
        let loaded = load_launch_settings(&path).expect("reload launch settings");
        assert_eq!(loaded.wallpapers, wallpapers);
        assert_eq!(loaded.playlists.active, None);
        assert_eq!(loaded.outputs, outputs);

        let _ = fs::remove_file(path);
    }
    #[test]
    fn profile_and_output_patch_survives_reload_and_preserves_renderer_fallback() {
        let path = unique_temp_path("profiles.toml");
        fs::write(&path, "[renderer]\nsource = \"/keep/fallback\"\nassets_path = \"/assets\"\n")
            .expect("write config");
        let outputs = std::collections::BTreeMap::from([(
            "DP-1".to_string(),
            OutputBinding::wallpaper("42", "/workshop/42"),
        )]);
        let mut profiles =
            ProfileConfig { active: Some("Desk".to_string()), ..ProfileConfig::default() };
        profiles.definitions.insert("Desk".to_string(), OutputProfile { outputs: outputs.clone() });

        save_profiles_and_outputs(&path, &profiles, &outputs).expect("save profiles and outputs");

        let loaded = load_launch_settings(&path).expect("reload launch settings");
        assert_eq!(loaded.profiles, profiles);
        assert_eq!(loaded.outputs, outputs);
        let document = fs::read_to_string(&path).expect("read config");
        let value = toml::from_str::<toml::Value>(&document).expect("valid TOML");
        assert_eq!(value["renderer"]["source"].as_str(), Some("/keep/fallback"));

        let _ = fs::remove_file(path);
    }
}
