use std::collections::BTreeMap;

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
    _rules: RuleSet,
    _outputs: impl IntoIterator<Item = String>,
    _toplevels: &[ToplevelActivity],
) -> BTreeMap<String, RuntimePolicy> {
    todo!("implemented after the behavior tests are established")
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct PauseState {
    manual: bool,
    rule: bool,
}

impl PauseState {
    pub(crate) fn set_manual(&mut self, _paused: bool) {
        todo!("implemented after the behavior tests are established")
    }

    pub(crate) fn set_rule(&mut self, _paused: bool) {
        todo!("implemented after the behavior tests are established")
    }

    pub(crate) fn effective(self) -> bool {
        todo!("implemented after the behavior tests are established")
    }
}

#[cfg(test)]
mod tests {
    use super::{
        policies_for_outputs, PauseState, RuleAction, RuleSet, RuntimePolicy, ToplevelActivity,
    };

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
