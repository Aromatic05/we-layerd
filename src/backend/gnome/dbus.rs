use anyhow::{anyhow, Context, Result};
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

/// Asks the extension to open GNOME Shell's desktop background menu at the
/// monitor-local logical position (x, y) on `connector`. The renderer window
/// covers the desktop, so GNOME's own background never sees the right-click.
pub(crate) fn show_background_menu(bus_name: &str, connector: &str, x: f64, y: f64) -> Result<bool> {
    let connection = Connection::session().map_err(|_| extension_unreachable_error())?;
    let proxy =
        Proxy::new(&connection, bus_name, protocol::OBJECT_PATH, protocol::WINDOW_BRIDGE_INTERFACE)
            .map_err(|_| extension_unreachable_error())?;
    proxy
        .call(protocol::SHOW_BACKGROUND_MENU_METHOD, &(connector, x, y))
        .context("failed to open the GNOME background menu")
}

/// Queries the extension for (output, activated, maximized, fullscreen) of
/// every regular toplevel so window rules can run without wlr protocols.
pub(crate) fn get_window_states(
    connection: &Connection,
    bus_name: &str,
) -> Result<Vec<(String, bool, bool, bool)>> {
    let proxy =
        Proxy::new(connection, bus_name, protocol::OBJECT_PATH, protocol::WINDOW_BRIDGE_INTERFACE)
            .map_err(|_| extension_unreachable_error())?;
    proxy
        .call(protocol::GET_WINDOW_STATES_METHOD, &())
        .context("failed to query GNOME window states")
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
    let (connection, proxy, version) = connect_window_bridge(bus_name)?;

    // The legacy extension protocol names this field xid, but actor matching
    // uses pid, title, and app-id for native Wayland windows. The id is simply
    // the registration token for UnregisterWindow.
    let id = pid;
    let accepted: bool =
        proxy.call(protocol::REGISTER_WINDOW_METHOD, &(id, pid, title, wm_class))?;
    anyhow::ensure!(accepted, "GNOME extension rejected renderer window registration");
    tracing::info!(%version, pid, title, "registered GNOME renderer window");
    drop(proxy);
    Ok(WindowRegistration { connection, bus_name: bus_name.to_string(), id })
}

/// Registers one renderer window that the extension pins to `connector`'s
/// monitor and clones into that monitor's background.
pub(crate) fn register_output_window(
    bus_name: &str,
    id: u32,
    pid: u32,
    title: &str,
    wm_class: &str,
    connector: &str,
) -> Result<WindowRegistration> {
    let (connection, proxy, version) = connect_window_bridge(bus_name)?;
    let accepted: bool = proxy
        .call(protocol::REGISTER_OUTPUT_WINDOW_METHOD, &(id, pid, title, wm_class, connector))?;
    anyhow::ensure!(
        accepted,
        "GNOME extension rejected renderer window registration for output {connector}"
    );
    tracing::info!(%version, pid, title, connector, "registered GNOME output renderer window");
    drop(proxy);
    Ok(WindowRegistration { connection, bus_name: bus_name.to_string(), id })
}

fn connect_window_bridge(bus_name: &str) -> Result<(Connection, Proxy<'static>, String)> {
    let connection = Connection::session().map_err(|_| extension_unreachable_error())?;
    let proxy = Proxy::new(
        &connection,
        bus_name.to_string(),
        protocol::OBJECT_PATH,
        protocol::WINDOW_BRIDGE_INTERFACE,
    )
    .map_err(|_| extension_unreachable_error())?;
    let version: String =
        proxy.call(protocol::PING_METHOD, &()).map_err(|_| extension_unreachable_error())?;
    Ok((connection, proxy, version))
}

impl Drop for WindowRegistration {
    fn drop(&mut self) {
        let result = Proxy::new(
            &self.connection,
            self.bus_name.as_str(),
            protocol::OBJECT_PATH,
            protocol::WINDOW_BRIDGE_INTERFACE,
        )
        .and_then(|proxy| {
            proxy.call::<_, _, bool>(protocol::UNREGISTER_WINDOW_METHOD, &(self.id,))
        });
        if let Err(error) = result {
            tracing::warn!(%error, "unable to unregister GNOME renderer window");
        }
    }
}
