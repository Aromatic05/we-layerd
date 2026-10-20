/// A compositor surface role for the shared Wayland renderer/presenter.
#[derive(Debug, Clone, Copy)]
pub(crate) enum SurfaceRole<'a> {
    LayerShell,
    XdgToplevel { title: &'a str, app_id: &'a str },
}
