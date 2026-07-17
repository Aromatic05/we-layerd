use std::sync::mpsc::{self, Receiver};

use tray_icon::{
    menu::{Menu, MenuEvent, MenuItem},
    Icon, TrayIcon, TrayIconBuilder,
};

use crate::domain::i18n::{Language, Text};

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
    #[cfg(target_os = "linux")]
    command_tx: mpsc::Sender<TrayCommand>,
    #[cfg(not(target_os = "linux"))]
    items: TrayItems,
}

impl TrayController {
    pub fn new(language: Language) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        #[cfg(target_os = "linux")]
        return new_linux(language);

        #[cfg(not(target_os = "linux"))]
        return new_other(language);
    }

    pub fn poll_action(&mut self) -> Option<TrayAction> {
        self.rx.try_recv().ok()
    }

    pub fn set_language(&mut self, language: Language) {
        #[cfg(target_os = "linux")]
        {
            let _ = self.command_tx.send(TrayCommand::SetLanguage(language));
        }

        #[cfg(not(target_os = "linux"))]
        self.items.set_language(language);
    }
}

#[cfg(target_os = "linux")]
enum TrayCommand {
    SetLanguage(Language),
}

struct TrayItems {
    show: MenuItem,
    play: MenuItem,
    stop: MenuItem,
    pause: MenuItem,
    resume: MenuItem,
    quit: MenuItem,
}

impl TrayItems {
    fn new(language: Language) -> Self {
        Self {
            show: MenuItem::new(language.text(Text::TrayShowWindow), true, None),
            play: MenuItem::new(language.text(Text::TrayPlaySwitch), true, None),
            stop: MenuItem::new(language.text(Text::TrayStop), true, None),
            pause: MenuItem::new(language.text(Text::TrayPause), true, None),
            resume: MenuItem::new(language.text(Text::TrayResume), true, None),
            quit: MenuItem::new(language.text(Text::TrayQuit), true, None),
        }
    }

    fn append_to(&self, menu: &Menu) -> Result<(), tray_icon::menu::Error> {
        menu.append(&self.show)?;
        menu.append(&self.play)?;
        menu.append(&self.stop)?;
        menu.append(&self.pause)?;
        menu.append(&self.resume)?;
        menu.append(&self.quit)
    }

    fn set_language(&self, language: Language) {
        self.show.set_text(language.text(Text::TrayShowWindow));
        self.play.set_text(language.text(Text::TrayPlaySwitch));
        self.stop.set_text(language.text(Text::TrayStop));
        self.pause.set_text(language.text(Text::TrayPause));
        self.resume.set_text(language.text(Text::TrayResume));
        self.quit.set_text(language.text(Text::TrayQuit));
    }
}

#[cfg(target_os = "linux")]
fn new_linux(language: Language) -> Result<TrayController, Box<dyn std::error::Error + Send + Sync>> {
    let (tx, rx) = mpsc::channel::<TrayAction>();
    let (command_tx, command_rx) = mpsc::channel::<TrayCommand>();
    std::thread::spawn(move || {
        if gtk::init().is_err() {
            return;
        }

        let menu = Menu::new();
        let items = TrayItems::new(language);

        if items.append_to(&menu).is_err() {
            return;
        }

        let show_id = items.show.id().0.clone();
        let play_id = items.play.id().0.clone();
        let stop_id = items.stop.id().0.clone();
        let pause_id = items.pause.id().0.clone();
        let resume_id = items.resume.id().0.clone();
        let quit_id = items.quit.id().0.clone();
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

        gtk::glib::timeout_add_local(std::time::Duration::from_millis(50), move || {
            while let Ok(command) = command_rx.try_recv() {
                match command {
                    TrayCommand::SetLanguage(language) => items.set_language(language),
                }
            }
            gtk::glib::ControlFlow::Continue
        });

        gtk::main();
    });

    Ok(TrayController { _tray: None, rx, command_tx })
}

#[cfg(not(target_os = "linux"))]
fn new_other(language: Language) -> Result<TrayController, Box<dyn std::error::Error + Send + Sync>> {
    let menu = Menu::new();
    let items = TrayItems::new(language);
    items.append_to(&menu)?;

    let icon = simple_icon()?;
    let tray = TrayIconBuilder::new()
        .with_tooltip("we-gui")
        .with_menu(Box::new(menu))
        .with_icon(icon)
        .build()?;

    let (tx, rx) = mpsc::channel::<TrayAction>();
    let menu_rx = MenuEvent::receiver();
    std::thread::spawn({
        let show_id = items.show.id().0.clone();
        let play_id = items.play.id().0.clone();
        let stop_id = items.stop.id().0.clone();
        let pause_id = items.pause.id().0.clone();
        let resume_id = items.resume.id().0.clone();
        let quit_id = items.quit.id().0.clone();
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

    Ok(TrayController { _tray: Some(tray), rx, items })
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
