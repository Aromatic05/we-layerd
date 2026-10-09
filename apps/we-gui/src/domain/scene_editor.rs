use std::collections::BTreeSet;

use we_core::{
    config::{
        parse_clock_time, AdaptiveConfig, LaunchSettings, SceneConfig, SceneDay, ScenePower,
        SceneRule,
    },
    profile::ProfileConfig,
    scenes::{matching_scene, next_scene_transition},
};

use super::playlist_editor::MoveDirection;

/// Draft-only state: editing does not alter persistent settings or the active daemon.
#[derive(Debug, Clone)]
pub(crate) struct SceneEditor {
    pub rules: SceneConfig,
    pub adaptive: AdaptiveConfig,
    pub battery_fps: String,
    /// Wayland output name being entered even when that monitor is disconnected.
    pub output_name_input: String,
    pub selected: Option<usize>,
    pub preview_time: String,
    pub preview_day: SceneDay,
    pub preview_power: Option<ScenePower>,
    pub preview_outputs: BTreeSet<String>,
    pub error: Option<String>,
    pub notice: Option<String>,
}

impl SceneEditor {
    pub(crate) fn new(settings: &LaunchSettings) -> Self {
        Self {
            rules: settings.scenes.clone(),
            adaptive: settings.adaptive,
            battery_fps: settings
                .adaptive
                .on_battery_fps
                .map(|n| n.to_string())
                .unwrap_or_default(),
            output_name_input: String::new(),
            selected: (!settings.scenes.rules.is_empty()).then_some(0),
            preview_time: "09:00".into(),
            preview_day: SceneDay::Mon,
            preview_power: Some(ScenePower::Ac),
            preview_outputs: BTreeSet::new(),
            error: None,
            notice: None,
        }
    }

    pub(crate) fn reset(&mut self, settings: &LaunchSettings) {
        let previous = self.selected;
        self.rules = settings.scenes.clone();
        self.adaptive = settings.adaptive;
        self.battery_fps =
            settings.adaptive.on_battery_fps.map(|n| n.to_string()).unwrap_or_default();
        self.output_name_input.clear();
        self.selected = previous
            .filter(|i| *i < self.rules.rules.len())
            .or_else(|| (!self.rules.rules.is_empty()).then_some(0));
        self.error = None;
        self.notice = None;
    }

    pub(crate) fn selected_rule_mut(&mut self) -> Option<&mut SceneRule> {
        self.selected.and_then(|i| self.rules.rules.get_mut(i))
    }

    /// Add a disconnected or otherwise undiscovered output as an explicit scene condition.
    /// A repeat is a no-op; existing conditions never get duplicated.
    pub(crate) fn add_output_name(&mut self) -> bool {
        let name = self.output_name_input.trim().to_string();
        if name.is_empty() || name.chars().any(char::is_whitespace) {
            return false;
        }
        let Some(rule) = self.selected_rule_mut() else { return false };
        if !rule.outputs.contains(&name) {
            rule.outputs.push(name);
            rule.outputs.sort();
        }
        self.output_name_input.clear();
        true
    }

    pub(crate) fn references_profile(&self, name: &str) -> bool {
        self.rules.rules.iter().any(|rule| rule.profile == name)
    }

    pub(crate) fn rename_profile(&mut self, old: &str, new: &str) {
        for rule in &mut self.rules.rules {
            if rule.profile == old {
                rule.profile = new.to_string();
            }
        }
    }

    pub(crate) fn add(&mut self, profile: String) {
        self.rules.rules.push(SceneRule {
            profile,
            start: None,
            end: None,
            days: vec![],
            power: None,
            outputs: vec![],
        });
        self.selected = Some(self.rules.rules.len() - 1);
        self.error = None;
        self.notice = None;
    }

    pub(crate) fn remove_selected(&mut self) {
        let Some(index) = self.selected.filter(|i| *i < self.rules.rules.len()) else { return };
        self.rules.rules.remove(index);
        self.selected = (!self.rules.rules.is_empty())
            .then_some(index.min(self.rules.rules.len().saturating_sub(1)));
        self.error = None;
        self.notice = None;
    }

    pub(crate) fn move_selected(&mut self, direction: MoveDirection) {
        let Some(index) = self.selected else { return };
        let target = match direction {
            MoveDirection::Up if index > 0 => index - 1,
            MoveDirection::Down if index + 1 < self.rules.rules.len() => index + 1,
            _ => return,
        };
        self.rules.rules.swap(index, target);
        self.selected = Some(target);
        self.error = None;
        self.notice = None;
    }

