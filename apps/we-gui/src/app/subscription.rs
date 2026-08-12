use iced::{window, Subscription};

use crate::domain::library_grid::gif_tick_needed;

use super::{App, Message};

pub(crate) fn subscription(app: &App) -> Subscription<Message> {
    let mut subscriptions = vec![
        window::resize_events().map(|(_id, size)| Message::WindowResized(size)),
        window::open_events().map(Message::WindowOpened),
        window::close_events().map(Message::WindowClosed),
        window::close_requests().map(Message::WindowCloseRequested),
        iced::time::every(std::time::Duration::from_millis(250)).map(|_| Message::TrayTick),
        iced::time::every(std::time::Duration::from_secs(2)).map(|_| Message::ThemeTick),
        iced::time::every(std::time::Duration::from_secs(3)).map(|_| Message::StatusTick),
    ];
    if gif_tick_needed(app.animated_previews.len()) {
        subscriptions.push(
            iced::time::every(std::time::Duration::from_millis(16)).map(|_| Message::GifTick),
        );
    }
    Subscription::batch(subscriptions)
}
