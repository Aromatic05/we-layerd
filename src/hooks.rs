use std::{
    process::{Command, Stdio},
    thread,
};

use tracing::{info, warn};
use we_core::{config::HookCommand, install_layout::expand_tilde};

use crate::{backend::traits::BackendKind, runtime::status::RuntimeStatusSnapshot};

#[derive(Debug, Default)]
pub(crate) struct WallpaperAppliedTrigger {
    fired: bool,
}

impl WallpaperAppliedTrigger {
    pub(crate) fn observe(&mut self, snapshot: &RuntimeStatusSnapshot) -> bool {
        if self.fired || snapshot.frame_stats.presented == 0 {
            return false;
        }

        self.fired = true;
        true
    }
}

pub(crate) struct WallpaperAppliedContext<'a> {
    pub(crate) source: &'a str,
    pub(crate) backend: BackendKind,
    pub(crate) generation: u64,
}

pub(crate) fn spawn_wallpaper_applied(
    hook: Option<&HookCommand>,
    context: WallpaperAppliedContext<'_>,
) {
    let Some(hook) = hook else {
        return;
    };
    if hook.command.trim().is_empty() {
        warn!("wallpaper_applied hook command is empty; skipping");
        return;
    }

    let program = expand_tilde(hook.command.trim());
    let source = expand_tilde(context.source).display().to_string();
    let mut command = Command::new(&program);
    command
        .args(&hook.args)
        .stdin(Stdio::null())
        .env("WE_LAYERD_EVENT", "wallpaper_applied")
        .env("WE_LAYERD_SOURCE", &source)
        .env("WE_LAYERD_BACKEND", context.backend.as_config_str())
        .env("WE_LAYERD_GENERATION", context.generation.to_string());

    let child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            warn!(
                command = %hook.command,
                %error,
                "failed to start wallpaper_applied hook"
            );
            return;
        }
    };

    let command_name = hook.command.clone();
    let _waiter = thread::spawn(move || {
        let mut child = child;
        match child.wait() {
            Ok(status) if status.success() => {
                info!(command = %command_name, "wallpaper_applied hook completed");
            }
            Ok(status) => {
                warn!(
                    command = %command_name,
                    exit_status = %status,
                    "wallpaper_applied hook failed"
                );
            }
            Err(error) => {
                warn!(
                    command = %command_name,
                    %error,
                    "failed to wait for wallpaper_applied hook"
                );
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use crate::runtime::status::RuntimeStatusSnapshot;

    use super::WallpaperAppliedTrigger;

    #[test]
    fn wallpaper_applied_fires_once_after_the_first_presented_frame() {
        let mut trigger = WallpaperAppliedTrigger::default();
        let mut snapshot = RuntimeStatusSnapshot::default();

        assert!(!trigger.observe(&snapshot));

        snapshot.frame_stats.presented = 1;
        assert!(trigger.observe(&snapshot));

        snapshot.frame_stats.presented = 2;
        assert!(!trigger.observe(&snapshot));
    }
}
