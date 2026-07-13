use std::{
    collections::HashMap,
    env,
    fs::File,
    io::BufReader,
    path::{Path, PathBuf},
    process::{Child, Command},
    time::Duration,
};
use image_rs::AnimationDecoder;

use iced::{
    alignment::{Horizontal, Vertical},
    widget::{button, column, container, image, pane_grid, responsive, row, scrollable, stack, text, text_input},
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

#[derive(Debug, Clone, Copy)]
enum Pane {
    Library,
    Sidebar,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Sidebar {
    Detail,
    Settings,
}

#[derive(Debug, Clone)]
struct GifFrame {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
    delay: Duration,
}

#[derive(Debug, Clone)]
struct AnimatedPreview {
    frames: Vec<GifFrame>,
    current: usize,
    elapsed: Duration,
}

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
    sidebar: Option<Sidebar>,
    detail_tab: wallpaper_detail::DetailTab,
    playback_paused: bool,
    playback_running: bool,
    search_query: String,
    type_filter: Option<WallpaperType>,
    panes: pane_grid::State<Pane>,
    animated_previews: HashMap<PathBuf, AnimatedPreview>,
    tray: Option<tray::TrayController>,
    main_window_id: Option<window::Id>,
    theme: Theme,
}

#[derive(Debug, Clone)]
enum Message {
    AutoScan,
    Scanned(Result<Vec<WallpaperEntry>, String>),
    GifLoaded(PathBuf, Result<Vec<GifFrame>, String>),
    GifTick,
    SelectWallpaper(usize),
    PlayPressed,
    StopPressed,
    SettingsPressed,
    SearchChanged(String),
    TypeFilterSelected(Option<WallpaperType>),
    PaneResized(pane_grid::ResizeEvent),
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
                app.animated_previews.clear();
                Task::batch(app.entries.iter().filter_map(|entry| {
                    let path = entry.preview.as_ref()?.clone();
                    (path.extension().and_then(|ext| ext.to_str()) == Some("gif")).then(|| {
                        Task::perform(decode_gif(path.clone()), move |result| Message::GifLoaded(path, result))
                    })
                }))
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
            app.sidebar = Some(Sidebar::Detail);
            app.detail_tab = wallpaper_detail::DetailTab::Actions;
            Task::none()
        }
        Message::GifLoaded(path, result) => {
            if let Ok(frames) = result {
                if !frames.is_empty() {
                    app.animated_previews.insert(path, AnimatedPreview { frames, current: 0, elapsed: Duration::ZERO });
                }
            }
            Task::none()
        }
        Message::GifTick => {
            for preview in app.animated_previews.values_mut() {
                preview.elapsed += Duration::from_millis(16);
                while preview.elapsed >= preview.frames[preview.current].delay {
                    preview.elapsed -= preview.frames[preview.current].delay;
                    preview.current = (preview.current + 1) % preview.frames.len();
                }
            }
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
                app.playback_running = true;
                app.playback_paused = false;
                return Task::none();
            }

            let spawn =
                Command::new("we-layerd").arg("run").arg("--config").arg(&app.config_path).spawn();

            match spawn {
                Ok(child) => {
                    app.runtime_child = Some(child);
                    app.ui_settings.status_text = "started daemon".to_string();
                    app.playback_running = true;
                    app.playback_paused = false;
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
            app.playback_running = false;
            app.playback_paused = false;
            Task::none()
        }
        Message::SettingsPressed => {
            app.sidebar = match app.sidebar {
                Some(Sidebar::Settings) => None,
                _ => Some(Sidebar::Settings),
            };
            app.show_settings = app.sidebar == Some(Sidebar::Settings);
            if app.show_settings {
                return Task::perform(fetch_runtime_status(), Message::StatusLoaded);
            }
            Task::none()
        }
        Message::SearchChanged(value) => {
            app.search_query = value;
            Task::none()
        }
        Message::TypeFilterSelected(value) => {
            app.type_filter = value;
            Task::none()
        }
        Message::PaneResized(event) => {
            app.panes.resize(event.split, event.ratio.clamp(0.45, 0.82));
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
                if send_layerd_ctl("pause") {
                    app.playback_paused = true;
                }
                Task::none()
            }
            tray::TrayAction::Resume => {
                if send_layerd_ctl("resume") {
                    app.playback_running = true;
                    app.playback_paused = false;
                }
                Task::none()
            }
            tray::TrayAction::Quit => iced::exit(),
        },
    }
}

