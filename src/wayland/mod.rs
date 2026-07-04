mod diagnostics;
mod geometry;
mod renderer;
mod state;
mod wayland;

#[cfg(test)]
mod tests;

pub use renderer::run_renderer_background_surface;
