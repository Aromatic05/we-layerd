mod app;
mod domain;
mod platform;
mod services;
mod ui;

fn main() -> iced::Result {
    let result = app::run();
    if result.is_ok() && app::was_interrupted() {
        std::process::exit(130);
    }
    result
}
