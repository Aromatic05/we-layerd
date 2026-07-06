use anyhow::Result;

use crate::{
    backend::traits::BackendContext,
    ipc::RuntimeLoopExit,
};

pub(crate) fn run(ctx: BackendContext<'_>) -> Result<RuntimeLoopExit> {
    let _version = super::dbus::ping_extension(&ctx.cfg.gnome.extension_dbus_name)?;
    Err(anyhow::anyhow!(
        "GNOME backend boundary is defined, but GNOME wallpaper runtime is not implemented yet"
    ))
}
