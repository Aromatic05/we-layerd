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
    let result = app::run();
    platform::single_instance::release();
    if result.is_ok() && app::was_interrupted() {
        std::process::exit(130);
    }
    result
}
