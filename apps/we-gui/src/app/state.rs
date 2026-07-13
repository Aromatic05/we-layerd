use std::{collections::HashMap, path::PathBuf, process::Child};

use iced::{widget::pane_grid, window, Size, Theme};
use we_core::{config::LaunchSettings, wallpaper::{properties::UserPropertySchema, WallpaperEntry, WallpaperType}};

use crate::{domain::{settings::{ScaleModeOption, UiSettings}, ui_state::{AnimatedPreview, GifFrame, Pane, Sidebar}}, platform::tray, ui::sidebar::detail::DetailMessage};

pub(crate) struct App {
    pub entries: Vec<WallpaperEntry>, pub selected_id: Option<String>, pub selected_schema: UserPropertySchema,
    pub resolution_width: String, pub resolution_height: String, pub config_path: PathBuf, pub runtime_child: Option<Child>,
    pub viewport_width: f32, pub layerd_available: bool, pub launch_settings: LaunchSettings, pub ui_settings: UiSettings,
    pub show_settings: bool, pub sidebar: Option<Sidebar>, pub detail_tab: crate::ui::sidebar::detail::DetailTab,
    pub playback_paused: bool, pub playback_running: bool, pub search_query: String, pub type_filter: Option<WallpaperType>,
    pub panes: pane_grid::State<Pane>, pub animated_previews: HashMap<PathBuf, AnimatedPreview>, pub tray: Option<tray::TrayController>,
    pub main_window_id: Option<window::Id>, pub theme: Theme,
}

#[derive(Debug, Clone)]
pub(crate) enum Message {
    AutoScan, Scanned(Result<Vec<WallpaperEntry>, String>), GifLoaded(PathBuf, Result<Vec<GifFrame>, String>), GifTick,
    SelectWallpaper(usize), PlayPressed, StopPressed, SettingsPressed, SearchChanged(String), TypeFilterSelected(Option<WallpaperType>), PaneResized(pane_grid::ResizeEvent),
    AssetsPathChanged(String), WorkshopPathChanged(String), RendererLibraryPathChanged(String), RendererCachePathChanged(String), PickAssetsPath, PickWorkshopPath,
    AssetsPathPicked(Option<PathBuf>), WorkshopPathPicked(Option<PathBuf>), FpsLimitChanged(String), InteractiveToggled(bool), ShowFpsToggled(bool),
    ScaleModeSelected(ScaleModeOption), PreferDmabufToggled(bool), AllowShmFallbackToggled(bool), Detail(DetailMessage), StatusLoaded(Result<String, String>), StatusTick,
    WindowResized(Size), WindowCloseRequested(window::Id), WindowOpened(window::Id), WindowClosed(window::Id), TrayTick, ThemeTick, TrayAction(tray::TrayAction),
}
