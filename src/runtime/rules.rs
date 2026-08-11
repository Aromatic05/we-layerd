use std::{
    collections::{BTreeMap, BTreeSet},
    os::fd::AsRawFd,
    time::Duration,
};

use anyhow::{bail, Context, Result};
use wayland_client::{
    protocol::{wl_output, wl_registry},
    Connection, Dispatch, Proxy, QueueHandle,
};
use wayland_protocols_wlr::foreign_toplevel::v1::client::{
    zwlr_foreign_toplevel_handle_v1::{self, ZwlrForeignToplevelHandleV1},
    zwlr_foreign_toplevel_manager_v1::{self, ZwlrForeignToplevelManagerV1},
};
use we_core::config::{RuntimeRuleAction, RuntimeRulesConfig};

use crate::backend::wayland_common::{connection, registry};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum RuleAction {
    #[default]
    Keep,
    Mute,
    Pause,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct RuleSet {
    pub(crate) focused: RuleAction,
    pub(crate) maximized: RuleAction,
    pub(crate) fullscreen: RuleAction,
}

impl RuleSet {
    pub(crate) fn is_keep(self) -> bool {
        self == Self::default()
    }
}

impl From<RuntimeRulesConfig> for RuleSet {
    fn from(value: RuntimeRulesConfig) -> Self {
        Self {
            focused: value.focused.into(),
            maximized: value.maximized.into(),
            fullscreen: value.fullscreen.into(),
        }
    }
}

impl From<RuntimeRuleAction> for RuleAction {
    fn from(value: RuntimeRuleAction) -> Self {
        match value {
            RuntimeRuleAction::Keep => Self::Keep,
            RuntimeRuleAction::Mute => Self::Mute,
            RuntimeRuleAction::Pause => Self::Pause,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ToplevelActivity {
    pub(crate) outputs: Vec<String>,
    pub(crate) activated: bool,
    pub(crate) maximized: bool,
    pub(crate) fullscreen: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct RuntimePolicy {
    pub(crate) pause: bool,
    pub(crate) mute: bool,
}

pub(crate) fn policies_for_outputs(
    rules: RuleSet,
    outputs: impl IntoIterator<Item = String>,
    toplevels: &[ToplevelActivity],
) -> BTreeMap<String, RuntimePolicy> {
    let mut policies = outputs
        .into_iter()
        .map(|output| (output, RuntimePolicy::default()))
        .collect::<BTreeMap<_, _>>();

    for toplevel in toplevels {
        for output in &toplevel.outputs {
            let Some(policy) = policies.get_mut(output) else {
                continue;
            };
            if toplevel.activated {
                apply_rule_action(policy, rules.focused);
            }
            if toplevel.maximized {
                apply_rule_action(policy, rules.maximized);
            }
            if toplevel.fullscreen {
                apply_rule_action(policy, rules.fullscreen);
            }
        }
    }
    policies
}

fn apply_rule_action(policy: &mut RuntimePolicy, action: RuleAction) {
    match action {
        RuleAction::Keep => {}
        RuleAction::Mute => policy.mute = true,
        RuleAction::Pause => policy.pause = true,
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct PauseState {
    manual: bool,
    rule: bool,
}

impl PauseState {
    pub(crate) fn set_manual(&mut self, paused: bool) {
        self.manual = paused;
    }

    pub(crate) fn set_rule(&mut self, paused: bool) {
        self.rule = paused;
    }

    pub(crate) fn effective(self) -> bool {
        self.manual || self.rule
    }
}

#[derive(Debug, Clone, Default)]
struct ToplevelRecord {
    outputs: BTreeSet<u32>,
    activated: bool,
    maximized: bool,
    fullscreen: bool,
}

#[derive(Default)]
struct ForeignToplevelProbe {
    output_names: BTreeMap<u32, String>,
    toplevels: BTreeMap<u32, ToplevelRecord>,
    finished: bool,
}

impl ForeignToplevelProbe {
    fn activities(&self) -> Vec<ToplevelActivity> {
        self.toplevels
            .values()
            .map(|toplevel| ToplevelActivity {
                outputs: toplevel
                    .outputs
                    .iter()
                    .filter_map(|id| self.output_names.get(id).cloned())
                    .collect(),
                activated: toplevel.activated,
                maximized: toplevel.maximized,
                fullscreen: toplevel.fullscreen,
            })
            .collect()
    }

    fn outputs(&self) -> Vec<String> {
        self.output_names.values().cloned().collect()
    }
}

pub(crate) struct ForeignToplevelMonitor {
    _connection: Connection,
    queue: wayland_client::EventQueue<ForeignToplevelProbe>,
    state: ForeignToplevelProbe,
    _manager: ZwlrForeignToplevelManagerV1,
}

impl ForeignToplevelMonitor {
    pub(crate) fn connect() -> Result<Self> {
        let connection = connection::connect_to_env()?;
        let (globals, mut queue) = registry::init_registry::<ForeignToplevelProbe>(&connection)?;
        let qh = queue.handle();
        for global in globals
            .contents()
            .clone_list()
            .into_iter()
            .filter(|global| global.interface == "wl_output" && global.version >= 4)
        {
            globals.registry().bind::<wl_output::WlOutput, _, _>(global.name, 4, &qh, ());
        }
        let manager = globals
            .bind::<ZwlrForeignToplevelManagerV1, _, _>(&qh, 1..=3, ())
            .context("compositor does not expose wlr foreign-toplevel management")?;
        let mut state = ForeignToplevelProbe::default();
        queue.roundtrip(&mut state).context("failed to read initial foreign-toplevel state")?;
        queue.roundtrip(&mut state).context("failed to finish initial foreign-toplevel state")?;
        Ok(Self { _connection: connection, queue, state, _manager: manager })
    }

    pub(crate) fn poll(
        &mut self,
        timeout: Duration,
    ) -> Result<(Vec<String>, Vec<ToplevelActivity>)> {
        self.queue
            .dispatch_pending(&mut self.state)
            .context("failed to dispatch pending foreign-toplevel events")?;
        if self.state.finished {
            bail!("foreign-toplevel manager finished");
        }
        if let Some(read_guard) = self.queue.prepare_read() {
            let fd = read_guard.connection_fd().as_raw_fd();
            let mut pollfd = libc::pollfd { fd, events: libc::POLLIN, revents: 0 };
            let timeout_ms = timeout.as_millis().min(i32::MAX as u128) as i32;
            let result = unsafe { libc::poll(&mut pollfd, 1, timeout_ms) };
            if result < 0 {
                let error = std::io::Error::last_os_error();
                drop(read_guard);
                if error.kind() != std::io::ErrorKind::Interrupted {
                    return Err(error).context("failed to poll foreign-toplevel Wayland fd");
                }
            } else if result > 0 && (pollfd.revents & libc::POLLIN) != 0 {
                read_guard.read().context("failed to read foreign-toplevel events")?;
                self.queue
                    .dispatch_pending(&mut self.state)
                    .context("failed to dispatch foreign-toplevel events")?;
            } else {
                drop(read_guard);
            }
        }
        Ok((self.state.outputs(), self.state.activities()))
    }
}

impl Dispatch<wl_registry::WlRegistry, wayland_client::globals::GlobalListContents>
    for ForeignToplevelProbe
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

impl Dispatch<wl_output::WlOutput, ()> for ForeignToplevelProbe {
    fn event(
        state: &mut Self,
        proxy: &wl_output::WlOutput,
        event: wl_output::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        if let wl_output::Event::Name { name } = event {
            state.output_names.insert(proxy.id().protocol_id(), name);
        }
    }
}

impl Dispatch<ZwlrForeignToplevelManagerV1, ()> for ForeignToplevelProbe {
    fn event(
        state: &mut Self,
        _proxy: &ZwlrForeignToplevelManagerV1,
        event: zwlr_foreign_toplevel_manager_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        match event {
            zwlr_foreign_toplevel_manager_v1::Event::Toplevel { toplevel } => {
                state.toplevels.entry(toplevel.id().protocol_id()).or_default();
            }
            zwlr_foreign_toplevel_manager_v1::Event::Finished => state.finished = true,
            _ => {}
        }
    }
}

impl Dispatch<ZwlrForeignToplevelHandleV1, ()> for ForeignToplevelProbe {
    fn event(
        state: &mut Self,
        proxy: &ZwlrForeignToplevelHandleV1,
        event: zwlr_foreign_toplevel_handle_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        let id = proxy.id().protocol_id();
        let record = state.toplevels.entry(id).or_default();
        match event {
            zwlr_foreign_toplevel_handle_v1::Event::OutputEnter { output } => {
                record.outputs.insert(output.id().protocol_id());
            }
            zwlr_foreign_toplevel_handle_v1::Event::OutputLeave { output } => {
                record.outputs.remove(&output.id().protocol_id());
            }
            zwlr_foreign_toplevel_handle_v1::Event::State { state: raw } => {
                let (maximized, activated, fullscreen) = decode_toplevel_states(&raw);
                record.maximized = maximized;
                record.activated = activated;
                record.fullscreen = fullscreen;
            }
            zwlr_foreign_toplevel_handle_v1::Event::Closed => {
                state.toplevels.remove(&id);
            }
            _ => {}
        }
    }
}

fn decode_toplevel_states(raw: &[u8]) -> (bool, bool, bool) {
    let mut maximized = false;
    let mut activated = false;
    let mut fullscreen = false;
    for value in raw
        .chunks_exact(4)
        .map(|bytes| u32::from_ne_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    {
        match value {
            0 => maximized = true,
            2 => activated = true,
            3 => fullscreen = true,
            _ => {}
        }
    }
    (maximized, activated, fullscreen)
}

#[cfg(test)]
mod tests {
    use super::{
        decode_toplevel_states, policies_for_outputs, PauseState, RuleAction, RuleSet,
        RuntimePolicy, ToplevelActivity,
    };

    #[test]
    fn foreign_toplevel_state_array_decodes_protocol_values() {
        let raw = [0_u32, 2, 3].into_iter().flat_map(u32::to_ne_bytes).collect::<Vec<_>>();
        assert_eq!(decode_toplevel_states(&raw), (true, true, true));
    }

    #[test]
    fn manual_pause_survives_a_temporary_rule_pause() {
        let mut pause = PauseState::default();
        pause.set_manual(true);
        pause.set_rule(true);
        assert!(pause.effective());

        pause.set_rule(false);
        assert!(pause.effective(), "clearing a rule must not resume a manually paused wallpaper");

        pause.set_manual(false);
        assert!(!pause.effective());
    }

    #[test]
    fn pause_outranks_mute_and_rules_are_scoped_to_the_toplevel_output() {
        let policies = policies_for_outputs(
            RuleSet {
                focused: RuleAction::Mute,
                maximized: RuleAction::Mute,
                fullscreen: RuleAction::Pause,
            },
            ["DP-1".to_string(), "HDMI-A-1".to_string()],
            &[
                ToplevelActivity {
                    outputs: vec!["DP-1".to_string()],
                    activated: true,
                    maximized: false,
                    fullscreen: false,
                },
                ToplevelActivity {
                    outputs: vec!["HDMI-A-1".to_string()],
                    activated: false,
                    maximized: true,
                    fullscreen: true,
                },
            ],
        );

        assert_eq!(policies["DP-1"], RuntimePolicy { pause: false, mute: true });
        assert_eq!(policies["HDMI-A-1"], RuntimePolicy { pause: true, mute: true });
    }
}
