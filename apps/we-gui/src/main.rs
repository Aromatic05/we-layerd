use std::{
    env,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::Duration,
};

use iced::{
    alignment::{Horizontal, Vertical},
    widget::{button, column, container, image, row, scrollable, stack, svg, text},
    window, Background, Border, Color, ContentFit, Element, Fill, Size, Subscription, Task, Theme,
};
use settings_panel::{
    build_settings_overlay, detect_supported_resolutions, pick_initial_resolution,
    CgroupModeOption, ExecutableVariantOption, IsolationModeOption, LauncherChoice,
    LauncherModeOption, ResolutionOption, UiSettings,
};
use we_core::{
    config::{
        build_config, load_launch_settings, save_config, CgroupMode, IsolationMode, LaunchSettings,
        WindowsLauncher,
    },
    steam::{self, WallpaperEngineInstallState},
    wallpaper::{self, WallpaperEntry, WallpaperType},
};

mod settings_panel;
mod tray;

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
    config_path: PathBuf,
    runtime_child: Option<Child>,
    selected_type: Option<WallpaperType>,
    selected_video_file: Option<PathBuf>,
    viewport_width: f32,
    layerd_available: bool,
    launch_settings: LaunchSettings,
    ui_settings: UiSettings,
    show_settings: bool,
    supported_resolutions: Vec<ResolutionOption>,
    wine_commands: Vec<LauncherChoice>,
    proton_versions: Vec<LauncherChoice>,
    install_notice: Option<String>,
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
    WallpaperExeChanged(String),
    ExecutableVariantSelected(ExecutableVariantOption),
    WorkshopPathChanged(String),
    LauncherModeSelected(LauncherModeOption),
    WineCommandSelected(LauncherChoice),
    ProtonVersionSelected(LauncherChoice),
    ProtonPathChanged(String),
    PickWallpaperExe,
    PickWorkshopPath,
    WallpaperExePicked(Option<PathBuf>),
    WorkshopPathPicked(Option<PathBuf>),
    FpsLimitChanged(String),
    ShowFpsToggled(bool),
    IsolationModeSelected(IsolationModeOption),
    IsolationCommandChanged(String),
    IsolationWidthChanged(String),
    IsolationHeightChanged(String),
    IsolationStartupTimeoutChanged(String),
    BorderlessToggled(bool),
    HideDebugWindowToggled(bool),
    DisableDebugWindowInputToggled(bool),
    HiddenWorkspaceNameChanged(String),
    ResolutionSelected(settings_panel::ResolutionOption),
    CgroupEnabledToggled(bool),
    CgroupModeSelected(CgroupModeOption),
    CgroupMemoryMaxChanged(String),
    CgroupCpuMaxChanged(String),
    RefreshStatus,
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
            app.selected_type = Some(entry.ty);
            app.selected_video_file = entry.source_file.clone();
            let cfg = build_config(
                &app.launch_settings,
                entry.ty,
                &entry.project_json,
                entry.source_file.as_deref(),
            );
            let _ = save_config(&app.config_path, &cfg);
            Task::none()
        }
        Message::PlayPressed => {
            if !app.layerd_available {
                return Task::none();
            }

            persist_current_config(app);

            if can_hot_switch(app.selected_type) && try_switch_runtime(&app.config_path) {
                return Task::none();
            }

            stop_runtime(app);

            let spawn =
                Command::new("we-layerd").arg("run").arg("--config").arg(&app.config_path).spawn();

            match spawn {
                Ok(child) => {
                    app.runtime_child = Some(child);
                }
                Err(_err) => {}
            }
            Task::none()
        }
        Message::StopPressed => {
            if !stop_runtime(app) {
                let _ = send_layerd_ctl("stop");
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
        Message::WallpaperExeChanged(value) => {
            app.ui_settings.wallpaper_exe = value;
            app.ui_settings.executable_variant =
                infer_executable_variant(&app.ui_settings.wallpaper_exe);
            sync_launch_settings(app);
            Task::none()
        }
        Message::ExecutableVariantSelected(value) => {
            app.ui_settings.executable_variant = value;
            if let Some(parent) = Path::new(&app.ui_settings.wallpaper_exe).parent() {
                app.ui_settings.wallpaper_exe = parent.join(value.filename()).display().to_string();
            }
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
        Message::LauncherModeSelected(value) => {
            app.ui_settings.launcher_mode = value;
            sync_launch_settings(app);
            Task::none()
        }
        Message::WineCommandSelected(value) => {
            app.ui_settings.wine_command = value.value;
            sync_launch_settings(app);
            Task::none()
        }
        Message::ProtonVersionSelected(value) => {
            app.ui_settings.proton_path = value.value;
            sync_launch_settings(app);
            Task::none()
        }
        Message::ProtonPathChanged(value) => {
            app.ui_settings.proton_path = value;
            sync_launch_settings(app);
            Task::none()
        }
        Message::PickWallpaperExe => Task::perform(
            async {
                rfd::FileDialog::new()
                    .set_title("Select wallpaper64.exe / wallpaper32.exe")
                    .pick_file()
            },
            Message::WallpaperExePicked,
        ),
        Message::PickWorkshopPath => Task::perform(
            async {
                rfd::FileDialog::new().set_title("Select workshop 431960 folder").pick_folder()
            },
            Message::WorkshopPathPicked,
        ),
        Message::WallpaperExePicked(path) => {
            if let Some(path) = path {
                app.ui_settings.wallpaper_exe = path.display().to_string();
                app.ui_settings.executable_variant =
                    infer_executable_variant(&app.ui_settings.wallpaper_exe);
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
        Message::ShowFpsToggled(value) => {
            app.ui_settings.show_fps = value;
            sync_launch_settings(app);
            Task::none()
        }
        Message::IsolationModeSelected(value) => {
            app.ui_settings.isolation_mode = value;
            sync_launch_settings(app);
            Task::none()
        }
        Message::IsolationCommandChanged(value) => {
            app.ui_settings.isolation_command = value;
            sync_launch_settings(app);
            Task::none()
        }
        Message::IsolationWidthChanged(value) => {
            app.ui_settings.isolation_width = value;
            sync_launch_settings(app);
            Task::none()
        }
        Message::IsolationHeightChanged(value) => {
            app.ui_settings.isolation_height = value;
            sync_launch_settings(app);
            Task::none()
        }
        Message::IsolationStartupTimeoutChanged(value) => {
            app.ui_settings.isolation_startup_timeout_secs = value;
            sync_launch_settings(app);
            Task::none()
        }
        Message::BorderlessToggled(value) => {
            app.ui_settings.borderless = value;
            sync_launch_settings(app);
            Task::none()
        }
        Message::HideDebugWindowToggled(value) => {
            app.ui_settings.hide_debug_window = value;
            sync_launch_settings(app);
            Task::none()
        }
        Message::DisableDebugWindowInputToggled(value) => {
            app.ui_settings.disable_debug_window_input = value;
            sync_launch_settings(app);
            Task::none()
        }
        Message::HiddenWorkspaceNameChanged(value) => {
            app.ui_settings.hidden_workspace_name = value;
            sync_launch_settings(app);
            Task::none()
        }
        Message::ResolutionSelected(value) => {
            app.ui_settings.selected_resolution = Some(value);
            sync_launch_settings(app);
            Task::none()
        }
        Message::CgroupEnabledToggled(value) => {
            app.ui_settings.cgroup_enabled = value;
            sync_launch_settings(app);
            Task::none()
        }
        Message::CgroupModeSelected(value) => {
            app.ui_settings.cgroup_mode = value;
            sync_launch_settings(app);
            Task::none()
        }
        Message::CgroupMemoryMaxChanged(value) => {
            app.ui_settings.cgroup_memory_max = value;
            sync_launch_settings(app);
            Task::none()
        }
        Message::CgroupCpuMaxChanged(value) => {
            app.ui_settings.cgroup_cpu_max = value;
            sync_launch_settings(app);
            Task::none()
        }
        Message::RefreshStatus => Task::perform(fetch_runtime_status(), Message::StatusLoaded),
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
            tray::TrayAction::Quit => {
                let _ = stop_runtime(app);
                iced::exit()
            }
        },
    }
}

fn view(app: &App, _window: window::Id) -> Element<'_, Message> {
    let grid = build_wallpaper_grid(&app.entries, app.selected_id.as_ref(), app.viewport_width);

    let content = container(scrollable(grid).width(Fill).height(Fill)).width(Fill).height(Fill);

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

    let mut notice_lines: Vec<String> = Vec::new();
    if !app.layerd_available {
        notice_lines.push("we-layerd not found in PATH".to_string());
    }
    if let Some(msg) = &app.install_notice {
        notice_lines.push(msg.clone());
    }

    let runtime_warning: Option<Element<'_, Message>> = if notice_lines.is_empty() {
        None
    } else {
        let mut warning_col = column!().spacing(6);
        for line in notice_lines {
            warning_col =
                warning_col.push(text(line).size(28).color(Color::from_rgb8(150, 205, 255)));
        }
        let warning = container(warning_col)
            .width(Fill)
            .height(Fill)
            .align_x(Horizontal::Center)
            .align_y(Vertical::Top)
            .padding(24);
        Some(warning.into())
    };

    let settings_overlay: Option<Element<'_, Message>> = if app.show_settings {
        Some(build_settings_overlay(
            &app.ui_settings,
            &app.supported_resolutions,
            &app.wine_commands,
            &app.proton_versions,
        ))
    } else {
        None
    };

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
        let install_state =
            steam::detect_wallpaper_engine_install_state(&launch_settings.wallpaper_exe);
        let install_notice = match &install_state {
            WallpaperEngineInstallState::NotInstalled => Some(
                "Wallpaper Engine is not installed. Please install it, or choose paths in Settings."
                    .to_string(),
            ),
            WallpaperEngineInstallState::FirstRunRequired { .. } => Some(
                "Wallpaper Engine first-run setup is pending. Launch it once in Steam to run installer.exe."
                    .to_string(),
            ),
            WallpaperEngineInstallState::Installed { exe_path, .. } => {
                if launch_settings.wallpaper_exe.trim().is_empty() {
                    launch_settings.wallpaper_exe = exe_path.display().to_string();
                }
                None
            }
        };
        if launch_settings.workshop_path.trim().is_empty() {
            launch_settings.workshop_path = steam::discover_workshop_wallpaper_root()
                .map(|p| p.display().to_string())
                .unwrap_or_default();
        }
        let supported_resolutions = detect_supported_resolutions();
        let wine_commands = steam::discover_wine_commands()
            .into_iter()
            .map(|v| LauncherChoice { label: v.clone(), value: v })
            .collect::<Vec<_>>();
        let proton_versions = steam::discover_proton_installs()
            .into_iter()
            .map(|p| LauncherChoice { label: p.name, value: p.proton_path.display().to_string() })
            .collect::<Vec<_>>();
        if let Some(first) = wine_commands.first() {
            launch_settings.wine_command = first.value.clone();
        }
        if let Some(first) = proton_versions.first() {
            launch_settings.proton_path = Some(first.value.clone());
        }
        let selected_resolution = pick_initial_resolution(
            &supported_resolutions,
            launch_settings.width,
            launch_settings.height,
        );
        let ui_settings = UiSettings {
            wallpaper_exe: launch_settings.wallpaper_exe.clone(),
            executable_variant: infer_executable_variant(&launch_settings.wallpaper_exe),
            workshop_path: launch_settings.workshop_path.clone(),
            launcher_mode: match launch_settings.launcher {
                WindowsLauncher::Wine => LauncherModeOption::Wine,
                WindowsLauncher::Proton => LauncherModeOption::Proton,
            },
            wine_command: launch_settings.wine_command.clone(),
            proton_path: launch_settings.proton_path.clone().unwrap_or_default(),
            fps_limit: launch_settings.fps_limit.to_string(),
            show_fps: launch_settings.show_fps,
            isolation_mode: match launch_settings.isolation_mode {
                IsolationMode::None => IsolationModeOption::None,
                IsolationMode::GamescopeHeadless => IsolationModeOption::GamescopeHeadless,
            },
            isolation_command: launch_settings.isolation_command.clone(),
            isolation_width: launch_settings
                .isolation_width
                .map(|value| value.to_string())
                .unwrap_or_default(),
            isolation_height: launch_settings
                .isolation_height
                .map(|value| value.to_string())
                .unwrap_or_default(),
            isolation_startup_timeout_secs: launch_settings
                .isolation_startup_timeout_secs
                .to_string(),
            borderless: launch_settings.borderless,
            hide_debug_window: launch_settings.hide_debug_window,
            disable_debug_window_input: launch_settings.disable_debug_window_input,
            hidden_workspace_name: launch_settings.hidden_workspace_name.clone(),
            selected_resolution,
            cgroup_enabled: launch_settings.cgroup_enabled,
            cgroup_mode: match launch_settings.cgroup_mode {
                CgroupMode::Detect => CgroupModeOption::Detect,
                CgroupMode::LimitWine => CgroupModeOption::LimitWine,
            },
            cgroup_memory_max: launch_settings.cgroup_memory_max.clone().unwrap_or_default(),
            cgroup_cpu_max: launch_settings.cgroup_cpu_max.clone().unwrap_or_default(),
            status_text: "status unavailable: daemon is not running".to_string(),
        };
        (
            Self {
                entries: Vec::new(),
                selected_id: None,
                config_path,
                runtime_child: None,
                selected_type: None,
                selected_video_file: None,
                viewport_width: 1280.0,
                layerd_available: command_exists_in_path("we-layerd"),
                launch_settings,
                ui_settings,
                show_settings: false,
                supported_resolutions,
                wine_commands,
                proton_versions,
                install_notice,
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
    fn drop(&mut self) {
        let _ = stop_runtime(self);
    }
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
    launch_settings.wallpaper_exe = ui_settings.wallpaper_exe.clone();
    launch_settings.workshop_path = ui_settings.workshop_path.clone();
    launch_settings.launcher = match ui_settings.launcher_mode {
        LauncherModeOption::Wine => WindowsLauncher::Wine,
        LauncherModeOption::Proton => WindowsLauncher::Proton,
    };
    launch_settings.wine_command = ui_settings.wine_command.clone();
    launch_settings.proton_path = non_empty_trimmed(&ui_settings.proton_path);
    launch_settings.show_fps = ui_settings.show_fps;
    launch_settings.isolation_mode = match ui_settings.isolation_mode {
        IsolationModeOption::None => IsolationMode::None,
        IsolationModeOption::GamescopeHeadless => IsolationMode::GamescopeHeadless,
    };
    launch_settings.isolation_command = if ui_settings.isolation_command.trim().is_empty() {
        "gamescope".to_string()
    } else {
        ui_settings.isolation_command.trim().to_string()
    };
    launch_settings.isolation_width = parse_optional_u32(&ui_settings.isolation_width);
    launch_settings.isolation_height = parse_optional_u32(&ui_settings.isolation_height);
    if let Ok(v) = ui_settings.isolation_startup_timeout_secs.trim().parse::<u64>() {
        launch_settings.isolation_startup_timeout_secs = v.max(1);
    }
    launch_settings.borderless = ui_settings.borderless;
    launch_settings.play_in_window_title = "WE-DEBUG-WINDOW".to_string();
    launch_settings.wm_class_contains =
        infer_wm_class(ui_settings.launcher_mode, ui_settings.executable_variant).to_string();
    launch_settings.x = 0;
    launch_settings.y = 0;

    if let Ok(v) = ui_settings.fps_limit.parse::<u32>() {
        launch_settings.fps_limit = v.clamp(1, 360);
    }
    if let Some(res) = &ui_settings.selected_resolution {
        launch_settings.width = res.width;
        launch_settings.height = res.height;
    }
    launch_settings.cgroup_enabled = ui_settings.cgroup_enabled;
    launch_settings.cgroup_mode = match ui_settings.cgroup_mode {
        CgroupModeOption::Detect => CgroupMode::Detect,
        CgroupModeOption::LimitWine => CgroupMode::LimitWine,
    };
    launch_settings.cgroup_memory_max = non_empty_trimmed(&ui_settings.cgroup_memory_max);
    launch_settings.cgroup_cpu_max = non_empty_trimmed(&ui_settings.cgroup_cpu_max);
    launch_settings.hide_debug_window = ui_settings.hide_debug_window;
    launch_settings.disable_debug_window_input = ui_settings.disable_debug_window_input;
    launch_settings.hidden_workspace_name = ui_settings.hidden_workspace_name.clone();
}

fn sync_launch_settings(app: &mut App) {
    sync_launch_settings_from_ui(&app.ui_settings, &mut app.launch_settings);
}

fn persist_current_config(app: &App) {
    let Some(selected_id) = app.selected_id.as_deref() else {
        return;
    };
    let Some(entry) = app.entries.iter().find(|entry| entry.id == selected_id) else {
        return;
    };

    let cfg = build_config(
        &app.launch_settings,
        entry.ty,
        &entry.project_json,
        entry.source_file.as_deref(),
    );
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

fn non_empty_trimmed(input: &str) -> Option<String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn parse_optional_u32(input: &str) -> Option<u32> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        None
    } else {
        trimmed.parse::<u32>().ok().filter(|value| *value > 0)
    }
}

