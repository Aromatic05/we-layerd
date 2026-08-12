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
    output_globals: BTreeMap<u32, u32>,
    output_names: BTreeMap<u32, String>,
    toplevels: BTreeMap<u32, ToplevelRecord>,
    finished: bool,
}

impl ForeignToplevelProbe {
    fn register_output_global(&mut self, global_name: u32, protocol_id: u32) {
        self.output_globals.insert(global_name, protocol_id);
    }

    fn record_output_name(&mut self, global_name: u32, protocol_id: u32, name: String) {
        if self.output_globals.get(&global_name) == Some(&protocol_id) {
            self.output_names.insert(protocol_id, name);
        }
    }

    fn remove_output_global(&mut self, global_name: u32) {
        let Some(protocol_id) = self.output_globals.remove(&global_name) else {
            return;
        };
        self.output_names.remove(&protocol_id);
        for toplevel in self.toplevels.values_mut() {
            toplevel.outputs.remove(&protocol_id);
        }
    }

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
        let mut state = ForeignToplevelProbe::default();
        for global in globals
            .contents()
            .clone_list()
            .into_iter()
            .filter(|global| global.interface == "wl_output" && global.version >= 4)
        {
            let output = globals.registry().bind::<wl_output::WlOutput, _, _>(
                global.name,
                4,
                &qh,
                global.name,
            );
            state.register_output_global(global.name, output.id().protocol_id());
        }
        let manager = globals
            .bind::<ZwlrForeignToplevelManagerV1, _, _>(&qh, 1..=3, ())
            .context("compositor does not expose wlr foreign-toplevel management")?;
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
            } else if result > 0 {
                if let Some(reason) = foreign_toplevel_poll_disconnect(pollfd.revents) {
                    drop(read_guard);
                    bail!("foreign-toplevel Wayland fd disconnected: {reason}");
                }
                if (pollfd.revents & libc::POLLIN) != 0 {
                    read_guard.read().context("failed to read foreign-toplevel events")?;
                    self.queue
                        .dispatch_pending(&mut self.state)
                        .context("failed to dispatch foreign-toplevel events")?;
                } else {
                    drop(read_guard);
                }
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
        state: &mut Self,
        proxy: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _data: &wayland_client::globals::GlobalListContents,
        _conn: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        match event {
            wl_registry::Event::Global { name, interface, version }
                if interface == "wl_output" && version >= 4 =>
            {
                let output = proxy.bind::<wl_output::WlOutput, _, _>(name, 4, qh, name);
                state.register_output_global(name, output.id().protocol_id());
            }
            wl_registry::Event::GlobalRemove { name } => state.remove_output_global(name),
            _ => {}
        }
    }
}

impl Dispatch<wl_output::WlOutput, u32> for ForeignToplevelProbe {
    fn event(
        state: &mut Self,
        proxy: &wl_output::WlOutput,
        event: wl_output::Event,
        global_name: &u32,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        if let wl_output::Event::Name { name } = event {
            state.record_output_name(*global_name, proxy.id().protocol_id(), name);
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

    // `toplevel` is opcode 0 and creates a handle object. wayland-client requires
    // explicit child user-data initialization for event-created protocol objects.
    wayland_client::event_created_child!(ForeignToplevelProbe, ZwlrForeignToplevelManagerV1, [
        0 => (ZwlrForeignToplevelHandleV1, ()),
    ]);
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

fn foreign_toplevel_poll_disconnect(revents: libc::c_short) -> Option<&'static str> {
    if (revents & libc::POLLNVAL) != 0 {
        Some("invalid fd")
    } else if (revents & libc::POLLERR) != 0 {
        Some("poll error")
    } else if (revents & libc::POLLHUP) != 0 {
        Some("hangup")
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::{
        decode_toplevel_states, foreign_toplevel_poll_disconnect, policies_for_outputs,
        ForeignToplevelProbe, PauseState, RuleAction, RuleSet, RuntimePolicy, ToplevelActivity,
        ToplevelRecord,
    };

    #[test]
    fn foreign_toplevel_state_array_decodes_protocol_values() {
        let raw = [0_u32, 2, 3].into_iter().flat_map(u32::to_ne_bytes).collect::<Vec<_>>();
        assert_eq!(decode_toplevel_states(&raw), (true, true, true));
    }

    #[test]
    fn output_hotplug_updates_names_and_removes_stale_toplevel_membership() {
        let mut probe = ForeignToplevelProbe::default();
        probe.register_output_global(41, 401);
        probe.record_output_name(41, 401, "DP-1".to_string());
        probe.toplevels.insert(
            7,
            ToplevelRecord {
                outputs: [401].into_iter().collect(),
                activated: true,
                ..ToplevelRecord::default()
            },
        );
        assert_eq!(probe.outputs(), vec!["DP-1".to_string()]);
        assert_eq!(probe.activities()[0].outputs, vec!["DP-1".to_string()]);

        probe.remove_output_global(41);

        assert!(probe.outputs().is_empty());
        assert!(probe.activities()[0].outputs.is_empty());

        probe.record_output_name(41, 401, "stale".to_string());
        assert!(probe.outputs().is_empty(), "events from a removed global must stay ignored");
    }

    #[test]
    fn poll_hangup_error_and_invalid_fd_are_disconnects() {
        for revents in [libc::POLLHUP, libc::POLLERR, libc::POLLNVAL] {
            assert!(foreign_toplevel_poll_disconnect(revents).is_some());
        }
        assert!(foreign_toplevel_poll_disconnect(libc::POLLIN).is_none());
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
