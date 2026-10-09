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
    if !scenes.enabled {
        return None;
    }
    scenes
        .rules
        .iter()
        .enumerate()
        .find(|(_, rule)| matches_rule(rule, minute, weekday, power, connected_outputs))
}

/// Find the next rule change in the coming week, assuming power and display connections
/// stay constant. Only rule boundaries and midnight need evaluation, not every minute.
pub fn next_scene_transition(
    scenes: &SceneConfig,
    minute: u16,
    weekday: SceneDay,
    power: Option<ScenePower>,
    connected_outputs: &[String],
) -> Option<(u8, u16, Option<usize>)> {
    if !scenes.enabled || minute >= 1440 {
        return None;
    }
    let days = [
        SceneDay::Mon,
        SceneDay::Tue,
        SceneDay::Wed,
        SceneDay::Thu,
        SceneDay::Fri,
        SceneDay::Sat,
        SceneDay::Sun,
    ];
    let start_day = days.iter().position(|day| *day == weekday)?;
    let current =
        matching_scene(scenes, minute, weekday, power, connected_outputs).map(|(index, _)| index);
    let mut boundaries = Vec::with_capacity(8 * (1 + scenes.rules.len() * 2));
    for offset in 0..=7_u32 {
        let midnight = offset * 1440;
        boundaries.push(midnight);
        for rule in &scenes.rules {
            for value in [&rule.start, &rule.end].into_iter().flatten() {
                if let Some(parsed) = parse_clock_time(value) {
                    boundaries.push(midnight + u32::from(parsed));
                }
            }
        }
    }
    boundaries.sort_unstable();
    boundaries.dedup();
    for candidate in boundaries {
        if candidate <= u32::from(minute) || candidate > u32::from(minute) + 7 * 1440 {
            continue;
        }
        let offset = (candidate / 1440) as usize;
        let next_minute = (candidate % 1440) as u16;
        let next_day = days[(start_day + offset) % days.len()];
        let selected = matching_scene(scenes, next_minute, next_day, power, connected_outputs)
            .map(|(index, _)| index);
        if selected != current {
            return Some((offset as u8, next_minute, selected));
        }
    }
    None
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
    use super::{matching_scene, next_scene_transition};
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
        let config = SceneConfig { enabled: true, rules: vec![night] };
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
        let config =
            SceneConfig { enabled: true, rules: vec![battery, rule("Default", "00:00", "00:00")] };
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
            enabled: true,
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

    #[test]
    fn next_transition_tracks_boundaries_and_weekday_rollover() {
        let mut day = rule("Work", "09:00", "17:00");
        day.days = vec![SceneDay::Mon];
        let scenes = SceneConfig { enabled: true, rules: vec![day] };
        assert_eq!(
            next_scene_transition(&scenes, 10 * 60, SceneDay::Mon, None, &[]),
            Some((0, 17 * 60, None))
        );
        assert_eq!(
            next_scene_transition(&scenes, 18 * 60, SceneDay::Mon, None, &[]),
            Some((7, 9 * 60, Some(0)))
        );
    }

    #[test]
    fn next_transition_considers_all_day_and_fallback_priority() {
        let mut work = rule("Work", "00:00", "00:00");
        work.days = vec![SceneDay::Mon];
        let scenes =
            SceneConfig { enabled: true, rules: vec![work, rule("Fallback", "00:00", "00:00")] };
        assert_eq!(
            next_scene_transition(&scenes, 22 * 60, SceneDay::Mon, None, &[]),
            Some((1, 0, Some(1)))
        );
        assert_eq!(
            next_scene_transition(&scenes, 22 * 60, SceneDay::Tue, None, &[]),
            Some((6, 0, Some(0)))
        );
        let never = SceneConfig { enabled: true, rules: vec![rule("All", "00:00", "00:00")] };
        assert_eq!(next_scene_transition(&never, 600, SceneDay::Mon, None, &[]), None);
    }

    #[test]
    fn paused_automation_preserves_rules_but_never_matches_or_forecasts() {
        let mut scenes =
            SceneConfig { enabled: false, rules: vec![rule("Desk", "00:00", "00:00")] };
        assert!(matching_scene(&scenes, 600, SceneDay::Mon, None, &[]).is_none());
        assert!(next_scene_transition(&scenes, 600, SceneDay::Mon, None, &[]).is_none());
        assert_eq!(scenes.rules[0].profile, "Desk");
        scenes.enabled = true;
        assert_eq!(matching(&scenes, 600, SceneDay::Mon), Some("Desk"));
    }
}
