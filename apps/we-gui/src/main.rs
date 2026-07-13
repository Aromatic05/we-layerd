use std::{
    env,
    path::{Path, PathBuf},
    process::{Child, Command},
    time::Duration,
};

use iced::{
    alignment::{Horizontal, Vertical},
    widget::{button, column, container, image, row, scrollable, stack, svg, text},
    window, Background, Border, Color, ContentFit, Element, Fill, Size, Subscription, Task, Theme,
};
use settings_panel::{build_settings_overlay, ScaleModeOption, UiSettings};
use we_core::{
    config::{build_config_for_wallpaper, load_launch_settings, save_config, LaunchSettings, ScaleMode},
    steam,
    wallpaper::{
        self,
        properties::UserPropertySchema,
        settings::{RenderResolution, WallpaperSettings},
        WallpaperEntry, WallpaperType,
    },
};

mod settings_panel;
mod tray;
mod wallpaper_detail;

fn main() -> iced::Result {
    iced::daemon(App::init, update, view)
        .title("we-gui")
        .theme(|app: &App, _window| app.theme.clone())
        .subscription(subscription)
        .run()
}

struct App {
    entries: Vec<WallpaperEntry>,
    selected_id: Option<String>,
    selected_schema: UserPropertySchema,
    resolution_width: String,
    resolution_height: String,
    config_path: PathBuf,
    runtime_child: Option<Child>,
    viewport_width: f32,
    layerd_available: bool,
    launch_settings: LaunchSettings,
    ui_settings: UiSettings,
    show_settings: bool,
    tray: Option<tray::TrayController>,
    main_window_id: Option<window::Id>,
    theme: Theme,
}

#[derive(Debug, Clone)]
enum Message {
    AutoScan,
    Scanned(Result<Vec<WallpaperEntry>, String>),
    SelectWallpaper(usize),
    PlayPressed,
    StopPressed,
    SettingsPressed,
    AssetsPathChanged(String),
    WorkshopPathChanged(String),
    RendererLibraryPathChanged(String),
    RendererCachePathChanged(String),
    PickAssetsPath,
    PickWorkshopPath,
    AssetsPathPicked(Option<PathBuf>),
    WorkshopPathPicked(Option<PathBuf>),
    FpsLimitChanged(String),
    InteractiveToggled(bool),
    ShowFpsToggled(bool),
    ScaleModeSelected(ScaleModeOption),
    PreferDmabufToggled(bool),
    AllowShmFallbackToggled(bool),
    Detail(wallpaper_detail::DetailMessage),
    StatusLoaded(Result<String, String>),
    StatusTick,
    WindowResized(Size),
    WindowCloseRequested(window::Id),
    WindowOpened(window::Id),
    WindowClosed(window::Id),
    TrayTick,
    ThemeTick,
    TrayAction(tray::TrayAction),
}

