use std::sync::{atomic::AtomicBool, mpsc};

use anyhow::Result;

use crate::{
    config::Config,
    ipc::{ControlCommand, RuntimeLoopExit},
    runtime::status::RuntimeStatusSnapshot,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendKind {
    LayerShell,
    Gnome,
}

impl BackendKind {
    pub fn as_config_str(self) -> &'static str {
        match self {
            Self::LayerShell => "layer-shell",
            Self::Gnome => "gnome",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BackendCapabilities {
    pub supports_dmabuf: bool,
    pub supports_shm: bool,
    pub supports_pointer_input: bool,
    pub needs_external_extension: bool,
    pub owns_wayland_surface: bool,
}

pub struct BackendContext<'a> {
    pub cfg: &'a Config,
    pub shutdown_requested: &'a AtomicBool,
    pub control_rx: &'a mpsc::Receiver<ControlCommand>,
    pub status_sink: &'a mut dyn FnMut(RuntimeStatusSnapshot),
}

pub trait WallpaperBackend {
    fn kind(&self) -> BackendKind;
    fn capabilities(&self) -> BackendCapabilities;
    fn run(&mut self, ctx: BackendContext<'_>) -> Result<RuntimeLoopExit>;
}
