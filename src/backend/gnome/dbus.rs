use anyhow::{anyhow, Result};
use zbus::blocking::{Connection, Proxy};

use crate::backend::gnome::protocol;

pub(crate) fn extension_unreachable_error() -> anyhow::Error {
    anyhow!("GNOME backend selected but we-layerd GNOME Shell extension is not reachable")
}

pub(crate) fn ping_extension(bus_name: &str) -> Result<String> {
    let connection = Connection::session().map_err(|_| extension_unreachable_error())?;
    let proxy =
        Proxy::new(&connection, bus_name, protocol::OBJECT_PATH, protocol::WINDOW_BRIDGE_INTERFACE)
            .map_err(|_| extension_unreachable_error())?;
    proxy
        .call::<_, _, String>(protocol::PING_METHOD, &())
        .map_err(|_| extension_unreachable_error())
}

/// Keeps the GNOME extension's actor-clone target registered until the window
/// runtime stops, including on errors and early returns.
pub(crate) struct WindowRegistration {
    connection: Connection,
    bus_name: String,
    id: u32,
}

pub(crate) fn register_window(
    bus_name: &str,
    pid: u32,
    title: &str,
    wm_class: &str,
) -> Result<WindowRegistration> {
    let connection = Connection::session().map_err(|_| extension_unreachable_error())?;
    let proxy =
        Proxy::new(&connection, bus_name, protocol::OBJECT_PATH, protocol::WINDOW_BRIDGE_INTERFACE)
            .map_err(|_| extension_unreachable_error())?;

    let version: String =
        proxy.call(protocol::PING_METHOD, &()).map_err(|_| extension_unreachable_error())?;

    // The legacy extension protocol names this field xid, but actor matching
    // uses pid, title, and app-id for native Wayland windows. The id is simply
    // the registration token for UnregisterWindow.
    let id = pid;
    let accepted: bool = proxy.call("RegisterWindow", &(id, pid, title, wm_class))?;
    anyhow::ensure!(accepted, "GNOME extension rejected renderer window registration");
    tracing::info!(%version, pid, title, "registered GNOME renderer window");
    Ok(WindowRegistration { connection, bus_name: bus_name.to_string(), id })
}

impl Drop for WindowRegistration {
    fn drop(&mut self) {
        let result = Proxy::new(
            &self.connection,
            self.bus_name.as_str(),
            protocol::OBJECT_PATH,
            protocol::WINDOW_BRIDGE_INTERFACE,
        )
        .and_then(|proxy| proxy.call::<_, _, bool>("UnregisterWindow", &(self.id,)));
        if let Err(error) = result {
            tracing::warn!(%error, "unable to unregister GNOME renderer window");
        }
    }
}