fn update(app: &mut App, message: Message) -> Task<Message> {
    match message {
        Message::AutoScan => Task::perform(
            scan_wallpapers_from(app.ui_settings.workshop_path.clone()),
            Message::Scanned,
        ),
        Message::Scanned(result) => match result {
            Ok(entries) => {
                app.entries = entries;
                Task::none()
            }
            Err(_err) => Task::none(),
        },
        Message::SelectWallpaper(index) => {
            let Some(entry) = app.entries.get(index).cloned() else {
                return Task::none();
            };

            app.selected_id = Some(entry.id.clone());
            app.selected_schema = UserPropertySchema::from_project_file(&entry.project_json)
                .unwrap_or(UserPropertySchema { entries: Vec::new() });
            let profile = app.launch_settings.wallpapers.entry(entry.id.clone()).or_default().clone();
            set_resolution_inputs(app, &profile);
            let cfg = build_config_for_wallpaper(&app.launch_settings, &entry.id, &entry.project_json);
            let _ = save_config(&app.config_path, &cfg);
            Task::none()
        }
        Message::Detail(message) => update_wallpaper_detail(app, message),
        Message::PlayPressed => {
            if !app.layerd_available {
                app.ui_settings.status_text = "we-layerd not found in PATH".to_string();
                return Task::none();
            }

            persist_current_config(app);

            reap_runtime_child(app);

            if try_switch_runtime(&app.config_path) {
                app.ui_settings.status_text = "switched running daemon".to_string();
                return Task::none();
            }

            let spawn =
                Command::new("we-layerd").arg("run").arg("--config").arg(&app.config_path).spawn();

            match spawn {
                Ok(child) => {
                    app.runtime_child = Some(child);
                    app.ui_settings.status_text = "started daemon".to_string();
                }
                Err(err) => {
                    app.ui_settings.status_text = format!("failed to start daemon: {err}");
                    eprintln!("failed to start daemon: {err}");
                }
            }
            Task::none()
        }
        Message::StopPressed => {
            let stopped = stop_runtime(app);
            app.ui_settings.status_text = if stopped {
                "stopped daemon".to_string()
            } else {
                "daemon stop request failed".to_string()
            };
            if !stopped {
                eprintln!("failed to stop daemon via IPC or owned child process");
            }
            Task::none()
        }
        Message::SettingsPressed => {
            app.show_settings = !app.show_settings;
            if app.show_settings {
                return Task::perform(fetch_runtime_status(), Message::StatusLoaded);
            }
            persist_current_config(app);
            Task::none()
        }
        Message::AssetsPathChanged(value) => {
            app.ui_settings.assets_path = value;
            sync_launch_settings(app);
            Task::none()
        }
        Message::WorkshopPathChanged(value) => {
            app.ui_settings.workshop_path = value.clone();
            sync_launch_settings(app);
            if Path::new(&value).is_dir() {
                return Task::perform(
                    scan_wallpapers_from(app.ui_settings.workshop_path.clone()),
                    Message::Scanned,
                );
            }
            Task::none()
        }
        Message::RendererLibraryPathChanged(value) => {
            app.ui_settings.renderer_library_path = value;
            sync_launch_settings(app);
            Task::none()
        }
        Message::RendererCachePathChanged(value) => {
            app.ui_settings.renderer_cache_path = value;
            sync_launch_settings(app);
            Task::none()
        }
        Message::PickAssetsPath => Task::perform(
            async {
                rfd::FileDialog::new()
                    .set_title("Select Wallpaper Engine assets directory")
                    .pick_folder()
            },
            Message::AssetsPathPicked,
        ),
        Message::PickWorkshopPath => Task::perform(
            async {
                rfd::FileDialog::new().set_title("Select workshop 431960 folder").pick_folder()
            },
            Message::WorkshopPathPicked,
        ),
        Message::AssetsPathPicked(path) => {
            if let Some(path) = path {
                app.ui_settings.assets_path = path.display().to_string();
                sync_launch_settings(app);
            }
            Task::none()
        }
        Message::WorkshopPathPicked(path) => {
            if let Some(path) = path {
                app.ui_settings.workshop_path = path.display().to_string();
                sync_launch_settings(app);
                return Task::perform(
                    scan_wallpapers_from(app.ui_settings.workshop_path.clone()),
                    Message::Scanned,
                );
            }
            Task::none()
        }
        Message::FpsLimitChanged(value) => {
            app.ui_settings.fps_limit = value;
            sync_launch_settings(app);
            Task::none()
        }
        Message::InteractiveToggled(value) => {
            app.ui_settings.interactive = value;
            sync_launch_settings(app);
            Task::none()
        }
        Message::ShowFpsToggled(value) => {
            app.ui_settings.show_fps = value;
            sync_launch_settings(app);
            Task::none()
        }
        Message::ScaleModeSelected(value) => {
            app.ui_settings.scale_mode = value;
            sync_launch_settings(app);
            Task::none()
        }
        Message::PreferDmabufToggled(value) => {
            app.ui_settings.prefer_dmabuf = value;
            sync_launch_settings(app);
            Task::none()
        }
        Message::AllowShmFallbackToggled(value) => {
            app.ui_settings.allow_shm_fallback = value;
            sync_launch_settings(app);
            Task::none()
        }
        Message::StatusLoaded(result) => {
            app.ui_settings.status_text = match result {
                Ok(text) => text,
                Err(err) => format!("status unavailable: {err}"),
            };
            Task::none()
        }
        Message::StatusTick => {
            if app.show_settings {
                return Task::perform(fetch_runtime_status(), Message::StatusLoaded);
            }
            Task::none()
        }
        Message::WindowResized(size) => {
            app.viewport_width = size.width;
            Task::none()
        }
        Message::WindowCloseRequested(id) => window::close(id),
        Message::WindowOpened(id) => {
            app.main_window_id = Some(id);
            Task::none()
        }
        Message::WindowClosed(id) => {
            if app.main_window_id == Some(id) {
                app.main_window_id = None;
            }
            Task::none()
        }
        Message::TrayTick => {
            if let Some(tray) = app.tray.as_mut() {
                if let Some(action) = tray.poll_action() {
                    return Task::done(Message::TrayAction(action));
                }
            }
            Task::none()
        }
        Message::ThemeTick => {
            app.theme = detect_system_theme();
            Task::none()
        }
        Message::TrayAction(action) => match action {
            tray::TrayAction::ShowWindow => {
                if let Some(id) = app.main_window_id {
                    return window::gain_focus(id);
                }
                let (_id, task) = window::open(window::Settings::default());
                task.map(Message::WindowOpened)
            }
            tray::TrayAction::PlaySwitch => Task::done(Message::PlayPressed),
            tray::TrayAction::Stop => Task::done(Message::StopPressed),
            tray::TrayAction::Pause => {
                let _ = send_layerd_ctl("pause");
                Task::none()
            }
            tray::TrayAction::Resume => {
                let _ = send_layerd_ctl("resume");
                Task::none()
            }
            tray::TrayAction::Quit => iced::exit(),
        },
    }
}

