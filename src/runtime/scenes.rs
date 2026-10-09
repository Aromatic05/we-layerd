use std::collections::BTreeMap;

use we_core::{
    config::{parse_clock_time, OutputBinding, SceneConfig, SceneDay, ScenePower, SceneRule},
    profile::apply_profile_to_outputs,
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

pub(crate) fn selected_profile(
    scenes: &SceneConfig,
    time: LocalTime,
    power: PowerState,
    connected_outputs: &[String],
) -> Option<&str> {
    scenes
        .rules
        .iter()
        .find(|rule| matches_rule(rule, time, power, connected_outputs))
        .map(|rule| rule.profile.as_str())
}

fn matches_rule(rule: &SceneRule, time: LocalTime, power: PowerState, outputs: &[String]) -> bool {
    if let Some(requested) = rule.power {
        if !matches!(
            (requested, power),
            (ScenePower::Ac, PowerState::Ac) | (ScenePower::Battery, PowerState::Battery)
        ) {
            return false;
        }
    }
    if !rule.outputs.iter().all(|output| outputs.contains(output)) {
        return false;
    }

    let mut weekday = time.weekday;
    if let (Some(start), Some(end)) = (&rule.start, &rule.end) {
        let (Some(start), Some(end)) = (parse_clock_time(start), parse_clock_time(end)) else {
            return false;
        };
        if start < end && !(start <= time.minute && time.minute < end) {
            return false;
        }
        if start > end {
            if time.minute >= end && time.minute < start {
                return false;
            }
            // After midnight the active rule still belongs to the preceding weekday.
            if time.minute < end {
                weekday = (weekday + 6) % 7;
            }
        }
    }

    let day = match weekday {
        0 => SceneDay::Sun,
        1 => SceneDay::Mon,
        2 => SceneDay::Tue,
        3 => SceneDay::Wed,
        4 => SceneDay::Thu,
        5 => SceneDay::Fri,
        6 => SceneDay::Sat,
        _ => return false,
    };
    rule.days.is_empty() || rule.days.contains(&day)
}

/// Runtime-only overlay over the persisted output bindings. Manual changes always win until
/// the selected rule changes. The overlay never writes the user's configuration file.
#[derive(Default)]
pub(crate) struct SceneRuntime {
    matched: Option<String>,
    active: Option<String>,
    baseline: Option<BTreeMap<String, OutputBinding>>,
    applied: Option<BTreeMap<String, OutputBinding>>,
    manual_override: bool,
    last_error: Option<String>,
}

impl SceneRuntime {
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
            return Ok(None);
        }

        self.matched = selected.clone();
        self.last_error = None;
        // A new condition transition is a new opportunity for automatic selection.
        self.manual_override = false;
        if selected.is_none() {
            let restore = if owned { self.baseline.take() } else { None };
            self.active = None;
            self.applied = None;
            self.baseline = None;
            return Ok(restore.filter(|outputs| *outputs != config.outputs).map(|outputs| {
                let mut next = config.clone();
                next.outputs = outputs;
                next
            }));
        }

        let profile = selected.expect("selected profile");
        let mut next = config.clone();
        if let Err(error) = apply_profile_to_outputs(
            &config.profiles,
            &profile,
            &config.playlists,
            &mut next.outputs,
            source_available,
        ) {
            self.last_error = Some(error.clone());
            return Err(error);
        }
        if self.baseline.is_none() {
            self.baseline = Some(config.outputs.clone());
        }
        self.applied = Some(next.outputs.clone());
        self.active = Some(profile);
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
}
