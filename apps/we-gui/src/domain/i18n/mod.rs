mod en;
mod zh_hans;

use std::fmt;

use super::runtime_status::RuntimeStatus;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum Language {
    #[default]
    English,
    SimplifiedChinese,
}

impl Language {
    pub(crate) const ALL: [Self; 2] = [Self::English, Self::SimplifiedChinese];

    pub(crate) fn tag(self) -> &'static str {
        match self {
            Self::English => "en",
            Self::SimplifiedChinese => "zh-Hans",
        }
    }

    pub(crate) fn from_tag(tag: &str) -> Option<Self> {
        match tag {
            "en" => Some(Self::English),
            "zh-Hans" => Some(Self::SimplifiedChinese),
            _ => None,
        }
    }

    pub(crate) fn text(self, key: Text) -> &'static str {
        match self {
            Self::English => en::text(key),
            Self::SimplifiedChinese => zh_hans::text(key),
        }
    }

    pub(crate) fn item_count(self, count: usize) -> String {
        match self {
            Self::English if count == 1 => "1 item".to_string(),
            Self::English => format!("{count} items"),
            Self::SimplifiedChinese => format!("{count} 项"),
        }
    }

    pub(crate) fn speed(self, value: f32) -> String {
        match self {
            Self::English => format!("Speed  {value:.2}×"),
            Self::SimplifiedChinese => format!("速度  {value:.2}×"),
        }
    }

    pub(crate) fn volume(self, value: f32) -> String {
        match self {
            Self::English => format!("Volume  {value:.0}%"),
            Self::SimplifiedChinese => format!("音量  {value:.0}%"),
        }
    }

    pub(crate) fn runtime_status(self, status: &RuntimeStatus) -> String {
        match (self, status) {
            (Self::English, RuntimeStatus::DaemonNotRunning) => {
                "status unavailable: daemon is not running".to_string()
            }
            (Self::SimplifiedChinese, RuntimeStatus::DaemonNotRunning) => {
                "状态不可用：守护进程未运行".to_string()
            }
            (Self::English, RuntimeStatus::DaemonNotFound) => {
                "we-layerd not found in PATH".to_string()
            }
            (Self::SimplifiedChinese, RuntimeStatus::DaemonNotFound) => {
                "在 PATH 中找不到 we-layerd".to_string()
            }
            (Self::English, RuntimeStatus::SwitchedDaemon) => {
                "switched running daemon".to_string()
            }
            (Self::SimplifiedChinese, RuntimeStatus::SwitchedDaemon) => {
                "已切换正在运行的守护进程".to_string()
            }
            (Self::English, RuntimeStatus::StartedDaemon) => "started daemon".to_string(),
            (Self::SimplifiedChinese, RuntimeStatus::StartedDaemon) => {
                "已启动守护进程".to_string()
            }
            (Self::English, RuntimeStatus::StartFailed(detail)) => {
                format!("failed to start daemon: {detail}")
            }
            (Self::SimplifiedChinese, RuntimeStatus::StartFailed(detail)) => {
                format!("启动守护进程失败：{detail}")
            }
            (Self::English, RuntimeStatus::StoppedDaemon) => "stopped daemon".to_string(),
            (Self::SimplifiedChinese, RuntimeStatus::StoppedDaemon) => {
                "已停止守护进程".to_string()
            }
            (Self::English, RuntimeStatus::StopFailed) => {
                "daemon stop request failed".to_string()
            }
            (Self::SimplifiedChinese, RuntimeStatus::StopFailed) => {
                "停止守护进程的请求失败".to_string()
            }
            (Self::English, RuntimeStatus::Unavailable(detail)) => {
                format!("status unavailable: {detail}")
            }
            (Self::SimplifiedChinese, RuntimeStatus::Unavailable(detail)) => {
                format!("状态不可用：{detail}")
            }
            (Self::English, RuntimeStatus::EmptyResponse) => {
                "status unavailable: daemon returned an empty response".to_string()
            }
            (Self::SimplifiedChinese, RuntimeStatus::EmptyResponse) => {
                "状态不可用：守护进程返回了空响应".to_string()
            }
            (_, RuntimeStatus::Raw(text)) => text.clone(),
            (Self::English, RuntimeStatus::InvalidWallpaperEngineDirectory) => {
                "selected directory is not a Wallpaper Engine installation".to_string()
            }
            (Self::SimplifiedChinese, RuntimeStatus::InvalidWallpaperEngineDirectory) => {
                "所选目录不是 Wallpaper Engine 安装目录".to_string()
            }
            (Self::English, RuntimeStatus::ConfigSaveFailed(detail)) => {
                format!("failed to save config: {detail}")
            }
            (Self::SimplifiedChinese, RuntimeStatus::ConfigSaveFailed(detail)) => {
                format!("保存配置失败：{detail}")
            }
            (Self::English, RuntimeStatus::PreferencesSaveFailed(detail)) => {
                format!("failed to save GUI preferences: {detail}")
            }
            (Self::SimplifiedChinese, RuntimeStatus::PreferencesSaveFailed(detail)) => {
                format!("保存界面偏好设置失败：{detail}")
            }
        }
    }
}

