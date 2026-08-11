use std::time::Duration;

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
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
    pub delay: Duration,
}

#[derive(Debug, Clone)]
pub(crate) struct AnimatedPreview {
    pub frames: Vec<GifFrame>,
    pub current: usize,
    pub elapsed: Duration,
}