fn infer_executable_variant(path: &str) -> ExecutableVariantOption {
    let lower = Path::new(path)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if lower == "wallpaper32.exe" {
        ExecutableVariantOption::Wallpaper32
    } else {
        ExecutableVariantOption::Wallpaper64
    }
}

fn infer_wm_class(launcher: LauncherModeOption, variant: ExecutableVariantOption) -> &'static str {
    match launcher {
        LauncherModeOption::Proton => "steam_proton",
        LauncherModeOption::Wine => match variant {
            ExecutableVariantOption::Wallpaper64 => "wallpaper64",
            ExecutableVariantOption::Wallpaper32 => "wallpaper32",
        },
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

#[cfg(test)]
mod tests {
    use super::{
        infer_executable_variant,
        settings_panel::{
            CgroupModeOption, IsolationModeOption, LauncherModeOption, ResolutionOption, UiSettings,
        },
        sync_launch_settings_from_ui,
    };
    use we_core::config::{IsolationMode, LaunchSettings};

    #[test]
    fn sync_launch_settings_copies_workshop_path_from_ui() {
        let ui_settings = UiSettings {
            wallpaper_exe: "/tmp/wallpaper32.exe".to_string(),
            executable_variant: infer_executable_variant("/tmp/wallpaper32.exe"),
            workshop_path: "/tmp/workshop/content/431960".to_string(),
            launcher_mode: LauncherModeOption::Wine,
            wine_command: "wine".to_string(),
            proton_path: String::new(),
            fps_limit: "144".to_string(),
            show_fps: true,
            isolation_mode: IsolationModeOption::GamescopeHeadless,
            isolation_command: "gamescope-custom".to_string(),
            isolation_width: "2560".to_string(),
            isolation_height: "1600".to_string(),
            isolation_startup_timeout_secs: "12".to_string(),
            borderless: true,
            hide_debug_window: true,
            disable_debug_window_input: false,
            hidden_workspace_name: "top".to_string(),
            selected_resolution: Some(ResolutionOption { width: 1920, height: 1080 }),
            cgroup_enabled: false,
            cgroup_mode: CgroupModeOption::Detect,
            cgroup_memory_max: String::new(),
            cgroup_cpu_max: String::new(),
            status_text: String::new(),
        };
        let mut launch_settings = LaunchSettings::default();

        sync_launch_settings_from_ui(&ui_settings, &mut launch_settings);

        assert_eq!(launch_settings.workshop_path, "/tmp/workshop/content/431960");
        assert_eq!(launch_settings.wallpaper_exe, "/tmp/wallpaper32.exe");
        assert_eq!(launch_settings.width, 1920);
        assert_eq!(launch_settings.height, 1080);
        assert_eq!(launch_settings.isolation_mode, IsolationMode::GamescopeHeadless);
        assert_eq!(launch_settings.isolation_command, "gamescope-custom");
        assert_eq!(launch_settings.isolation_width, Some(2560));
        assert_eq!(launch_settings.isolation_height, Some(1600));
        assert_eq!(launch_settings.isolation_startup_timeout_secs, 12);
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

fn can_hot_switch(selected_type: Option<WallpaperType>) -> bool {
    selected_type.is_some()
}

fn try_switch_runtime(config_path: &Path) -> bool {
    Command::new("we-layerd")
        .arg("switch")
        .arg("--config")
        .arg(config_path)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn stop_runtime(app: &mut App) -> bool {
    let stopped_by_ipc = send_layerd_ctl("stop");
    let mut stopped_any = stopped_by_ipc;

    if let Some(mut child) = app.runtime_child.take() {
        if wait_child_exit(&mut child, 40, 100) {
            stopped_any = true;
        } else {
            let _ = send_process_signal(child.id(), "INT");
            if wait_child_exit(&mut child, 30, 100) {
                stopped_any = true;
            } else {
                let _ = send_process_signal(child.id(), "TERM");
                if wait_child_exit(&mut child, 20, 100) {
                    stopped_any = true;
                } else {
                    let _ = child.kill();
                    let _ = child.wait();
                    stopped_any = true;
                }
            }
        }
    }

    if cleanup_runtime_residue(&app.launch_settings.wallpaper_exe) {
        stopped_any = true;
    }

    stopped_any
}

fn send_layerd_ctl(action: &str) -> bool {
    Command::new("we-layerd")
        .arg("ctl")
        .arg(action)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
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

fn send_process_signal(pid: u32, signal: &str) -> bool {
    Command::new("kill")
        .arg(format!("-{signal}"))
        .arg(pid.to_string())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[cfg(target_os = "linux")]
fn cleanup_runtime_residue(wallpaper_exe: &str) -> bool {
    let mut any = false;
    let mut patterns = vec![
        "we-layerd run".to_string(),
        "wallpaper64.exe".to_string(),
        "wallpaper32.exe".to_string(),
        "explorer.exe".to_string(),
        "ui32.exe".to_string(),
    ];
    if !wallpaper_exe.trim().is_empty() {
        patterns.push(wallpaper_exe.to_string());
    }

    for pattern in &patterns {
        any |= pkill_for_user("TERM", pattern);
    }
    std::thread::sleep(Duration::from_millis(120));
    for pattern in &patterns {
        any |= pkill_for_user("KILL", pattern);
    }
    any
}

#[cfg(target_os = "linux")]
fn pkill_for_user(signal: &str, pattern: &str) -> bool {
    let user = env::var("USER").unwrap_or_default();
    let mut cmd = Command::new("pkill");
    cmd.arg(format!("-{signal}"));
    if !user.is_empty() {
        cmd.args(["-u", &user]);
    }
    cmd.args(["-f", pattern]).stdout(Stdio::null()).stderr(Stdio::null());
    cmd.status().map(|s| s.success()).unwrap_or(false)
}

#[cfg(not(target_os = "linux"))]
fn cleanup_runtime_residue(_wallpaper_exe: &str) -> bool {
    false
}
