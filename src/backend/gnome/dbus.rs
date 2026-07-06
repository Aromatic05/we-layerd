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
