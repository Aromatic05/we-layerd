use anyhow::Result;

use crate::{
    backend::traits::{BackendCapabilities, BackendContext, BackendKind, WallpaperBackend},
    ipc::RuntimeLoopExit,
};

#[derive(Default)]
pub(crate) struct LayerShellBackend;

impl WallpaperBackend for LayerShellBackend {
    fn kind(&self) -> BackendKind {
        BackendKind::LayerShell
    }

    fn capabilities(&self) -> BackendCapabilities {
        BackendCapabilities {
            supports_dmabuf: true,
            supports_shm: true,
            supports_pointer_input: true,
            needs_external_extension: false,
            owns_wayland_surface: true,
        }
    }

    fn run(&mut self, ctx: BackendContext<'_>) -> Result<RuntimeLoopExit> {
        super::orchestrator::run(ctx)
    }
}
