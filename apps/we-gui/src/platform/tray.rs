use std::sync::mpsc::{self, Receiver};

use tray_icon::{
    menu::{Menu, MenuEvent, MenuItem},
    Icon, TrayIcon, TrayIconBuilder,
};

#[derive(Debug, Clone, Copy)]
pub enum TrayAction {
    ShowWindow,
    PlaySwitch,
    Stop,
    Pause,
    Resume,
    Quit,
}

pub struct TrayController {
    _tray: Option<TrayIcon>,
    rx: Receiver<TrayAction>,
}

impl TrayController {
    pub fn new() -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        #[cfg(target_os = "linux")]
        return new_linux();

        #[cfg(not(target_os = "linux"))]
        return new_other();
    }

    pub fn poll_action(&mut self) -> Option<TrayAction> {
        self.rx.try_recv().ok()
    }
}

#[cfg(target_os = "linux")]
fn new_linux() -> Result<TrayController, Box<dyn std::error::Error + Send + Sync>> {
    let (tx, rx) = mpsc::channel::<TrayAction>();
    std::thread::spawn(move || {
        if gtk::init().is_err() {
            return;
        }

        let menu = Menu::new();
        let show = MenuItem::new("Show Window", true, None);
        let play = MenuItem::new("Play / Switch", true, None);
        let stop = MenuItem::new("Stop", true, None);
        let pause = MenuItem::new("Pause", true, None);
        let resume = MenuItem::new("Resume", true, None);
        let quit = MenuItem::new("Quit", true, None);

        if menu.append(&show).is_err()
            || menu.append(&play).is_err()
            || menu.append(&stop).is_err()
            || menu.append(&pause).is_err()
            || menu.append(&resume).is_err()
            || menu.append(&quit).is_err()
        {
            return;
        }

        let show_id = show.id().0.clone();
        let play_id = play.id().0.clone();
        let stop_id = stop.id().0.clone();
        let pause_id = pause.id().0.clone();
        let resume_id = resume.id().0.clone();
        let quit_id = quit.id().0.clone();
        let tx_events = tx.clone();
        MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
            let id = event.id.0;
            let action = if id == show_id {
                Some(TrayAction::ShowWindow)
            } else if id == play_id {
                Some(TrayAction::PlaySwitch)
            } else if id == stop_id {
                Some(TrayAction::Stop)
            } else if id == pause_id {
                Some(TrayAction::Pause)
            } else if id == resume_id {
                Some(TrayAction::Resume)
            } else if id == quit_id {
                Some(TrayAction::Quit)
            } else {
                None
            };
            if let Some(action) = action {
                let _ = tx_events.send(action);
            }
        }));

        let Ok(icon) = simple_icon() else {
            return;
        };
        let Ok(_tray) = TrayIconBuilder::new()
            .with_tooltip("we-gui")
            .with_menu(Box::new(menu))
            .with_icon(icon)
            .build()
        else {
            return;
        };

        gtk::main();
    });

    Ok(TrayController { _tray: None, rx })
}

#[cfg(not(target_os = "linux"))]
fn new_other() -> Result<TrayController, Box<dyn std::error::Error + Send + Sync>> {
    let menu = Menu::new();
    let show = MenuItem::new("Show Window", true, None);
    let play = MenuItem::new("Play / Switch", true, None);
    let stop = MenuItem::new("Stop", true, None);
    let pause = MenuItem::new("Pause", true, None);
    let resume = MenuItem::new("Resume", true, None);
    let quit = MenuItem::new("Quit", true, None);

    menu.append(&show)?;
    menu.append(&play)?;
    menu.append(&stop)?;
    menu.append(&pause)?;
    menu.append(&resume)?;
    menu.append(&quit)?;

    let icon = simple_icon()?;
    let tray = TrayIconBuilder::new()
        .with_tooltip("we-gui")
        .with_menu(Box::new(menu))
        .with_icon(icon)
        .build()?;

    let (tx, rx) = mpsc::channel::<TrayAction>();
    let menu_rx = MenuEvent::receiver();
    std::thread::spawn({
        let show_id = show.id().0.clone();
        let play_id = play.id().0.clone();
        let stop_id = stop.id().0.clone();
        let pause_id = pause.id().0.clone();
        let resume_id = resume.id().0.clone();
        let quit_id = quit.id().0.clone();
        move || loop {
            let Ok(event) = menu_rx.recv() else {
                break;
            };
            let id = event.id.0;
            let action = if id == show_id {
                Some(TrayAction::ShowWindow)
            } else if id == play_id {
                Some(TrayAction::PlaySwitch)
            } else if id == stop_id {
                Some(TrayAction::Stop)
            } else if id == pause_id {
                Some(TrayAction::Pause)
            } else if id == resume_id {
                Some(TrayAction::Resume)
            } else if id == quit_id {
                Some(TrayAction::Quit)
            } else {
                None
            };

            if let Some(action) = action {
                let _ = tx.send(action);
            }
        }
    });

    Ok(TrayController { _tray: Some(tray), rx })
}

fn simple_icon() -> Result<Icon, Box<dyn std::error::Error + Send + Sync>> {
    if let Ok(icon) = icon_from_svg(22) {
        return Ok(icon);
    }

    let width = 16;
    let height = 16;
    let mut rgba = vec![0u8; width * height * 4];
    for y in 0..height {
        for x in 0..width {
            let i = (y * width + x) * 4;
            let edge = x == 0 || y == 0 || x == width - 1 || y == height - 1;
            if edge {
                rgba[i] = 30;
                rgba[i + 1] = 140;
                rgba[i + 2] = 240;
                rgba[i + 3] = 255;
            } else {
                rgba[i] = 10;
                rgba[i + 1] = 45;
                rgba[i + 2] = 70;
                rgba[i + 3] = 220;
            }
        }
    }
    Ok(Icon::from_rgba(rgba, width as u32, height as u32)?)
}

fn icon_from_svg(size: u32) -> Result<Icon, Box<dyn std::error::Error + Send + Sync>> {
    let svg = include_str!("../../assets/we-gui-logo.svg");
    let options = resvg::usvg::Options::default();
    let tree = resvg::usvg::Tree::from_str(svg, &options)?;
    let mut pixmap = resvg::tiny_skia::Pixmap::new(size, size).ok_or("failed to alloc pixmap")?;
    let target = resvg::tiny_skia::Transform::from_scale(
        size as f32 / tree.size().width(),
        size as f32 / tree.size().height(),
    );
    let mut pixmap_mut = pixmap.as_mut();
    resvg::render(&tree, target, &mut pixmap_mut);
    Ok(Icon::from_rgba(pixmap.take(), size, size)?)
}
