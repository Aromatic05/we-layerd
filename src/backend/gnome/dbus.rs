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

pub(crate) fn register_window(
    bus_name: &str,
    xid: u32,
    pid: u32,
    title: &str,
    wm_class: &str,
) -> Result<()> {
    let connection = Connection::session().map_err(|_| extension_unreachable_error())?;
    let proxy =
        Proxy::new(&connection, bus_name, protocol::OBJECT_PATH, protocol::WINDOW_BRIDGE_INTERFACE)
            .map_err(|_| extension_unreachable_error())?;
    let accepted: bool = proxy
        .call(protocol::REGISTER_WINDOW_METHOD, &(xid, pid, title, wm_class))
        .map_err(|error| anyhow!("GNOME extension rejected wallpaper window: {error}"))?;
    if !accepted {
        anyhow::bail!("GNOME extension did not accept the wallpaper window");
    }
    Ok(())
}

pub(crate) fn unregister_window(bus_name: &str, xid: u32) {
    let Ok(connection) = Connection::session() else {
        return;
    };
    let Ok(proxy) =
        Proxy::new(&connection, bus_name, protocol::OBJECT_PATH, protocol::WINDOW_BRIDGE_INTERFACE)
    else {
        return;
    };
    let _: Result<bool, _> = proxy.call(protocol::UNREGISTER_WINDOW_METHOD, &(xid,));
}
