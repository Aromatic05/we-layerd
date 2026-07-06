pub(crate) mod diagnostics;
mod geometry;
mod renderer;
pub(crate) mod state;
#[allow(clippy::module_inception)]
mod wayland;

#[cfg(test)]
mod tests;

pub use renderer::{run_renderer_background_surface, run_renderer_window_surface};
