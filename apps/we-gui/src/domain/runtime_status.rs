#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RuntimeStatus {
    DaemonNotRunning,
    DaemonNotFound,
    SwitchedDaemon,
    StartedDaemon,
    StartFailed(String),
    StoppedDaemon,
    StopFailed,
    Unavailable(String),
    EmptyResponse,
    Raw(String),
    InvalidWallpaperEngineDirectory,
    ConfigSaveFailed(String),
    PreferencesSaveFailed(String),
    PlaylistError(String),
}
