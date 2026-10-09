use std::collections::BTreeMap;

use we_core::{
    config::{OutputBinding, SceneConfig, SceneDay, ScenePower},
    profile::apply_profile_to_outputs,
    scenes::matching_scene,
};

use crate::{config::Config, runtime::power::PowerState};

#[derive(Clone, Copy, Debug)]
pub(crate) struct LocalTime {
    pub(crate) minute: u16,
    /// C localtime convention: Sunday = 0.
    pub(crate) weekday: u8,
}

impl LocalTime {
    pub(crate) fn now() -> Option<Self> {
        // The clock is consulted only by the policy scheduler, never on the render path.
        let mut seconds: libc::time_t = 0;
        let mut tm: libc::tm = unsafe { std::mem::zeroed() };
        if unsafe { libc::time(&mut seconds) } < 0
            || unsafe { libc::localtime_r(&seconds, &mut tm) }.is_null()
        {
            return None;
        }
        Some(Self { minute: tm.tm_hour as u16 * 60 + tm.tm_min as u16, weekday: tm.tm_wday as u8 })
    }
}

pub(crate) fn selected_profile<'a>(
    scenes: &'a SceneConfig,
    time: LocalTime,
    power: PowerState,
    connected_outputs: &[String],
) -> Option<&'a str> {
    let weekday = match time.weekday {
        0 => SceneDay::Sun,
        1 => SceneDay::Mon,
        2 => SceneDay::Tue,
        3 => SceneDay::Wed,
        4 => SceneDay::Thu,
        5 => SceneDay::Fri,
        6 => SceneDay::Sat,
        _ => return None,
    };
    let power = match power {
        PowerState::Ac => Some(ScenePower::Ac),
        PowerState::Battery => Some(ScenePower::Battery),
        PowerState::Unknown => None,
    };
    matching_scene(scenes, time.minute, weekday, power, connected_outputs)
        .map(|(_, rule)| rule.profile.as_str())
}

/// Runtime-only overlay over the persisted output bindings. Manual changes always win until
/// the selected rule changes. The overlay never writes the user's configuration file.
#[derive(Clone, Default)]
pub(crate) struct SceneRuntime {
    matched: Option<String>,
    active: Option<String>,
    baseline: Option<BTreeMap<String, OutputBinding>>,
    applied: Option<BTreeMap<String, OutputBinding>>,
    manual_override: bool,
    last_error: Option<String>,
}

impl SceneRuntime {
    /// A user explicitly applied a configuration or output profile. Discard ownership of the
    /// automatic overlay even when the selected bindings happen to be identical.
    pub(crate) fn manual_override(&mut self) {
        if self.matched.is_some() {
            self.manual_override = true;
            self.active = None;
            self.applied = None;
            self.baseline = None;
            self.last_error = None;
        }
    }

    pub(crate) fn reconcile<F>(
        &mut self,
        config: &Config,
        selected: Option<&str>,
        source_available: F,
    ) -> Result<Option<Config>, String>
    where
        F: Fn(&str) -> bool,
    {
        let selected = selected.map(str::to_string);
        let owned = self.applied.as_ref().is_some_and(|outputs| *outputs == config.outputs);
        if self.applied.is_some() && !owned {
            self.manual_override = true;
            self.active = None;
            self.applied = None;
            self.baseline = None;
        }
        if selected == self.matched {
            if selected.is_none() {
                self.last_error = None;
            }
            return Ok(None);
        }
        if selected.is_none() {
            let restore = if owned { self.baseline.take() } else { None };
            self.matched = None;
            self.active = None;
            self.applied = None;
            self.baseline = None;
            self.last_error = None;
            self.manual_override = false;
            return Ok(restore.filter(|outputs| *outputs != config.outputs).map(|outputs| {
                let mut next = config.clone();
                next.outputs = outputs;
                next
            }));
        }

        let profile = selected.as_deref().expect("selected profile");
        let mut next = config.clone();
        if let Err(error) = apply_profile_to_outputs(
            &config.profiles,
            &profile,
            &config.playlists,
            &mut next.outputs,
            source_available,
        ) {
            // Keep the previous match so a missing source/profile can recover without a
            // clock or power transition. Report a repeated identical error only once.
            if self.last_error.as_deref() == Some(&error) {
                return Ok(None);
            }
            self.last_error = Some(error.clone());
            return Err(error);
        }
        self.active = selected.clone();
        self.matched = selected;
        self.last_error = None;
        self.manual_override = false;
        if self.baseline.is_none() {
            self.baseline = Some(config.outputs.clone());
        }
        self.applied = Some(next.outputs.clone());
        Ok((next.outputs != config.outputs).then_some(next))
    }