fn view(app: &App, _window: window::Id) -> Element<'_, Message> {
    let library = library_view(app);
    let content = if let Some(sidebar) = app.sidebar {
        pane_grid(&app.panes, |_pane, pane, _| {
            let content: Element<'_, Message> = match pane {
                Pane::Library => library_view(app),
                Pane::Sidebar => sidebar_view(app, sidebar),
            };
            pane_grid::Content::new(content)
        })
        .on_resize(8, Message::PaneResized)
        .spacing(1)
        .into()
    } else {
        library
    };

    if app.layerd_available {
        content
    } else {
        stack![
            content,
            container(text("we-layerd not found in PATH").size(18).color(Color::from_rgb8(255, 180, 171)))
                .width(Fill)
                .align_x(Horizontal::Center)
                .padding(16),
        ]
        .into()
    }
}

fn library_view(app: &App) -> Element<'_, Message> {
    let matches = app.entries.iter().enumerate().filter(|(_, entry)| {
        app.type_filter.is_none_or(|ty| entry.ty == ty)
            && entry.title.to_lowercase().contains(&app.search_query.to_lowercase())
    });
    let entries = matches.collect::<Vec<_>>();
    let grid = responsive(move |size| build_wallpaper_grid(entries.iter().copied(), app.selected_id.as_ref(), size.width, &app.animated_previews));
    let filters = row![
        filter_chip("All", app.type_filter.is_none(), None),
        filter_chip("Web", app.type_filter == Some(WallpaperType::Web), Some(WallpaperType::Web)),
        filter_chip("Scene", app.type_filter == Some(WallpaperType::Scene), Some(WallpaperType::Scene)),
        filter_chip("Video", app.type_filter == Some(WallpaperType::Video), Some(WallpaperType::Video)),
    ]
    .spacing(8);
    let toolbar = row![
        column![text("Wallpapers").size(28), text(format!("{} items", app.entries.len())).size(13)].spacing(2).width(Fill),
        button(text("⚙").size(20)).on_press(Message::SettingsPressed).style(top_bar_button_style),
    ]
    .align_y(Vertical::Center);
    container(column![
        toolbar,
        row![
            text_input("Search wallpapers", &app.search_query).on_input(Message::SearchChanged).padding(12).style(search_style).width(Fill),
            filters,
        ].spacing(12).align_y(Vertical::Center),
        scrollable(grid).width(Fill).height(Fill),
    ]
    .spacing(16))
    .padding(24)
    .width(Fill)
    .height(Fill)
    .style(library_style)
    .into()
}