fn view(app: &App, _window: window::Id) -> Element<'_, Message> {
    let grid = build_wallpaper_grid(&app.entries, app.selected_id.as_ref(), app.viewport_width);
    let grid = container(scrollable(grid).width(Fill).height(Fill)).width(Fill).height(Fill);
    let content: Element<'_, Message> = match app.selected_id.as_deref() {
        Some(selected_id) => match app.entries.iter().find(|entry| entry.id == selected_id) {
            Some(entry) => row![
                grid,
                wallpaper_detail::view(
                    entry,
                    app.launch_settings
                        .wallpapers
                        .get(selected_id)
                        .expect("selected wallpaper must have a profile"),
                    &app.selected_schema,
                    &app.resolution_width,
                    &app.resolution_height,
                )
                .map(Message::Detail),
            ]
            .into(),
            None => grid.into(),
        },
        None => grid.into(),
    };

    let floating = container(
        column![
            button(
                svg(svg::Handle::from_memory(include_bytes!("../assets/icons/stop.svg")))
                    .width(24)
                    .height(24),
            )
            .width(52)
            .height(52)
            .style(secondary_fab_style)
            .on_press(Message::StopPressed),
            button(
                svg(svg::Handle::from_memory(include_bytes!("../assets/icons/settings.svg")))
                    .width(24)
                    .height(24),
            )
            .width(52)
            .height(52)
            .style(secondary_fab_style)
            .on_press(Message::SettingsPressed),
            button(
                svg(svg::Handle::from_memory(include_bytes!("../assets/icons/play_arrow.svg")))
                    .width(28)
                    .height(28),
            )
            .width(60)
            .height(60)
            .style(primary_fab_style)
            .on_press(Message::PlayPressed),
        ]
        .spacing(12),
    )
    .width(Fill)
    .height(Fill)
    .align_x(Horizontal::Right)
    .align_y(Vertical::Bottom)
    .padding(20);

    let runtime_warning: Option<Element<'_, Message>> = if !app.layerd_available {
        let warning = container(
            text("we-layerd not found in PATH").size(28).color(Color::from_rgb8(150, 205, 255)),
        )
        .width(Fill)
        .height(Fill)
        .align_x(Horizontal::Center)
        .align_y(Vertical::Top)
        .padding(24);
        Some(warning.into())
    } else {
        None
    };

    let settings_overlay: Option<Element<'_, Message>> =
        if app.show_settings { Some(build_settings_overlay(&app.ui_settings)) } else { None };

    match (runtime_warning, settings_overlay) {
        (Some(w), Some(s)) => stack![content, w, s, floating].into(),
        (Some(w), None) => stack![content, w, floating].into(),
        (None, Some(s)) => stack![content, s, floating].into(),
        (None, None) => stack![content, floating].into(),
    }
}

async fn scan_wallpapers_from(workshop_path: String) -> Result<Vec<WallpaperEntry>, String> {
    let workshop_root = if workshop_path.trim().is_empty() {
        steam::discover_workshop_wallpaper_root()
            .ok_or_else(|| "cannot find Steam workshop path for app 431960".to_string())?
    } else {
        PathBuf::from(workshop_path)
    };
    wallpaper::scan_workshop_wallpapers(&workshop_root).map_err(|e| e.to_string())
}

