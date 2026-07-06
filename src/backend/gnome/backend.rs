use anyhow::Result;

use crate::{
    backend::traits::{BackendCapabilities, BackendContext, BackendKind, WallpaperBackend},
    ipc::RuntimeLoopExit,
};

#[derive(Default)]
pub(crate) struct GnomeBackend;

impl WallpaperBackend for GnomeBackend {
    fn kind(&self) -> BackendKind {
        BackendKind::Gnome
    }

    fn capabilities(&self) -> BackendCapabilities {
        BackendCapabilities {
            supports_dmabuf: true,
            supports_shm: true,
            supports_pointer_input: true,
            needs_external_extension: true,
            owns_wayland_surface: false,
        }
    }

    fn run(&mut self, ctx: BackendContext<'_>) -> Result<RuntimeLoopExit> {
        super::window_bridge::run(ctx)
    }
}
