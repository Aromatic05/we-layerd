//! Pure scene condition matching shared by the runtime and GUI preview.
//! Only the daemon owns live application and switching of profiles.

use crate::config::{parse_clock_time, SceneConfig, SceneDay, ScenePower, SceneRule};

/// The first matching rule wins. Times use minutes since midnight in local wall time.
pub fn matching_scene<'a>(
    scenes: &'a SceneConfig,
    minute: u16,
    weekday: SceneDay,
    power: Option<ScenePower>,
    connected_outputs: &[String],
) -> Option<(usize, &'a SceneRule)> {
    scenes
        .rules
        .iter()
        .enumerate()
        .find(|(_, rule)| matches_rule(rule, minute, weekday, power, connected_outputs))
}

fn matches_rule(
    rule: &SceneRule,
    minute: u16,
    mut weekday: SceneDay,
    power: Option<ScenePower>,
    outputs: &[String],
) -> bool {
    if rule.power.is_some_and(|required| Some(required) != power) {
        return false;
    }
    if !rule.outputs.iter().all(|output| outputs.contains(output)) {
        return false;
    }
    match (&rule.start, &rule.end) {
        (None, None) => {}
        (Some(start), Some(end)) => {
            let (Some(start), Some(end)) = (parse_clock_time(start), parse_clock_time(end)) else {
                return false;
            };
            if start < end && !(start <= minute && minute < end) {
                return false;
            }
            if start > end {
                if minute >= end && minute < start {
                    return false;
                }
                if minute < end {
                    weekday = previous_day(weekday);
                }
            }
        }
        _ => return false,
    }
    rule.days.is_empty() || rule.days.contains(&weekday)
}

fn previous_day(day: SceneDay) -> SceneDay {
    match day {
        SceneDay::Mon => SceneDay::Sun,
        SceneDay::Tue => SceneDay::Mon,
        SceneDay::Wed => SceneDay::Tue,
        SceneDay::Thu => SceneDay::Wed,
        SceneDay::Fri => SceneDay::Thu,
        SceneDay::Sat => SceneDay::Fri,
        SceneDay::Sun => SceneDay::Sat,
    }
}

#[cfg(test)]
mod tests {
    use super::matching_scene;
    use crate::config::{SceneConfig, SceneDay, ScenePower, SceneRule};

    fn rule(profile: &str, start: &str, end: &str) -> SceneRule {
        SceneRule {
            profile: profile.into(),
            start: Some(start.into()),
            end: Some(end.into()),
            days: vec![],
            power: None,
            outputs: vec![],
        }
    }

    fn matching<'a>(scenes: &'a SceneConfig, minute: u16, day: SceneDay) -> Option<&'a str> {
        matching_scene(scenes, minute, day, None, &[]).map(|(_, rule)| rule.profile.as_str())
    }

    #[test]
    fn overnight_window_uses_start_day_and_end_is_exclusive() {
        let mut night = rule("Night", "22:00", "06:00");
        night.days = vec![SceneDay::Mon];
        let config = SceneConfig { rules: vec![night] };
        assert_eq!(matching(&config, 23 * 60, SceneDay::Mon), Some("Night"));
        assert_eq!(matching(&config, 5 * 60, SceneDay::Tue), Some("Night"));
        assert_eq!(matching(&config, 6 * 60, SceneDay::Tue), None);
        assert_eq!(matching(&config, 5 * 60, SceneDay::Mon), None);
    }

    #[test]
    fn priority_and_power_outputs_are_consistent() {
        let mut battery = rule("Battery", "00:00", "00:00");
        battery.power = Some(ScenePower::Battery);
        battery.outputs = vec!["DP-1".into()];
        let config = SceneConfig { rules: vec![battery, rule("Default", "00:00", "00:00")] };
        let time = 500;
        let active = matching_scene(
            &config,
            time,
            SceneDay::Fri,
            Some(ScenePower::Battery),
            &["DP-1".into()],
        );
        assert_eq!(active.map(|(index, _)| index), Some(0));
        assert_eq!(matching(&config, time, SceneDay::Fri), Some("Default"));
    }

    #[test]
    fn invalid_partial_windows_never_match() {
        let config = SceneConfig {
            rules: vec![SceneRule {
                profile: "Broken".into(),
                start: Some("12:00".into()),
                end: None,
                days: vec![],
                power: None,
                outputs: vec![],
            }],
        };
        assert!(matching_scene(&config, 720, SceneDay::Mon, None, &[]).is_none());
    }
}
