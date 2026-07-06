use anyhow::{Context, Result};
use wayland_client::Connection;

pub(crate) fn connect_to_env() -> Result<Connection> {
    Connection::connect_to_env().context("failed to connect to Wayland display")
}
