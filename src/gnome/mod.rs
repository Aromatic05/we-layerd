mod dbus;

use std::{env, sync::mpsc};

use anyhow::Result;
use tracing::info;

use crate::{
    config::Config,
    ipc::{ControlCommand, RuntimeLoopExit},
};

pub(crate) const RENDERER_APP_ID: &str = "io.github.weLayerd.Renderer";

#[derive(Debug, Clone)]
pub(crate) struct RegisteredWindow {
    pub(crate) id: u32,
    pub(crate) pid: u32,
    pub(crate) title: String,
    pub(crate) wm_class: String,
}

pub(crate) fn registered_window() -> RegisteredWindow {
    let pid = std::process::id();
    RegisteredWindow {
        id: pid,
        pid,
        title: format!("we-layerd-renderer-{pid}"),
        wm_class: RENDERER_APP_ID.to_string(),
    }
}

pub(crate) fn is_gnome_session() -> bool {
    [
        env::var("XDG_CURRENT_DESKTOP").ok(),
        env::var("XDG_SESSION_DESKTOP").ok(),
        env::var("DESKTOP_SESSION").ok(),
    ]
    .into_iter()
    .flatten()
    .any(|value| value.to_ascii_lowercase().contains("gnome"))
}

pub(crate) fn run_session(
    cfg: &Config,
    control_rx: &mpsc::Receiver<ControlCommand>,
    runtime: impl FnOnce(
        &Config,
        &RegisteredWindow,
        &mpsc::Receiver<ControlCommand>,
    ) -> Result<RuntimeLoopExit>,
) -> Result<RuntimeLoopExit> {
    let client = dbus::GnomeShellClient::connect(&cfg.gnome.extension_dbus_name)?;
    let version = client.ping()?;
    let window = registered_window();
    client.register_window(&window)?;
    info!(
        version,
        pid = window.pid,
        title = %window.title,
        wm_class = %window.wm_class,
        "registered GNOME renderer window target"
    );

    let result = runtime(cfg, &window, control_rx);
    let unregister_result = client.unregister_window(&window);

    match (result, unregister_result) {
        (Ok(exit), Ok(())) => Ok(exit),
        (Err(err), Ok(())) => Err(err),
        (Ok(_), Err(err)) => Err(err),
        (Err(err), Err(unregister_err)) => Err(err.context(format!(
            "failed to unregister GNOME renderer window: {unregister_err}"
        ))),
    }
}

pub(crate) fn doctor(cfg: &Config, lines: &mut Vec<String>) {
    match dbus::GnomeShellClient::connect(&cfg.gnome.extension_dbus_name) {
        Ok(client) => match client.ping() {
            Ok(version) => lines.push(format!("OK gnome_extension = {}", version)),
            Err(err) => lines.push(format!("ERR gnome_extension = {err}")),
        },
        Err(err) => lines.push(format!("ERR gnome_extension = {err}")),
    }
}
