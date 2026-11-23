mod app;
mod domain;
mod platform;
mod services;
mod ui;

fn main() -> iced::Result {
    if let platform::single_instance::Startup::AlreadyRunning =
        platform::single_instance::acquire()
    {
        // The running we-gui was asked to show its window.
        return Ok(());
    }
    // The login unit starts we-gui in tray-only mode so the mute/pause menu is
    // available without popping a window over the desktop at every login.
    let open_window = !std::env::args().any(|arg| arg == "--tray");
    let result = app::run(open_window);
    platform::single_instance::release();
    if result.is_ok() && app::was_interrupted() {
        std::process::exit(130);
    }
    result
}