    pub(crate) fn validate(&self, profiles: &ProfileConfig) -> Result<AdaptiveConfig, String> {
        self.rules.validate()?;
        for (i, rule) in self.rules.rules.iter().enumerate() {
            if !profiles.definitions.contains_key(&rule.profile) {
                return Err(format!(
                    "Rule {} references missing profile '{}'",
                    i + 1,
                    rule.profile
                ));
            }
        }
        let mut adaptive = self.adaptive;
        adaptive.on_battery_fps = if self.battery_fps.trim().is_empty() {
            None
        } else {
            let fps = self
                .battery_fps
                .trim()
                .parse::<u32>()
                .map_err(|_| "Battery FPS must be between 1 and 360".to_string())?;
            if !(1..=360).contains(&fps) {
                return Err("Battery FPS must be between 1 and 360".into());
            }
            Some(fps)
        };
        Ok(adaptive)
    }

    pub(crate) fn is_dirty(&self, settings: &LaunchSettings) -> bool {
        self.rules != settings.scenes
            || self.adaptive.on_battery != settings.adaptive.on_battery
            || self.battery_fps
                != settings.adaptive.on_battery_fps.map(|n| n.to_string()).unwrap_or_default()
    }

    pub(crate) fn preview(&self) -> Option<(usize, &str)> {
        let minute = parse_clock_time(&self.preview_time)?;
        let outputs = self.preview_outputs.iter().cloned().collect::<Vec<_>>();
        matching_scene(&self.rules, minute, self.preview_day, self.preview_power, &outputs)
            .map(|(i, rule)| (i, rule.profile.as_str()))
    }

    pub(crate) fn next_transition(&self) -> Option<(u8, u16, Option<usize>)> {
        let minute = parse_clock_time(&self.preview_time)?;
        let outputs = self.preview_outputs.iter().cloned().collect::<Vec<_>>();
        next_scene_transition(&self.rules, minute, self.preview_day, self.preview_power, &outputs)
    }
}

#[cfg(test)]
mod tests {
    use super::SceneEditor;
    use crate::domain::playlist_editor::MoveDirection;
    use we_core::{
        config::{LaunchSettings, SceneDay},
        profile::OutputProfile,
    };

    #[test]
    fn drafts_do_not_modify_saved_config_and_order_is_stable() {
        let mut settings = LaunchSettings::default();
        settings.profiles.definitions.insert("Day".into(), OutputProfile::default());
        settings.profiles.definitions.insert("Night".into(), OutputProfile::default());
        let mut editor = SceneEditor::new(&settings);
        editor.add("Day".into());
        editor.add("Night".into());
        editor.move_selected(MoveDirection::Up);
        assert_eq!(editor.rules.rules[0].profile, "Night");
        assert!(settings.scenes.is_empty());
        editor.remove_selected();
        editor.reset(&settings);
        assert!(!editor.is_dirty(&settings));
    }

    #[test]
    fn editor_validates_profile_battery_cap_and_preview() {
        let mut settings = LaunchSettings::default();
        let mut editor = SceneEditor::new(&settings);
        editor.add("Day".into());
        assert!(editor.validate(&settings.profiles).is_err());
        settings.profiles.definitions.insert("Day".into(), OutputProfile::default());
        editor.battery_fps = "0".into();
        assert!(editor.validate(&settings.profiles).is_err());
        editor.battery_fps = "24".into();
        assert_eq!(editor.validate(&settings.profiles).unwrap().on_battery_fps, Some(24));
        let rule = editor.selected_rule_mut().unwrap();
        rule.start = Some("08:00".into());
        rule.end = Some("17:00".into());
        rule.days = vec![SceneDay::Mon];
        assert_eq!(editor.preview(), Some((0, "Day")));
        editor.preview_time = "18:00".into();
        assert_eq!(editor.preview(), None);
    }

    #[test]
    fn rename_profile_rewrites_all_draft_rules_without_losing_editor_state() {
        let settings = LaunchSettings::default();
        let mut editor = SceneEditor::new(&settings);
        editor.add("Work".into());
        editor.add("Work".into());
        editor.add("Home".into());
        editor.rename_profile("Work", "Office");
        assert!(editor.references_profile("Office"));
        assert!(!editor.references_profile("Work"));
        assert_eq!(editor.rules.rules.iter().filter(|rule| rule.profile == "Office").count(), 2);
        assert_eq!(editor.selected, Some(2));
    }

    #[test]
    fn disconnected_output_can_be_added_without_duplicate_or_empty_conditions() {
        let mut editor = SceneEditor::new(&LaunchSettings::default());
        editor.add("Work".into());
        editor.output_name_input = "  DP-1  ".into();
        assert!(editor.add_output_name());
        assert_eq!(editor.rules.rules[0].outputs, ["DP-1"]);
        editor.output_name_input = "DP-1".into();
        assert!(editor.add_output_name());
        assert_eq!(editor.rules.rules[0].outputs, ["DP-1"]);
        editor.output_name_input = " HDMI A-1 ".into();
        assert!(!editor.add_output_name());
        assert_eq!(editor.rules.rules[0].outputs, ["DP-1"]);
    }
}
