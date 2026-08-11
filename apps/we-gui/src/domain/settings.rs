use we_core::config::{RuntimeRuleAction, ScaleMode};

#[derive(Debug, Clone)]
pub(crate) struct UiSettings {
    pub assets_path: String,
    pub workshop_path: String,
    pub renderer_library_path: String,
    pub renderer_cache_path: String,
    pub prefer_dmabuf: bool,
    pub allow_shm_fallback: bool,
    pub interactive: bool,
    pub force_scene_audio_loop: bool,
    pub fps_limit: String,
    pub show_fps: bool,
    pub scale_mode: ScaleModeOption,
    pub media_integration: bool,
    pub audio_spectrum: bool,
    pub audio_source: String,
    pub rule_focused: RuntimeRuleAction,
    pub rule_maximized: RuntimeRuleAction,
    pub rule_fullscreen: RuntimeRuleAction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ScaleModeOption {
    Fit,
    Cover,
    Stretch,
}

impl From<ScaleMode> for ScaleModeOption {
    fn from(value: ScaleMode) -> Self {
        match value {
            ScaleMode::Fit => Self::Fit,
            ScaleMode::Cover => Self::Cover,
            ScaleMode::Stretch => Self::Stretch,
        }
    }
}

impl From<ScaleModeOption> for ScaleMode {
    fn from(value: ScaleModeOption) -> Self {
        match value {
            ScaleModeOption::Fit => Self::Fit,
            ScaleModeOption::Cover => Self::Cover,
            ScaleModeOption::Stretch => Self::Stretch,
        }
    }
}
