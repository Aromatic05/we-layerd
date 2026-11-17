use anyhow::{Context, Result};

use crate::{
    backend::{
        gnome::{dbus, protocol},
        layer_shell::{
            event_loop,
            orchestrator::{self, WorkerSurface},
        },
        traits::BackendContext,
        wayland_common::{outputs, surface_role::SurfaceRole},
    },
    ipc::RuntimeLoopExit,
};

pub(crate) const RENDERER_APP_ID: &str = "io.github.weLayerd.Renderer";

/// Window title for an output's renderer. The extension matches registered
/// windows by pid, title, and app-id, so each output needs a distinct title.
pub(crate) fn output_window_title(output: &str) -> String {
    format!("we-layerd-renderer-{}-{output}", std::process::id())
}

pub(crate) fn run(ctx: BackendContext<'_>) -> Result<RuntimeLoopExit> {
    let bus_name = ctx.cfg.gnome.extension_dbus_name.clone();
    let version = wait_for_extension(&bus_name, EXTENSION_STARTUP_WAIT)?;
    if protocol::supports_output_windows(&version) {
        // One renderer window per output, each pinned by the extension to its
        // own monitor, sharing the layer-shell per-output worker orchestration.
        return orchestrator::run_with_surface(ctx, WorkerSurface::GnomeWindow { bus_name });
    }

    tracing::warn!(
        %version,
        "GNOME Shell is still running an older we-layerd extension that supports only one \
         renderer window; log out and back in to load the multi-monitor extension"
    );
    run_single_window(ctx)
}

/// How long a starting daemon waits for GNOME Shell to load the extension.
const EXTENSION_STARTUP_WAIT: std::time::Duration = std::time::Duration::from_secs(90);

/// At login the session can start the daemon before GNOME Shell has finished
/// enabling extensions (and extensions are off while the screen is locked).
/// Wait for the extension instead of failing, so the wallpaper is applied as
/// soon as both sides are up.
fn wait_for_extension(bus_name: &str, timeout: std::time::Duration) -> Result<String> {
    let deadline = std::time::Instant::now() + timeout;
    let mut announced = false;
    loop {
        match dbus::ping_extension(bus_name) {
            Ok(version) => {
                if announced {
                    tracing::info!(%version, "GNOME Shell extension is ready");
                }
                return Ok(version);
            }
            Err(error) if std::time::Instant::now() >= deadline => return Err(error),
            Err(_) => {
                if !announced {
                    tracing::info!(
                        timeout_secs = timeout.as_secs(),
                        "waiting for the we-layerd GNOME Shell extension to load"
                    );
                    announced = true;
                }
                std::thread::sleep(std::time::Duration::from_millis(500));
            }
        }
    }
}

/// Legacy bridge for extensions older than the output-window protocol: renders
/// only the first advertised output in one unpinned window.
fn run_single_window(ctx: BackendContext<'_>) -> Result<RuntimeLoopExit> {
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
        SurfaceRole::XdgToplevel { title: &title, app_id: RENDERER_APP_ID, fullscreen: true },
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

    #[test]
    fn missing_extension_is_waited_for_then_reported() {
        let started = std::time::Instant::now();
        let error = super::wait_for_extension(
            "io.github.weLayerd.DoesNotExist",
            std::time::Duration::from_millis(1200),
        )
        .expect_err("a missing extension must eventually fail");
        assert!(started.elapsed() >= std::time::Duration::from_millis(1200));
        assert!(error.to_string().contains("extension is not reachable"));
    }
}
