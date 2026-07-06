use anyhow::{Context, Result};
use wayland_client::{globals::{registry_queue_init, GlobalList}, Connection, EventQueue};

pub(crate) fn init_registry<State: 'static>(
    conn: &Connection,
) -> Result<(GlobalList, EventQueue<State>)>
where
    State: wayland_client::Dispatch<wayland_client::protocol::wl_registry::WlRegistry, wayland_client::globals::GlobalListContents> + 'static,
{
    registry_queue_init::<State>(conn).context("failed to init Wayland registry")
}
