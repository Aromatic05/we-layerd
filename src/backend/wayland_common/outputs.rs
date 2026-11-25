use anyhow::{anyhow, Context, Result};
use wayland_client::{
    protocol::{
        wl_output::{self, WlOutput},
        wl_registry,
    },
    Connection, Dispatch, QueueHandle,
};

use super::{connection, registry};

#[derive(Default)]
struct OutputProbe {
    names: Vec<(u32, String)>,
    /// Current mode size per global, before the output transform.
    modes: std::collections::BTreeMap<u32, (i32, i32)>,
    /// Whether the output transform rotates by 90 or 270 degrees.
    rotated: std::collections::BTreeMap<u32, bool>,
}

/// An output's on-screen size in device pixels, with rotation applied
/// (a 1920x1080 panel rotated 90 degrees reports 1080x1920).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub(crate) struct OutputGeometry {
    pub(crate) name: String,
    pub(crate) width: u32,
    pub(crate) height: u32,
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
        match event {
            wl_output::Event::Name { name } => state.names.push((*global_name, name)),
            wl_output::Event::Mode { flags, width, height, .. } => {
                let current = matches!(flags, wayland_client::WEnum::Value(f) if f.contains(wl_output::Mode::Current));
                if current {
                    state.modes.insert(*global_name, (width, height));
                }
            }
            wl_output::Event::Geometry { transform, .. } => {
                let rotated = matches!(
                    transform,
                    wayland_client::WEnum::Value(
                        wl_output::Transform::_90
                            | wl_output::Transform::_270
                            | wl_output::Transform::Flipped90
                            | wl_output::Transform::Flipped270
                    )
                );
                state.rotated.insert(*global_name, rotated);
            }
            _ => {}
        }
    }
}

impl Dispatch<wl_registry::WlRegistry, wayland_client::globals::GlobalListContents>
    for OutputProbe
{
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
    Ok(probe_outputs()?.names.into_iter().map(|(_, name)| name).collect())
}

/// Names and rotated on-screen sizes of every output, for UI hints such as the
/// wallpaper aspect-ratio warning.
pub(crate) fn list_output_geometry() -> Result<Vec<OutputGeometry>> {
    let probe = probe_outputs()?;
    Ok(probe
        .names
        .iter()
        .map(|(global, name)| {
            let (width, height) = probe.modes.get(global).copied().unwrap_or((0, 0));
            let (width, height) = if probe.rotated.get(global).copied().unwrap_or(false) {
                (height, width)
            } else {
                (width, height)
            };
            OutputGeometry {
                name: name.clone(),
                width: width.max(0) as u32,
                height: height.max(0) as u32,
            }
        })
        .collect())
}

fn probe_outputs() -> Result<OutputProbe> {
    let conn = connection::connect_to_env()?;
    let (globals, mut queue) = registry::init_registry::<OutputProbe>(&conn)?;
    let qh = queue.handle();
    let mut probe = OutputProbe::default();

    for global in
        globals.contents().clone_list().into_iter().filter(|global| global.interface == "wl_output")
    {
        if global.version < 4 {
            return Err(anyhow!(
                "wl_output global {} does not support stable output names",
                global.name
            ));
        }
        globals.registry().bind::<WlOutput, _, _>(global.name, 4, &qh, global.name);
    }

    queue.roundtrip(&mut probe).context("failed to read Wayland output names")?;
    probe.names.sort_unstable_by_key(|(global_name, _)| *global_name);
    if probe.names.is_empty() {
        return Err(anyhow!("compositor did not expose any named wl_output"));
    }
    Ok(probe)
}
