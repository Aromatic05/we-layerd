use std::path::Path;

use anyhow::{Context, Result};
use we_renderer::{Frame, RenderConfig, RendererLibrary, Session, Source};

pub(crate) struct RendererSession {
    pub(crate) session: Session,
    pub(crate) _library: RendererLibrary,
}

impl RendererSession {
    pub(crate) fn create(library_path: &Path, cache_path: Option<&Path>) -> Result<Self> {
        let library = RendererLibrary::load(library_path)
            .with_context(|| format!("failed to load renderer library {}", library_path.display()))?;
        let session =
            library.create_session(cache_path).context("failed to create renderer session")?;
        Ok(Self { session, _library: library })
    }

    pub(crate) fn set_source(&mut self, source: Source) -> Result<()> {
        self.session.set_source(&source).context("failed to set renderer source")
    }

    pub(crate) fn configure(&mut self, config: RenderConfig) -> Result<()> {
        self.session.configure(config).context("failed to set render config")
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
}
