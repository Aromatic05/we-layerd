/// A compositor surface role for the shared Wayland renderer/presenter.
#[derive(Debug, Clone, Copy)]
pub(crate) enum SurfaceRole<'a> {
    LayerShell,
    /// `fullscreen` asks the compositor to fullscreen the window on its output.
    /// Windows pinned by the GNOME extension skip it: the extension un-fullscreens
    /// and positions them itself, and a fullscreen round-trip that ends at the same
    /// rectangle can leave Mutter's window actor frozen with an empty surface.
    XdgToplevel {
        title: &'a str,
        app_id: &'a str,
        fullscreen: bool,
    },
}
