use std::{collections::HashMap, path::PathBuf};

use iced::{widget::pane_grid, window, Task, Theme};
use we_core::{config::{load_launch_settings, LaunchSettings}, steam, wallpaper::properties::UserPropertySchema};

use crate::{domain::{settings::{ScaleModeOption, UiSettings}, ui_state::Pane}, platform::tray, ui::sidebar::detail::DetailTab};

use super::{App, Message};

pub(crate) fn initialize() -> (App, Task<Message>) {
    let config_path = steam::default_config_path().unwrap_or_else(|| PathBuf::from("config.toml"));
    let mut launch_settings = load_launch_settings(&config_path).unwrap_or_else(|_| LaunchSettings::default());
    if launch_settings.workshop_path.trim().is_empty() {
        launch_settings.workshop_path = steam::discover_workshop_wallpaper_root()
            .map(|path| path.display().to_string())
            .unwrap_or_default();
    }
    if launch_settings.assets_path.trim().is_empty() {
        launch_settings.assets_path = steam::discover_wallpaper_engine_path()
            .map(|path| path.display().to_string())
            .unwrap_or_default();
    }
    let ui_settings = UiSettings {
        assets_path: launch_settings.assets_path.clone(),
        workshop_path: launch_settings.workshop_path.clone(),
        renderer_library_path: launch_settings.renderer_library_path.clone(),
        renderer_cache_path: launch_settings.renderer_cache_path.clone(),
        prefer_dmabuf: launch_settings.prefer_dmabuf,
        allow_shm_fallback: launch_settings.allow_shm_fallback,
        interactive: launch_settings.interactive,
        force_scene_audio_loop: launch_settings.force_scene_audio_loop,
        fps_limit: launch_settings.fps_limit.to_string(),
        show_fps: launch_settings.show_fps,
        scale_mode: ScaleModeOption::from(launch_settings.scale_mode),
        status_text: "status unavailable: daemon is not running".to_string(),
    };

    (
        App {
            entries: Vec::new(),
            selected_id: None,
            selected_schema: UserPropertySchema { entries: Vec::new() },
            resolution_width: String::new(),
            resolution_height: String::new(),
            config_path,
            runtime_child: None,
            viewport_width: 1280.0,
            layerd_available: crate::services::runtime::command_exists_in_path("we-layerd"),
            launch_settings,
            ui_settings,
            show_settings: false,
            sidebar: None,
            detail_tab: DetailTab::Actions,
            playback_paused: false,
            playback_running: false,
            search_query: String::new(),
            type_filter: None,
            panes: pane_grid::State::with_configuration(pane_grid::Configuration::Split {
                axis: pane_grid::Axis::Vertical,
                ratio: 0.68,
                a: Box::new(pane_grid::Configuration::Pane(Pane::Library)),
                b: Box::new(pane_grid::Configuration::Pane(Pane::Sidebar)),
            }),
            animated_previews: HashMap::new(),
            tray: tray::TrayController::new().ok(),
            main_window_id: None,
            theme: detect_system_theme(),
            runtime_shutdown: false,
            outputs: Vec::new(), selected_outputs: Default::default(),
            running_source: None,
        },
        Task::batch(vec![
            Task::done(Message::AutoScan),
            Task::perform(crate::services::runtime::fetch_outputs(), Message::OutputsLoaded),
            window::open(window::Settings::default()).1.map(Message::WindowOpened),
        ]),
    )
}

pub(crate) fn detect_system_theme() -> Theme {
    match dark_light::detect() {
        dark_light::Mode::Light => Theme::Light,
        dark_light::Mode::Dark => Theme::Dark,
        dark_light::Mode::Default => Theme::Dark,
    }
}
