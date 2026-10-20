use anyhow::{Context, Result};

use crate::{
    backend::{
        gnome::dbus,
        layer_shell::event_loop,
        traits::BackendContext,
        wayland_common::{outputs, surface_role::SurfaceRole},
    },
    ipc::RuntimeLoopExit,
};

const RENDERER_APP_ID: &str = "io.github.weLayerd.Renderer";

pub(crate) fn run(ctx: BackendContext<'_>) -> Result<RuntimeLoopExit> {
    // The GNOME extension currently manages a single renderer window.
    // Pick the first advertised output; a multi-monitor window bridge requires
    // the extension protocol to accept multiple registered windows.
    let outputs = outputs::list_output_names().context("unable to enumerate GNOME outputs")?;
    let output = outputs.first().context("GNOME did not advertise any outputs")?;
    let pid = std::process::id();
    let title = format!("we-layerd-renderer-{pid}");

    // Register before mapping the XDG window. The extension also inspects newly
    // mapped windows, so it will discover the actor when it becomes available.
    let registration =
        dbus::register_window(&ctx.cfg.gnome.extension_dbus_name, pid, &title, RENDERER_APP_ID)?;

    let result = event_loop::run_output_with_role(
        ctx,
        output,
        SurfaceRole::XdgToplevel { title: &title, app_id: RENDERER_APP_ID },
    );
    drop(registration);
    result
}

#[cfg(test)]
mod tests {
    use super::RENDERER_APP_ID;

    #[test]
    fn gnome_renderer_app_id_matches_extension_registration() {
        assert_eq!(RENDERER_APP_ID, "io.github.weLayerd.Renderer");
    }
}