fn wallpaper_type_name(ty: WallpaperType) -> &'static str {
    match ty {
        WallpaperType::Video => "video",
        WallpaperType::Scene => "scene",
        WallpaperType::Web => "web",
        WallpaperType::Unknown => "unknown",
    }
}

fn subscription(_app: &App) -> Subscription<Message> {
    Subscription::batch(vec![
        window::resize_events().map(|(_id, size)| Message::WindowResized(size)),
        window::open_events().map(Message::WindowOpened),
        window::close_events().map(Message::WindowClosed),
        window::close_requests().map(Message::WindowCloseRequested),
        iced::time::every(std::time::Duration::from_millis(250)).map(|_| Message::TrayTick),
        iced::time::every(std::time::Duration::from_secs(2)).map(|_| Message::ThemeTick),
        iced::time::every(std::time::Duration::from_secs(3)).map(|_| Message::StatusTick),
    ])
}

impl App {
    fn init() -> (Self, Task<Message>) {
        let config_path =
            steam::default_config_path().unwrap_or_else(|| PathBuf::from("config.toml"));
        let mut launch_settings =
            load_launch_settings(&config_path).unwrap_or_else(|_| LaunchSettings::default());
        if launch_settings.workshop_path.trim().is_empty() {
            launch_settings.workshop_path = steam::discover_workshop_wallpaper_root()
                .map(|p| p.display().to_string())
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
            fps_limit: launch_settings.fps_limit.to_string(),
            show_fps: launch_settings.show_fps,
            scale_mode: ScaleModeOption::from(launch_settings.scale_mode),
            status_text: "status unavailable: daemon is not running".to_string(),
        };
        (
            Self {
                entries: Vec::new(),
                selected_id: None,
                selected_schema: UserPropertySchema { entries: Vec::new() },
                resolution_width: String::new(),
                resolution_height: String::new(),
                config_path,
                runtime_child: None,
                viewport_width: 1280.0,
                layerd_available: command_exists_in_path("we-layerd"),
                launch_settings,
                ui_settings,
                show_settings: false,
                tray: tray::TrayController::new().ok(),
                main_window_id: None,
                theme: detect_system_theme(),
            },
            Task::batch(vec![
                Task::done(Message::AutoScan),
                window::open(window::Settings::default()).1.map(Message::WindowOpened),
            ]),
        )
    }
}

impl Drop for App {
    fn drop(&mut self) {}
}

fn build_wallpaper_grid<'a>(
    entries: &'a [WallpaperEntry],
    selected_id: Option<&String>,
    width: f32,
) -> Element<'a, Message> {
    let spacing = 12.0;
    let card_width = 360.0;
    let cols = ((width - spacing) / (card_width + spacing)).floor().max(1.0) as usize;

    let mut root = column!().spacing(spacing).padding(spacing);

    for (row_index, chunk) in entries.chunks(cols).enumerate() {
        let mut r = row!().spacing(spacing);
        for (inner, entry) in chunk.iter().enumerate() {
            let index = row_index * cols + inner;
            let is_selected = selected_id.map(|id| id == &entry.id).unwrap_or(false);
            r = r.push(make_wallpaper_card(entry, index, card_width, is_selected));
        }
        root = root.push(r);
    }

    root.into()
}

