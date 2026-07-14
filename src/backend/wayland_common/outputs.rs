use anyhow::{anyhow, Context, Result};
use wayland_client::{
    protocol::{wl_output::{self, WlOutput}, wl_registry},
    Connection, Dispatch, QueueHandle,
};

use super::{connection, registry};

#[derive(Default)]
struct OutputProbe {
    names: Vec<(u32, String)>,
}

impl Dispatch<WlOutput, u32> for OutputProbe {
    fn event(
        state: &mut Self,
        _proxy: &WlOutput,
        event: wl_output::Event,
        global_name: &u32,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        if let wl_output::Event::Name { name } = event {
            state.names.push((*global_name, name));
        }
    }
}

impl Dispatch<wl_registry::WlRegistry, wayland_client::globals::GlobalListContents> for OutputProbe {
    fn event(
        _state: &mut Self,
        _proxy: &wl_registry::WlRegistry,
        _event: wl_registry::Event,
        _data: &wayland_client::globals::GlobalListContents,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
    }
}

pub(crate) fn list_output_names() -> Result<Vec<String>> {
    let conn = connection::connect_to_env()?;
    let (globals, mut queue) = registry::init_registry::<OutputProbe>(&conn)?;
    let qh = queue.handle();
    let mut probe = OutputProbe::default();

    for global in globals.contents().clone_list().into_iter().filter(|global| global.interface == "wl_output") {
        if global.version < 4 {
            return Err(anyhow!("wl_output global {} does not support stable output names", global.name));
        }
        globals.registry().bind::<WlOutput, _, _>(global.name, 4, &qh, global.name);
    }

    queue.roundtrip(&mut probe).context("failed to read Wayland output names")?;
    probe.names.sort_unstable_by_key(|(global_name, _)| *global_name);
    if probe.names.is_empty() {
        return Err(anyhow!("compositor did not expose any named wl_output"));
    }
    Ok(probe.names.into_iter().map(|(_, name)| name).collect())
}