impl fmt::Display for Language {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::English => "English",
            Self::SimplifiedChinese => "简体中文",
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Text {
    Wallpapers,
    SearchWallpapers,
    NoMatchingWallpapers,
    OpenSettings,
    FilterAll,
    FilterWeb,
    FilterScene,
    FilterVideo,
    TypeVideo,
    TypeScene,
    TypeWeb,
    TypeUnknown,
    Settings,
    SettingsSubtitle,
    CloseSettings,
    Language,
    WallpaperEngine,
    AssetsPath,
    WorkshopPath,
    Renderer,
    RendererLibrary,
    AutomaticSearch,
    RendererCachePath,
    Presentation,
    FrameRateLimit,
    ScaleMode,
    ScaleFit,
    ScaleCover,
    ScaleStretch,
    Behaviour,
    EnableWallpaperInput,
    ForceSceneAudioLoop,
    ForceSceneAudioLoopDescription,
    ShowRealtimeFps,
    PreferDmabuf,
    AllowShmFallback,
    RuntimeStatus,
    Browse,
    Actions,
    UserProperties,
    Playback,
    FrameRate,
    MuteWallpaperAudio,
    Display,
    ApplyToDisplays,
    NoWaylandDisplaysDetected,
    RenderResolution,
    FollowOutput,
    FixedResolution,
    Width,
    Height,
    Scaling,
    Rotation,
    FillCover,
    FillFit,
    FillStretch,
    FillCenter,
    ApplyAndPlay,
    Play,
    Pause,
    Stop,
    ResetProperties,
    PropertiesSavedAutomatically,
    NoUserProperties,
    Enabled,
    Value,
    Path,
    UnsupportedProperty,
    SelectWallpaperDetails,
    SelectAssetsDirectory,
    SelectWorkshopDirectory,
    SelectPropertyPath,
    TrayShowWindow,
    TrayPlaySwitch,
    TrayStop,
    TrayPause,
    TrayResume,
    TrayQuit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Localized<T> {
    pub(crate) value: T,
    pub(crate) label: &'static str,
}

impl<T> Localized<T> {
    pub(crate) fn new(value: T, label: &'static str) -> Self {
        Self { value, label }
    }
}

impl<T> fmt::Display for Localized<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.label)
    }
}

#[cfg(test)]
mod tests {
    use super::{Language, Text};
    use crate::domain::runtime_status::RuntimeStatus;

    #[test]
    fn language_tags_use_bcp47_identifiers() {
        assert_eq!(Language::English.tag(), "en");
        assert_eq!(Language::SimplifiedChinese.tag(), "zh-Hans");
        assert_eq!(Language::from_tag("zh-Hans"), Some(Language::SimplifiedChinese));
        assert_eq!(Language::from_tag("zh_CN"), None);
    }

    #[test]
    fn runtime_status_is_retranslated_from_structured_state() {
        let status = RuntimeStatus::StartFailed("permission denied".to_string());
        assert_eq!(
            Language::English.runtime_status(&status),
            "failed to start daemon: permission denied"
        );
        assert_eq!(
            Language::SimplifiedChinese.runtime_status(&status),
            "启动守护进程失败：permission denied"
        );

        let status = RuntimeStatus::ConfigSaveFailed("disk full".to_string());
        assert_eq!(
            Language::English.runtime_status(&status),
            "failed to save config: disk full"
        );
        assert_eq!(
            Language::SimplifiedChinese.runtime_status(&status),
            "保存配置失败：disk full"
        );

        let status = RuntimeStatus::InvalidWallpaperEngineDirectory;
        assert_eq!(
            Language::English.runtime_status(&status),
            "selected directory is not a Wallpaper Engine installation"
        );
        assert_eq!(
            Language::SimplifiedChinese.runtime_status(&status),
            "所选目录不是 Wallpaper Engine 安装目录"
        );
    }

    #[test]
    fn primary_ui_copy_exists_in_both_catalogs() {
        for language in Language::ALL {
            assert!(!language.text(Text::ApplyToDisplays).is_empty());
            assert!(!language.text(Text::NoWaylandDisplaysDetected).is_empty());
            assert!(!language.text(Text::RuntimeStatus).is_empty());
        }
    }
}
