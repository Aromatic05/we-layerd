use anyhow::{anyhow, Context, Result};
use zbus::blocking::{Connection, Proxy};

use super::RegisteredWindow;

const DBUS_PATH: &str = "/io/github/weLayerd/Gnome";
const DBUS_INTERFACE: &str = "io.github.weLayerd.Gnome";

pub(super) struct GnomeShellClient {
    connection: Connection,
    bus_name: String,
}

impl GnomeShellClient {
    pub(super) fn connect(bus_name: &str) -> Result<Self> {
        let connection = Connection::session().context("failed to connect to the session D-Bus")?;
        let client = Self { connection, bus_name: bus_name.to_string() };
        let _ = client.proxy().context("failed to create GNOME extension proxy")?;
        Ok(client)
    }

    pub(super) fn ping(&self) -> Result<String> {
        self.proxy()?.call("Ping", &()).context("failed to ping GNOME extension")
    }

    pub(super) fn register_window(&self, window: &RegisteredWindow) -> Result<()> {
        let accepted: bool = self
            .proxy()?
            .call(
                "RegisterWindow",
                &(window.id, window.pid, window.title.as_str(), window.wm_class.as_str()),
            )
            .context("failed to register renderer window with GNOME extension")?;
        if accepted {
            Ok(())
        } else {
            Err(anyhow!("GNOME extension rejected the renderer window registration"))
        }
    }

    pub(super) fn unregister_window(&self, window: &RegisteredWindow) -> Result<()> {
        let removed: bool = self
            .proxy()?
            .call("UnregisterWindow", &(window.id,))
            .context("failed to unregister renderer window from GNOME extension")?;
        if removed {
            Ok(())
        } else {
            Err(anyhow!(
                "GNOME extension rejected the renderer window unregistration for pid {}",
                window.pid
            ))
        }
    }

    fn proxy(&self) -> Result<Proxy<'_>> {
        Proxy::new(&self.connection, self.bus_name.as_str(), DBUS_PATH, DBUS_INTERFACE)
            .context("failed to bind GNOME extension D-Bus proxy")
    }
}
