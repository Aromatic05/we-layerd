use iced::{window, Subscription};

use super::{Message};

pub(crate) fn subscription() -> Subscription<Message> {
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