    pub(crate) fn render_status_toml(&self) -> String {
        let string = |value: Option<&str>| {
            toml::Value::String(value.unwrap_or_default().to_string()).to_string()
        };
        format!(
            "[scene_runtime]\nmatched_profile = {}\nactive_profile = {}\nmanual_override = {}\nlast_error = {}\n",
            string(self.matched.as_deref()),
            string(self.active.as_deref()),
            self.manual_override,
            string(self.last_error.as_deref()),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{selected_profile, LocalTime, SceneRuntime};
    use crate::config::Config;
    use crate::runtime::power::PowerState;
    use we_core::config::{OutputBinding, SceneConfig, SceneDay, ScenePower, SceneRule};
    use we_core::profile::OutputProfile;

    fn rule(profile: &str, start: &str, end: &str) -> SceneRule {
        SceneRule {
            profile: profile.to_string(),
            start: Some(start.to_string()),
            end: Some(end.to_string()),
            days: vec![],
            power: None,
            outputs: vec![],
        }
    }

    #[test]
    fn overnight_window_uses_start_day_and_respects_end_boundary() {
        let mut night = rule("Night", "22:00", "06:00");
        night.days = vec![SceneDay::Mon];
        let config = SceneConfig { rules: vec![night] };
        assert_eq!(
            selected_profile(
                &config,
                LocalTime { minute: 23 * 60, weekday: 1 },
                PowerState::Unknown,
                &[]
            ),
            Some("Night")
        );
        assert_eq!(
            selected_profile(
                &config,
                LocalTime { minute: 5 * 60, weekday: 2 },
                PowerState::Unknown,
                &[]
            ),
            Some("Night")
        );
        assert_eq!(
            selected_profile(
                &config,
                LocalTime { minute: 6 * 60, weekday: 2 },
                PowerState::Unknown,
                &[]
            ),
            None
        );
        assert_eq!(
            selected_profile(
                &config,
                LocalTime { minute: 5 * 60, weekday: 1 },
                PowerState::Unknown,
                &[]
            ),
            None
        );
    }

    #[test]
    fn selection_requires_power_outputs_and_uses_declared_priority() {
        let mut battery = rule("Battery", "00:00", "00:00");
        battery.power = Some(ScenePower::Battery);
        battery.outputs = vec!["DP-1".to_string()];
        let scenes = SceneConfig { rules: vec![battery, rule("Default", "00:00", "00:00")] };
        let time = LocalTime { minute: 500, weekday: 5 };
        assert_eq!(
            selected_profile(&scenes, time, PowerState::Battery, &["DP-1".to_string()]),
            Some("Battery")
        );
        assert_eq!(selected_profile(&scenes, time, PowerState::Unknown, &[]), Some("Default"));
    }

    #[test]
    fn runtime_restores_baseline_without_persisting_and_respects_manual_change() {
        let mut config = Config::default();
        let original = OutputBinding::wallpaper("42", "/old");
        let scheduled = OutputBinding::wallpaper("77", "/new");
        config.outputs.insert("DP-1".into(), original.clone());
        config.profiles.definitions.insert(
            "Desk".into(),
            OutputProfile { outputs: [("DP-1".into(), scheduled.clone())].into() },
        );
        let mut runtime = SceneRuntime::default();
        let selected = runtime.reconcile(&config, Some("Desk"), |_| true).unwrap().unwrap();
        assert_eq!(selected.outputs["DP-1"], scheduled);
        let restored = runtime.reconcile(&selected, None, |_| true).unwrap().unwrap();
        assert_eq!(restored.outputs["DP-1"], original);

        let selected = runtime.reconcile(&restored, Some("Desk"), |_| true).unwrap().unwrap();
        let mut manual = selected.clone();
        manual.outputs.insert("DP-1".into(), OutputBinding::wallpaper("99", "/manual"));
        assert!(runtime.reconcile(&manual, Some("Desk"), |_| true).unwrap().is_none());
        assert!(runtime.reconcile(&manual, None, |_| true).unwrap().is_none());
        assert_eq!(manual.outputs["DP-1"].wallpaper_id.as_deref(), Some("99"));
    }

    #[test]
    fn explicit_manual_apply_of_identical_bindings_is_not_undone_at_rule_end() {
        let mut config = Config::default();
        let initial = OutputBinding::wallpaper("old", "/old");
        let selected = OutputBinding::wallpaper("desk", "/desk");
        config.outputs.insert("DP-1".into(), initial);
        config.profiles.definitions.insert(
            "Desk".into(),
            OutputProfile { outputs: [("DP-1".into(), selected.clone())].into() },
        );
        let mut runtime = SceneRuntime::default();
        let applied = runtime.reconcile(&config, Some("Desk"), |_| true).unwrap().unwrap();
        runtime.manual_override();
        assert!(runtime.reconcile(&applied, Some("Desk"), |_| true).unwrap().is_none());
        assert!(runtime.reconcile(&applied, None, |_| true).unwrap().is_none());
        assert_eq!(applied.outputs["DP-1"], selected);
    }

    #[test]
    fn missing_profile_recovers_without_a_scene_condition_transition() {
        let mut config = Config::default();
        let mut runtime = SceneRuntime::default();
        assert!(runtime.reconcile(&config, Some("Late"), |_| true).is_err());
        // Repeated unchanged errors do not spam the log, but the next poll still retries.
        assert!(runtime.reconcile(&config, Some("Late"), |_| true).unwrap().is_none());
        config.profiles.definitions.insert(
            "Late".into(),
            OutputProfile {
                outputs: [("DP-1".into(), OutputBinding::wallpaper("new", "/new"))].into(),
            },
        );
        let applied = runtime.reconcile(&config, Some("Late"), |_| true).unwrap().unwrap();
        assert_eq!(applied.outputs["DP-1"].wallpaper_id.as_deref(), Some("new"));
    }
}
