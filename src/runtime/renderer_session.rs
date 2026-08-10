use std::{os::fd::RawFd, path::Path};

use anyhow::{Context, Result};
use we_renderer::{Frame, RenderConfig, RendererDiagnostics, RendererLibrary, Session, Source};

pub(crate) struct RendererSession {
    pub(crate) session: Session,
    pub(crate) _library: RendererLibrary,
}

impl RendererSession {
    pub(crate) fn create(library_path: &Path, cache_path: Option<&Path>) -> Result<Self> {
        let library = RendererLibrary::load(library_path).with_context(|| {
            format!("failed to load renderer library {}", library_path.display())
        })?;
        let session =
            library.create_session(cache_path).context("failed to create renderer session")?;
        Ok(Self { session, _library: library })
    }

    pub(crate) fn set_source(&mut self, source: Source) -> Result<()> {
        self.session.set_source(&source).context("failed to set renderer source")
    }

    pub(crate) fn frame_ready_fd(&self) -> Result<RawFd> {
        self.session.frame_ready_fd().context("failed to get renderer frame-ready fd")
    }

    pub(crate) fn set_dmabuf_formats(&mut self, formats: &[(u32, u64)]) -> Result<()> {
        self.session.set_dmabuf_formats(formats).context("failed to set renderer DMA-BUF formats")
    }

    pub(crate) fn configure(&mut self, config: RenderConfig) -> Result<()> {
        self.session.configure(config).context("failed to set render config")
    }

    pub(crate) fn resize_output(&mut self, width: u32, height: u32) -> Result<()> {
        self.session.resize_output(width, height).context("failed to resize renderer output")
    }

    pub(crate) fn play(&mut self) -> Result<()> {
        self.session.play().context("failed to start renderer session")
    }

    pub(crate) fn pause(&mut self) {
        let _ = self.session.pause();
    }

    pub(crate) fn resume(&mut self) {
        let _ = self.session.play();
    }

    pub(crate) fn stop(mut self) {
        let _ = self.session.stop();
    }

    pub(crate) fn tick(&mut self) -> Result<()> {
        self.session.tick().context("renderer tick failed")
    }

    pub(crate) fn acquire_frame(&mut self) -> Result<Option<Frame>> {
        self.session.acquire_frame().context("failed to acquire frame")
    }

    pub(crate) fn diagnostics(&self) -> Result<RendererDiagnostics> {
        self.session.diagnostics().context("failed to read renderer diagnostics")
    }
}
