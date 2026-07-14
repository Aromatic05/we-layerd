use std::{path::Path, time::Duration};

use iced::{window, Task};
use we_core::wallpaper::properties::UserPropertySchema;

use crate::{
    domain::ui_state::{AnimatedPreview, Sidebar},
    platform::tray,
    services::{config, runtime, wallpaper as wallpaper_service},
    ui::sidebar::detail as wallpaper_detail,
};

use super::{detail_update::{persist_current_config, set_resolution_inputs}, App, Message};

pub(crate) fn update(app: &mut App, message: Message) -> Task<Message> {
    match message {
        Message::AutoScan => Task::perform(
            wallpaper_service::scan(app.ui_settings.workshop_path.clone()),
            Message::Scanned,
        ),
        Message::Scanned(result) => match result {
            Ok(entries) => {
                app.entries = entries;
                app.animated_previews.clear();
                Task::batch(app.entries.iter().filter_map(|entry| {
                    let path = entry.preview.as_ref()?.clone();
                    (path.extension().and_then(|ext| ext.to_str()) == Some("gif")).then(|| {
                        Task::perform(wallpaper_service::decode_gif(path.clone()), move |result| Message::GifLoaded(path, result))
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
            config::persist_selected(&app.config_path, &app.launch_settings, &entry);
            app.sidebar = Some(Sidebar::Detail);
            app.detail_tab = wallpaper_detail::DetailTab::Actions;
            Task::perform(runtime::fetch_status(), Message::StatusLoaded)
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
        Message::Detail(message) => super::detail_update::update(app, message),
        Message::PlayPressed => {
            app.runtime_shutdown = false;
            if !app.layerd_available {
                app.ui_settings.status_text = "we-layerd not found in PATH".to_string();
                return Task::none();
            }

            persist_current_config(app);

            if let Err(error) = runtime::reap(&mut app.runtime_child) {
                eprintln!("failed to query daemon child status: {error}");
            }

            if runtime::try_switch(&app.config_path) {
                app.ui_settings.status_text = "switched running daemon".to_string();
                app.playback_running = true;
                app.playback_paused = false;
                return Task::none();
            }

            let spawn = runtime::start(&app.config_path);

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
            let stopped = app.shutdown_runtime();
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
            app.sidebar = match app.sidebar {
                Some(Sidebar::Settings) => None,
                _ => Some(Sidebar::Settings),
            };
            app.show_settings = app.sidebar == Some(Sidebar::Settings);
            if app.show_settings {
                return Task::perform(runtime::fetch_status(), Message::StatusLoaded);
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
            super::settings::sync(app);
            Task::none()
        }
        Message::WorkshopPathChanged(value) => {
            app.ui_settings.workshop_path = value.clone();
            super::settings::sync(app);
            if Path::new(&value).is_dir() {
                return Task::perform(
                    wallpaper_service::scan(app.ui_settings.workshop_path.clone()),
                    Message::Scanned,
                );
            }
            Task::none()
        }
        Message::RendererLibraryPathChanged(value) => {
            app.ui_settings.renderer_library_path = value;
            super::settings::sync(app);
            Task::none()
        }
        Message::RendererCachePathChanged(value) => {
            app.ui_settings.renderer_cache_path = value;
            super::settings::sync(app);
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
                super::settings::sync(app);
            }
            Task::none()
        }
        Message::WorkshopPathPicked(path) => {
            if let Some(path) = path {
                app.ui_settings.workshop_path = path.display().to_string();
                super::settings::sync(app);
                return Task::perform(
                    wallpaper_service::scan(app.ui_settings.workshop_path.clone()),
                    Message::Scanned,
                );
            }
            Task::none()
        }
        Message::FpsLimitChanged(value) => {
            app.ui_settings.fps_limit = value;
            super::settings::sync(app);
            Task::none()
        }
        Message::InteractiveToggled(value) => {
            app.ui_settings.interactive = value;
            super::settings::sync(app);
            Task::none()
        }
        Message::ShowFpsToggled(value) => {
            app.ui_settings.show_fps = value;
            super::settings::sync(app);
            Task::none()
        }
        Message::ScaleModeSelected(value) => {
            app.ui_settings.scale_mode = value;
            super::settings::sync(app);
            Task::none()
        }
        Message::PreferDmabufToggled(value) => {
            app.ui_settings.prefer_dmabuf = value;
            super::settings::sync(app);
            Task::none()
        }
        Message::AllowShmFallbackToggled(value) => {
            app.ui_settings.allow_shm_fallback = value;
            super::settings::sync(app);
            Task::none()
        }
        Message::StatusLoaded(result) => {
            app.ui_settings.status_text = match result {
                Ok(text) => {
                    app.playback_running = status_value(&text, "phase") == Some("running");
                    app.playback_paused = status_value(&text, "phase") == Some("paused");
                    app.running_source = status_value(&text, "source").map(str::to_string);
                    text
                }
                Err(err) => format!("status unavailable: {err}"),
            };
            Task::none()
        }
        Message::StatusTick => {
            Task::perform(runtime::fetch_status(), Message::StatusLoaded)
        }
        Message::OutputsLoaded(result) => {
            if let Ok(outputs) = result {
                app.selected_outputs = outputs.iter().cloned().collect();
                app.outputs = outputs;
            }
            Task::none()
        }
        Message::ToggleOutput(output) => {
            if !app.selected_outputs.remove(&output) {
                app.selected_outputs.insert(output);
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
            app.theme = super::init::detect_system_theme();
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
                if runtime::send_control("pause") {
                    app.playback_paused = true;
                }
                Task::none()
            }
            tray::TrayAction::Resume => {
                if runtime::send_control("resume") {
                    app.playback_running = true;
                    app.playback_paused = false;
                }
                Task::none()
            }
            tray::TrayAction::Quit => {
                if !app.shutdown_runtime() {
                    eprintln!("failed to stop daemon while exiting we-gui");
                }
                iced::exit()
            }
        },
    }
}

fn status_value<'a>(status: &'a str, key: &str) -> Option<&'a str> {
    status.lines().find_map(|line| line.strip_prefix(&format!("{key} = ")))
        .map(|value| value.trim_matches('"'))
}