fn sidebar_view(app: &App, sidebar: Sidebar) -> Element<'_, Message> {
    match sidebar {
        Sidebar::Settings => build_settings_overlay(&app.ui_settings),
        Sidebar::Detail => match app.selected_id.as_deref().and_then(|id| app.entries.iter().find(|entry| entry.id == id)) {
            Some(entry) => wallpaper_detail::view(
                entry,
                app.launch_settings.wallpapers.get(&entry.id).expect("selected wallpaper must have a profile"),
                &app.selected_schema,
                &app.resolution_width,
                &app.resolution_height,
                app.detail_tab,
                app.playback_running,
                app.playback_paused,
            )
            .map(Message::Detail),
            None => container(text("Select a wallpaper to view its details.")).padding(24).into(),
        },
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

async fn decode_gif(path: PathBuf) -> Result<Vec<GifFrame>, String> {
    let decoder = image_rs::codecs::gif::GifDecoder::new(BufReader::new(File::open(path).map_err(|err| err.to_string())?))
        .map_err(|err| err.to_string())?;
    decoder.into_frames().collect_frames().map_err(|err| err.to_string()).map(|frames| {
        frames.into_iter().map(|frame| {
            let (numerator, denominator) = frame.delay().numer_denom_ms();
            let milliseconds = (numerator / denominator.max(1)).max(16);
            let buffer = frame.into_buffer();
            GifFrame { width: buffer.width(), height: buffer.height(), pixels: buffer.into_raw(), delay: Duration::from_millis(milliseconds.into()) }
        }).collect()
    })
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
        iced::time::every(std::time::Duration::from_millis(16)).map(|_| Message::GifTick),
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
                sidebar: None,
                detail_tab: wallpaper_detail::DetailTab::Actions,
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
    entries: impl Iterator<Item = (usize, &'a WallpaperEntry)>,
    selected_id: Option<&String>,
    width: f32,
    animated_previews: &'a HashMap<PathBuf, AnimatedPreview>,
) -> Element<'a, Message> {
    let spacing = 12.0;
    let target_card_width = 360.0;
    let cols = ((width + spacing) / (target_card_width + spacing)).floor().max(1.0) as usize;
    let card_width = ((width - spacing * (cols.saturating_sub(1) as f32)) / cols as f32).max(180.0);

    let mut root = column!().spacing(spacing).padding(spacing);

    let entries = entries.collect::<Vec<_>>();
    for chunk in entries.chunks(cols) {
        let mut r = row!().spacing(spacing);
        for (index, entry) in chunk.iter() {
            let is_selected = selected_id.map(|id| id == &entry.id).unwrap_or(false);
            r = r.push(make_wallpaper_card(entry, *index, card_width, is_selected, animated_previews));
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
    animated_previews: &'a HashMap<PathBuf, AnimatedPreview>,
) -> Element<'a, Message> {
    let card_height = (card_width * 9.0 / 16.0).round();

    let media: Element<'a, Message> = if let Some(path) = &entry.preview {
        let handle = animated_previews.get(path).map(|preview| {
            let frame = &preview.frames[preview.current];
            image::Handle::from_rgba(frame.width, frame.height, frame.pixels.clone())
        }).unwrap_or_else(|| image::Handle::from_path(path));
        image(handle)
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

    let border_color = if is_selected { Color::from_rgb8(174, 198, 255) } else { Color::from_rgb8(70, 72, 78) };

    let frame =
        container(composed).width(card_width).height(card_height).style(move |_theme: &Theme| {
            container::Style {
                border: Border {
                    radius: 16.0.into(),
                    width: if is_selected { 2.0 } else { 1.0 },
                    color: border_color,
                },
                shadow: if is_selected {
                    iced::Shadow {
                        color: Color::from_rgba8(0, 0, 0, 0.35),
                        blur_radius: 8.0,
                        offset: iced::Vector::new(0.0, 2.0),
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
    match message {
        DetailMessage::SelectTab(tab) => {
            app.detail_tab = tab;
            return Task::none();
        }
        DetailMessage::TogglePlayback => {
            if !app.playback_running {
                return update(app, Message::PlayPressed);
            }
            let action = if app.playback_paused { "resume" } else { "pause" };
            if send_layerd_ctl(action) {
                app.playback_paused = !app.playback_paused;
            }
            return Task::none();
        }
        DetailMessage::Stop => return update(app, Message::StopPressed),
        _ => {}
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
        DetailMessage::Apply | DetailMessage::TogglePlayback | DetailMessage::Stop | DetailMessage::SelectTab(_) => unreachable!("detail action handled before profile mutation"),
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

fn library_style(_theme: &Theme) -> container::Style {
    container::Style { background: Some(Background::Color(Color::from_rgb8(24, 25, 28))), ..Default::default() }
}

fn search_style(_theme: &Theme, status: text_input::Status) -> text_input::Style {
    let border = if matches!(status, text_input::Status::Focused { .. }) {
        Color::from_rgb8(174, 198, 255)
    } else {
        Color::from_rgb8(140, 144, 153)
    };
    text_input::Style {
        background: Background::Color(Color::from_rgb8(43, 44, 48)),
        border: Border { radius: 28.0.into(), width: 1.0, color: border },
        icon: Color::from_rgb8(196, 199, 204),
        placeholder: Color::from_rgb8(196, 199, 204),
        value: Color::from_rgb8(230, 225, 229),
        selection: Color::from_rgb8(78, 99, 139),
    }
}

fn filter_chip<'a>(label: &'a str, selected: bool, value: Option<WallpaperType>) -> iced::widget::Button<'a, Message> {
    button(text(if selected { format!("✓ {label}") } else { label.to_string() }).size(14))
        .on_press(Message::TypeFilterSelected(value))
        .padding([8, 14])
        .style(move |_theme, status| {
            let background = if selected {
                Color::from_rgb8(70, 91, 129)
            } else if matches!(status, button::Status::Hovered) {
                Color::from_rgb8(54, 56, 62)
            } else {
                Color::TRANSPARENT
            };
            button::Style {
                background: Some(Background::Color(background)),
                text_color: if selected { Color::from_rgb8(222, 231, 255) } else { Color::from_rgb8(201, 203, 209) },
                border: Border { radius: 20.0.into(), width: if selected { 0.0 } else { 1.0 }, color: Color::from_rgb8(143, 147, 156) },
                ..Default::default()
            }
        })
}

fn top_bar_button_style(_theme: &Theme, status: button::Status) -> button::Style {
    let background = if matches!(status, button::Status::Hovered) { Color::from_rgb8(56, 58, 63) } else { Color::TRANSPARENT };
    button::Style { background: Some(Background::Color(background)), text_color: Color::from_rgb8(220, 225, 235), border: Border { radius: 20.0.into(), ..Default::default() }, ..Default::default() }
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
