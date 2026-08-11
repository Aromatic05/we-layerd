use std::sync::{atomic::AtomicBool, mpsc, Arc, Mutex};

use anyhow::Result;

use crate::{
    config::Config,
    ipc::{ControlCommand, OutputPlaylistRequest, RuntimeLoopExit},
    runtime::{integrations::HostIntegrations, status::RuntimeStatusSnapshot},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendKind {
    LayerShell,
    Gnome,
}

impl BackendKind {
    pub fn as_config_str(self) -> &'static str {
        match self {
            Self::LayerShell => "layer_shell",
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
    pub desired_cfg: Arc<Mutex<Config>>,
    pub shutdown_requested: Arc<AtomicBool>,
    pub host_integrations: HostIntegrations,
    pub control_rx: &'a mpsc::Receiver<ControlCommand>,
    pub output_playlist_rx: Option<&'a mpsc::Receiver<OutputPlaylistRequest>>,
    pub status_sink: &'a mut dyn FnMut(RuntimeStatusSnapshot),
}

pub trait WallpaperBackend {
    fn kind(&self) -> BackendKind;
    fn capabilities(&self) -> BackendCapabilities;
    fn run(&mut self, ctx: BackendContext<'_>) -> Result<RuntimeLoopExit>;
}
