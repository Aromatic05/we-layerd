use std::time::Duration;

use iced::widget::image;

#[derive(Debug, Clone, Copy)]
pub(crate) enum Pane {
    Library,
    Sidebar,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Sidebar {
    Detail,
    Settings,
    Playlist,
    Profile,
}

#[derive(Debug, Clone)]
pub(crate) struct GifFrame {
    pub handle: image::Handle,
    pub decoded_bytes: usize,
    pub delay: Duration,
}

#[derive(Debug, Clone)]
pub(crate) struct AnimatedPreview {
    pub frames: Vec<GifFrame>,
    pub current: usize,
    pub elapsed: Duration,
}