fn make_wallpaper_card<'a>(
    entry: &'a WallpaperEntry,
    index: usize,
    card_width: f32,
    is_selected: bool,
) -> Element<'a, Message> {
    let card_height = (card_width * 9.0 / 16.0).round();

    let media: Element<'a, Message> = if let Some(path) = &entry.preview {
        image(image::Handle::from_path(path))
            .width(card_width)
            .height(card_height)
            .content_fit(ContentFit::Cover)
            .into()
    } else {
        container(text(""))
            .width(card_width)
            .height(card_height)
            .style(|_theme: &Theme| container::Style {
                background: Some(Background::Color(Color::from_rgb8(18, 18, 18))),
                ..Default::default()
            })
            .into()
    };

    let chip = container(text(wallpaper_type_name(entry.ty)).size(12)).padding([3, 8]).style(
        |_theme: &Theme| container::Style {
            text_color: Some(Color::WHITE),
            background: Some(Background::Color(Color { r: 0.0, g: 0.0, b: 0.0, a: 0.45 })),
            border: Border { radius: 10.0.into(), ..Default::default() },
            ..Default::default()
        },
    );

    let chip_overlay = container(chip)
        .width(Fill)
        .height(Fill)
        .align_x(Horizontal::Right)
        .align_y(Vertical::Bottom)
        .padding(8);

    let composed = stack![media, chip_overlay];

    let border_color = if is_selected {
        Color::from_rgb8(45, 175, 255)
    } else {
        Color { r: 1.0, g: 1.0, b: 1.0, a: 0.1 }
    };

    let frame =
        container(composed).width(card_width).height(card_height).style(move |_theme: &Theme| {
            container::Style {
                border: Border {
                    radius: 14.0.into(),
                    width: if is_selected { 6.0 } else { 1.0 },
                    color: border_color,
                },
                shadow: if is_selected {
                    iced::Shadow {
                        color: Color::from_rgba8(45, 175, 255, 0.85),
                        blur_radius: 24.0,
                        offset: iced::Vector::new(0.0, 0.0),
                    }
                } else {
                    iced::Shadow::default()
                },
                ..Default::default()
            }
        });

    button(frame).on_press(Message::SelectWallpaper(index)).style(image_card_button_style).into()
}

fn sync_launch_settings_from_ui(ui_settings: &UiSettings, launch_settings: &mut LaunchSettings) {
    launch_settings.assets_path = ui_settings.assets_path.clone();
    launch_settings.workshop_path = ui_settings.workshop_path.clone();
    launch_settings.renderer_library_path = ui_settings.renderer_library_path.clone();
    launch_settings.renderer_cache_path = ui_settings.renderer_cache_path.clone();
    launch_settings.prefer_dmabuf = ui_settings.prefer_dmabuf;
    launch_settings.allow_shm_fallback = ui_settings.allow_shm_fallback;
    launch_settings.interactive = ui_settings.interactive;
    launch_settings.show_fps = ui_settings.show_fps;
    launch_settings.scale_mode = ScaleMode::from(ui_settings.scale_mode);

    if let Ok(v) = ui_settings.fps_limit.parse::<u32>() {
        launch_settings.fps_limit = v.clamp(1, 360);
    }
}

fn sync_launch_settings(app: &mut App) {
    sync_launch_settings_from_ui(&app.ui_settings, &mut app.launch_settings);
}

fn update_wallpaper_detail(
    app: &mut App,
    message: wallpaper_detail::DetailMessage,
) -> Task<Message> {
    use wallpaper_detail::{DetailMessage, ResolutionMode};

    if matches!(message, DetailMessage::Apply) {
        persist_current_config(app);
        return update(app, Message::PlayPressed);
    }
    if let DetailMessage::PickPath { key, directory } = message {
        return Task::perform(
            async move {
                let dialog = rfd::FileDialog::new().set_title("Select wallpaper property path");
                let path = if directory { dialog.pick_folder() } else { dialog.pick_file() };
                DetailMessage::PathPicked { key, path: path.map(|path| path.display().to_string()) }
            },
            Message::Detail,
        );
    }

    let Some(selected_id) = app.selected_id.clone() else {
        return Task::none();
    };
    let profile = app.launch_settings.wallpapers.entry(selected_id).or_default();
    match message {
        DetailMessage::Apply => unreachable!("apply handled before profile mutation"),
        DetailMessage::FpsChanged(value) => {
            if let Ok(fps) = value.parse::<u32>() {
                profile.fps = fps.clamp(1, 360);
            }
        }
        DetailMessage::SpeedChanged(value) => profile.speed = value,
        DetailMessage::VolumeChanged(value) => profile.volume = value,
        DetailMessage::MutedChanged(value) => profile.muted = value,
        DetailMessage::ResolutionModeChanged(ResolutionMode::Automatic) => {
            profile.render_resolution = RenderResolution::Automatic;
            app.resolution_width.clear();
            app.resolution_height.clear();
        }
        DetailMessage::ResolutionModeChanged(ResolutionMode::Fixed) => {
            let width = app.resolution_width.parse().unwrap_or(1920).max(1);
            let height = app.resolution_height.parse().unwrap_or(1080).max(1);
            profile.render_resolution = RenderResolution::Fixed { width, height };
            app.resolution_width = width.to_string();
            app.resolution_height = height.to_string();
        }
        DetailMessage::ResolutionWidthChanged(value) => {
            app.resolution_width = value;
            sync_fixed_resolution(profile, &app.resolution_width, &app.resolution_height);
        }
        DetailMessage::ResolutionHeightChanged(value) => {
            app.resolution_height = value;
            sync_fixed_resolution(profile, &app.resolution_width, &app.resolution_height);
        }
        DetailMessage::FillModeChanged(value) => profile.fill_mode = value,
        DetailMessage::RotationChanged(value) => profile.rotation_degrees = value,
        DetailMessage::PropertyChanged { key, value } => {
            profile.user_properties.insert(key, value);
        }
        DetailMessage::PathPicked { key, path } => {
            if let Some(path) = path {
                profile.user_properties.insert(key, serde_json::Value::String(path));
            }
        }
        DetailMessage::PickPath { .. } => unreachable!("path picker handled before profile mutation"),
        DetailMessage::ResetProperties => profile.user_properties.clear(),
    }
    persist_current_config(app);
    Task::none()
}

