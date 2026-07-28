use std::{collections::{BTreeSet, HashMap}, path::PathBuf, process::Child, time::Duration};

use iced::{widget::pane_grid, window, Size, Theme};
use we_core::{config::LaunchSettings, wallpaper::{properties::UserPropertySchema, WallpaperEntry, WallpaperType}};

use crate::{
    domain::{
        i18n::Language,
        runtime_status::RuntimeStatus,
        settings::{ScaleModeOption, UiSettings},
        ui_state::{AnimatedPreview, GifFrame, Pane, Sidebar},
    },
    platform::tray,
    ui::sidebar::detail::DetailMessage,
};

pub(crate) struct App {
    pub entries: Vec<WallpaperEntry>, pub selected_id: Option<String>, pub selected_schema: UserPropertySchema,
    pub resolution_width: String, pub resolution_height: String, pub config_path: PathBuf, pub runtime_child: Option<Child>,
    pub viewport_width: f32, pub layerd_available: bool, pub launch_settings: LaunchSettings, pub ui_settings: UiSettings,
    pub show_settings: bool, pub sidebar: Option<Sidebar>, pub detail_tab: crate::ui::sidebar::detail::DetailTab,
    pub playback_paused: bool, pub playback_running: bool, pub search_query: String, pub type_filter: Option<WallpaperType>,
    pub panes: pane_grid::State<Pane>, pub animated_previews: HashMap<PathBuf, AnimatedPreview>, pub tray: Option<tray::TrayController>,
    pub main_window_id: Option<window::Id>, pub theme: Theme, pub runtime_shutdown: bool,
    pub outputs: Vec<String>, pub selected_outputs: BTreeSet<String>,
    pub running_source: Option<String>,
    pub language: Language, pub preferences_path: Option<PathBuf>, pub runtime_status: RuntimeStatus,
    pub preferences_generation: u64,
    pub shuffle_elapsed: Duration,
}

impl App {
    pub(crate) fn selected_wallpaper_is_running(&self) -> bool {
        let Some(selected_id) = self.selected_id.as_deref() else { return false };
        let Some(entry) = self.entries.iter().find(|entry| entry.id == selected_id) else { return false };
        let source = entry.project_json.parent().unwrap_or(&entry.project_json).to_string_lossy();
        self.playback_running && !self.playback_paused && self.running_source.as_deref() == Some(source.as_ref())
    }

    pub(crate) fn shutdown_runtime(&mut self) -> bool {
        if self.runtime_shutdown {
            return true;
        }
        self.runtime_shutdown = true;
        let stopped = crate::services::runtime::stop(&mut self.runtime_child);
        self.clear_playback_state();
        stopped
    }

    pub(crate) fn clear_playback_state(&mut self) {
        self.playback_running = false;
        self.playback_paused = false;
        self.running_source = None;
        self.shuffle_elapsed = Duration::ZERO;
    }
}

impl Drop for App {
    fn drop(&mut self) {
        let _ = self.shutdown_runtime();
    }
}

#[derive(Debug, Clone)]
pub(crate) enum Message {
    AutoScan, Scanned(Result<Vec<WallpaperEntry>, String>), GifLoaded(PathBuf, Result<Vec<GifFrame>, String>), GifTick,
    SelectWallpaper(usize), PlayPressed, StopPressed, SettingsPressed, SearchChanged(String), TypeFilterSelected(Option<WallpaperType>), PaneResized(pane_grid::ResizeEvent),
    AssetsPathChanged(String), WorkshopPathChanged(String), RendererLibraryPathChanged(String), RendererCachePathChanged(String), PickAssetsPath, PickWorkshopPath,
    AssetsPathPicked(Option<PathBuf>), WorkshopPathPicked(Option<PathBuf>), FpsLimitChanged(String), InteractiveToggled(bool), ForceSceneAudioLoopToggled(bool), ShowFpsToggled(bool),
    ScaleModeSelected(ScaleModeOption), PreferDmabufToggled(bool), AllowShmFallbackToggled(bool), LanguageSelected(Language), PreferencesSaved { generation: u64, result: Result<(), String> }, Detail(DetailMessage), StatusLoaded(Result<Option<String>, String>), StatusTick,
    ShufflePressed, ShufflePlaybackPressed, ShuffleEnabledToggled(bool), ShuffleIntervalSelected(u32), ShuffleIntervalChanged(String), ShuffleIncludeVideoToggled(bool), ShuffleIncludeSceneToggled(bool), ShuffleIncludeWebToggled(bool),
    WindowResized(Size), WindowCloseRequested(window::Id), WindowOpened(window::Id), WindowClosed(window::Id), TrayTick, ThemeTick, TrayAction(tray::TrayAction),
    OutputsLoaded(Result<Vec<String>, String>), ToggleOutput(String),
}