fn sync_fixed_resolution(profile: &mut WallpaperSettings, width: &str, height: &str) {
    let (Ok(width), Ok(height)) = (width.parse::<u32>(), height.parse::<u32>()) else {
        return;
    };
    profile.render_resolution = RenderResolution::Fixed { width: width.max(1), height: height.max(1) };
}

fn set_resolution_inputs(app: &mut App, profile: &WallpaperSettings) {
    match profile.render_resolution {
        RenderResolution::Automatic => {
            app.resolution_width.clear();
            app.resolution_height.clear();
        }
        RenderResolution::Fixed { width, height } => {
            app.resolution_width = width.to_string();
            app.resolution_height = height.to_string();
        }
    }
}

fn persist_current_config(app: &App) {
    let Some(selected_id) = app.selected_id.as_deref() else {
        return;
    };
    let Some(entry) = app.entries.iter().find(|entry| entry.id == selected_id) else {
        return;
    };

    let cfg = build_config_for_wallpaper(&app.launch_settings, &entry.id, &entry.project_json);
    let _ = save_config(&app.config_path, &cfg);
}

async fn fetch_runtime_status() -> Result<String, String> {
    let output =
        Command::new("we-layerd").arg("ctl").arg("status").output().map_err(|e| e.to_string())?;

    if output.status.success() {
        let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if text.is_empty() {
            Ok("status unavailable: daemon returned empty response".to_string())
        } else {
            Ok(text)
        }
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

fn image_card_button_style(_theme: &Theme, _status: button::Status) -> button::Style {
    button::Style {
        background: None,
        text_color: Color::WHITE,
        border: Border::default(),
        shadow: iced::Shadow::default(),
        ..Default::default()
    }
}

fn primary_fab_style(_theme: &Theme, status: button::Status) -> button::Style {
    let is_light = matches!(_theme, Theme::Light);
    let (r, g, b) = match (is_light, status) {
        (true, button::Status::Hovered) => (0.08, 0.47, 0.86),
        (true, button::Status::Pressed) => (0.06, 0.40, 0.78),
        (true, _) => (0.07, 0.44, 0.82),
        (false, button::Status::Hovered) => (0.13, 0.56, 0.96),
        (false, button::Status::Pressed) => (0.09, 0.48, 0.88),
        (false, _) => (0.11, 0.53, 0.93),
    };

    button::Style {
        background: Some(Background::Color(Color::from_rgb(r, g, b))),
        text_color: Color::WHITE,
        border: Border { radius: 30.0.into(), ..Default::default() },
        shadow: iced::Shadow {
            color: Color { a: 0.35, ..Color::BLACK },
            blur_radius: 12.0,
            offset: iced::Vector::new(0.0, 4.0),
        },
        ..Default::default()
    }
}

fn secondary_fab_style(_theme: &Theme, status: button::Status) -> button::Style {
    let is_light = matches!(_theme, Theme::Light);
    let bg = match (is_light, status) {
        (true, button::Status::Hovered) => Color::from_rgba(0.95, 0.95, 0.95, 0.95),
        (true, button::Status::Pressed) => Color::from_rgba(0.90, 0.90, 0.90, 0.98),
        (true, _) => Color::from_rgba(0.93, 0.93, 0.93, 0.92),
        (false, button::Status::Hovered) => Color::from_rgba(0.14, 0.14, 0.14, 0.82),
        (false, button::Status::Pressed) => Color::from_rgba(0.10, 0.10, 0.10, 0.88),
        (false, _) => Color::from_rgba(0.12, 0.12, 0.12, 0.78),
    };

    button::Style {
        background: Some(Background::Color(bg)),
        text_color: Color::WHITE,
        border: Border {
            radius: 26.0.into(),
            width: 1.0,
            color: Color::from_rgba(1.0, 1.0, 1.0, 0.14),
        },
        shadow: iced::Shadow {
            color: Color { a: 0.28, ..Color::BLACK },
            blur_radius: 10.0,
            offset: iced::Vector::new(0.0, 3.0),
        },
        ..Default::default()
    }
}

fn detect_system_theme() -> Theme {
    match dark_light::detect() {
        dark_light::Mode::Light => Theme::Light,
        dark_light::Mode::Dark => Theme::Dark,
        dark_light::Mode::Default => Theme::Dark,
    }
}

fn command_exists_in_path(name: &str) -> bool {
    let Some(path_os) = env::var_os("PATH") else {
        return false;
    };

    for dir in env::split_paths(&path_os) {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return true;
        }
    }

    false
}

fn try_switch_runtime(config_path: &Path) -> bool {
    Command::new("we-layerd")
        .arg("switch")
        .arg("--config")
        .arg(config_path)
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn stop_runtime(app: &mut App) -> bool {
    let stopped_by_ipc = send_layerd_ctl("stop");
    let mut stopped_any = stopped_by_ipc;

    if let Some(mut child) = app.runtime_child.take() {
        if wait_child_exit(&mut child, 3, 100) {
            stopped_any = true;
        } else {
            let _ = child.kill();
            let _ = child.wait();
            stopped_any = true;
        }
    }

    stopped_any
}

fn send_layerd_ctl(action: &str) -> bool {
    Command::new("we-layerd").arg("ctl").arg(action).status().map(|s| s.success()).unwrap_or(false)
}

fn reap_runtime_child(app: &mut App) {
    let Some(child) = app.runtime_child.as_mut() else {
        return;
    };
    match child.try_wait() {
        Ok(Some(_)) => {
            app.runtime_child = None;
        }
        Ok(None) => {}
        Err(err) => {
            eprintln!("failed to query daemon child status: {err}");
        }
    }
}

fn wait_child_exit(child: &mut Child, attempts: usize, sleep_ms: u64) -> bool {
    for _ in 0..attempts {
        match child.try_wait() {
            Ok(Some(_)) => return true,
            Ok(None) => std::thread::sleep(Duration::from_millis(sleep_ms)),
            Err(_) => return false,
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::{
        settings_panel::{ScaleModeOption, UiSettings},
        sync_launch_settings_from_ui,
    };
    use we_core::config::{LaunchSettings, ScaleMode};

    #[test]
    fn sync_launch_settings_copies_workshop_path_from_ui() {
        let ui_settings = UiSettings {
            assets_path: "/opt/wallpaper_engine/assets".to_string(),
            workshop_path: "/tmp/workshop/content/431960".to_string(),
            renderer_library_path: "/opt/libwallpaper-engine-renderer.so".to_string(),
            renderer_cache_path: "~/.cache/we-layerd/test".to_string(),
            prefer_dmabuf: false,
            allow_shm_fallback: true,
            interactive: false,
            fps_limit: "144".to_string(),
            show_fps: true,
            scale_mode: ScaleModeOption::Stretch,
            status_text: String::new(),
        };
        let mut launch_settings = LaunchSettings::default();

        sync_launch_settings_from_ui(&ui_settings, &mut launch_settings);

        assert_eq!(launch_settings.workshop_path, "/tmp/workshop/content/431960");
        assert_eq!(launch_settings.assets_path, "/opt/wallpaper_engine/assets");
        assert_eq!(launch_settings.renderer_library_path, "/opt/libwallpaper-engine-renderer.so");
        assert_eq!(launch_settings.renderer_cache_path, "~/.cache/we-layerd/test");
        assert!(!launch_settings.prefer_dmabuf);
        assert!(launch_settings.allow_shm_fallback);
        assert!(!launch_settings.interactive);
        assert_eq!(launch_settings.scale_mode, ScaleMode::Stretch);
    }
}
