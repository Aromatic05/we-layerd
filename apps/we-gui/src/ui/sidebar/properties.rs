use iced::{
    widget::{button, checkbox, column, container, pick_list, row, slider, text, text_input},
    Alignment, Background, Border, Color, Element, Fill,
};
use serde_json::Value;
use std::hash::{Hash, Hasher};
use we_core::wallpaper::{
    properties::{UserProperty, UserPropertyKind, UserPropertySchema},
    settings::WallpaperSettings,
};

use crate::domain::{
    i18n::{Language, Text},
    property_search::{self, plain_text, PropertyFilter},
};

use super::detail::{self, DetailMessage};

#[derive(Debug, Clone, PartialEq)]
struct PropertyOption {
    label: String,
    value: Value,
}

impl std::fmt::Display for PropertyOption {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.label)
    }
}

pub(crate) fn toolbar<'a>(
    schema: &'a UserPropertySchema,
    settings: &'a WallpaperSettings,
    filter: &'a PropertyFilter,
    language: Language,
) -> Element<'a, DetailMessage> {
    let results = property_search::search(schema, settings, filter);
    let shown = if filter.show_internal {
        results.entries.len()
    } else {
        results.entries.iter().filter(|entry| property_search::is_editable(entry.property)).count()
    };
    column![
        row![
            crate::ui::search_input::clearable(
                text_input(language.text(Text::SearchProperties), &filter.query)
                    .id("detail.properties.search")
                    .on_input(DetailMessage::PropertySearchChanged)
                    .padding([10, 12])
                    .size(14)
                    .width(Fill)
                    .style(detail::md_text_input_style),
                DetailMessage::ClearPropertyFilters
            ),
            button(text(language.text(Text::ClearSearch)).size(12))
                .on_press_maybe(
                    (!filter.query.is_empty() || filter.modified_only)
                        .then_some(DetailMessage::ClearPropertyFilters)
                )
                .padding([10, 10])
                .style(detail::outlined_button_style),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
        row![
            text(format!("{shown} / {}", results.total)).size(12).style(text::secondary),
            container(
                checkbox(filter.modified_only)
                    .label(format!(
                        "{} ({})",
                        language.text(Text::ModifiedProperties),
                        results.modified
                    ))
                    .size(16)
                    .text_size(12)
                    .on_toggle(DetailMessage::ModifiedPropertiesToggled)
                    .style(detail::md_checkbox_style),
            )
            .width(Fill)
            .align_x(iced::alignment::Horizontal::Right),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
        checkbox(filter.show_internal)
            .label(language.text(Text::ShowInternalProperties))
            .size(16)
            .text_size(12)
            .on_toggle(DetailMessage::InternalPropertiesToggled)
            .style(detail::md_checkbox_style),
        row![
            text(language.text(Text::PropertiesSavedAutomatically))
                .size(12)
                .style(text::secondary)
                .width(Fill),
            button(text(language.text(Text::ResetProperties)).size(12))
                // Reset all also clears overrides currently hidden by author conditions.
                .on_press_maybe(
                    (!settings.user_properties.is_empty())
                        .then_some(DetailMessage::ResetProperties)
                )
                .padding([6, 10])
                .style(detail::outlined_button_style),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    ]
    .spacing(10)
    .into()
}

pub(crate) fn view<'a>(
    schema: &'a UserPropertySchema,
    settings: &'a WallpaperSettings,
    filter: &'a PropertyFilter,
    wallpaper_id: &'a str,
    language: Language,
) -> Element<'a, DetailMessage> {
    let results = property_search::search(schema, settings, filter);
    if results.total == 0 && results.entries.is_empty() {
        return empty_state(language.text(Text::NoUserProperties), None, language);
    }
    if results.entries.is_empty() {
        return empty_state(
            if filter.query.trim().is_empty() {
                language.text(Text::NoModifiedProperties)
            } else {
                language.text(Text::NoMatchingProperties)
            },
            Some(language.text(Text::PropertySearchHint)),
            language,
        );
    }
    let mut properties: iced::widget::keyed::Column<'a, (u64, bool), DetailMessage> =
        iced::widget::keyed::Column::new().spacing(8).width(Fill);
    let mut last_group: Option<String> = None;
    for matched in results.entries {
        if matched.group != last_group {
            if let Some(group) = &matched.group {
                properties = properties.push(
                    row_key(wallpaper_id, &matched.property.key, true),
                    crate::ui::state_key::keyed(
                        row_key(wallpaper_id, &matched.property.key, true).0,
                        container(text(group.clone()).size(15)).padding(iced::Padding {
                            top: 12.0,
                            bottom: 4.0,
                            left: 0.0,
                            right: 0.0,
                        }),
                    ),
                );
            }
            last_group = matched.group;
        }
        let property = matched.property;
        let label = if matched.decorative {
            language.text(Text::DecorativeProperty).to_owned()
        } else {
            matched.label
        };
        let mut header =
            row![text(label).size(14).width(Fill)].align_y(Alignment::Center).spacing(8);
        if matched.internal {
            header = header
                .push(text(language.text(Text::InternalProperty)).size(11).style(text::secondary));
        }
        if matched.modified {
            header = header
                .push(text(language.text(Text::ModifiedProperty)).size(11).style(text::secondary));
        }
        if property_search::is_editable(property) {
            header = header.push(
                button(text(language.text(Text::ResetProperty)).size(12))
                    .on_press_maybe(
                        matched
                            .modified
                            .then(|| DetailMessage::ResetProperty(property.key.clone())),
                    )
                    .padding([4, 7])
                    .style(detail::outlined_button_style),
            );
        }
        let mut content = column![header].spacing(8);
        if filter.show_internal {
            let key = if property.key.chars().count() > 48 {
                format!(
                    "{}…{}",
                    property.key.chars().take(18).collect::<String>(),
                    property
                        .key
                        .chars()
                        .rev()
                        .take(28)
                        .collect::<String>()
                        .chars()
                        .rev()
                        .collect::<String>()
                )
            } else {
                property.key.clone()
            };
            content = content.push(iced::widget::tooltip(
                text(key).size(11).style(text::secondary),
                container(text(&property.key).size(12))
                    .width(400)
                    .padding(8)
                    .style(detail::section_style),
                iced::widget::tooltip::Position::Top,
            ));
        }
        if matched.decorative {
            let current = settings.user_properties.get(&property.key).unwrap_or(&property.default);
            content = content.push(text(value_text(current)).size(12).style(text::secondary));
        } else {
            content = content.push(control(property, settings, language));
        }
        properties = properties.push(
            row_key(wallpaper_id, &property.key, false),
            crate::ui::state_key::keyed(
                row_key(wallpaper_id, &property.key, false).0,
                container(content)
                    .id(format!("detail.property.{}", property.key))
                    .width(Fill)
                    .padding(12)
                    .style(detail::section_style),
            ),
        );
    }
    properties.into()
}

fn row_key(wallpaper: &str, property: &str, heading: bool) -> (u64, bool) {
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    (wallpaper, property, heading).hash(&mut hash);
    (hash.finish(), heading)
}

fn empty_state<'a>(
    message: &'a str,
    hint: Option<&'a str>,
    language: Language,
) -> Element<'a, DetailMessage> {
    let mut content = column![text(message).size(16)].spacing(12);
    if let Some(hint) = hint {
        content = content.push(text(hint).size(13).style(text::secondary));
        content = content.push(
            button(text(language.text(Text::ClearPropertyFilters)).size(13))
                .on_press(DetailMessage::ClearPropertyFilters)
                .padding([8, 12])
                .style(detail::outlined_button_style),
        );
    }
    container(content).padding([24, 12]).width(Fill).into()
}

fn control<'a>(
    property: &'a UserProperty,
    settings: &'a WallpaperSettings,
    language: Language,
) -> Element<'a, DetailMessage> {
    let current = settings.user_properties.get(&property.key).unwrap_or(&property.default);
    match property.kind {
        UserPropertyKind::Boolean => container(
            checkbox(current.as_bool().unwrap_or(false))
                .label(language.text(Text::Enabled))
                .size(18)
                .text_size(13)
                .on_toggle({
                    let key = property.key.clone();
                    move |value| DetailMessage::PropertyChanged {
                        key: key.clone(),
                        value: Value::Bool(value),
                    }
                })
                .style(detail::md_checkbox_style),
        )
        .id(format!("detail.property.{}.enabled", property.key))
        .into(),
        UserPropertyKind::Slider => {
            let minimum = property.minimum.unwrap_or(0.0) as f32;
            let maximum = property.maximum.unwrap_or(1.0) as f32;
            let value = current.as_f64().unwrap_or(minimum as f64) as f32;
            let precision = property.precision.unwrap_or(0).min(6) as usize;
            let step = 10.0_f32.powi(-(property.precision.unwrap_or(0).min(38) as i32));
            row![
                container(
                    slider(
                        minimum..=maximum.max(minimum),
                        value.clamp(minimum, maximum.max(minimum)),
                        {
                            let key = property.key.clone();
                            move |value| DetailMessage::PropertyChanged {
                                key: key.clone(),
                                value: serde_json::json!(value),
                            }
                        }
                    )
                    .step(step)
                    .style(detail::md_slider_style),
                )
                .id(format!("detail.property.{}.slider", property.key))
                .width(Fill),
                text(format!("{value:.precision$}"))
                    .size(13)
                    .width(54)
                    .align_x(iced::alignment::Horizontal::Right),
            ]
            .spacing(12)
            .align_y(Alignment::Center)
            .into()
        }
        UserPropertyKind::Combo => {
            let choices = property
                .options
                .iter()
                .map(|option| PropertyOption {
                    label: plain_text(&option.label),
                    value: option.value.clone(),
                })
                .collect::<Vec<_>>();
            let selected = choices.iter().find(|choice| choice.value == *current).cloned();
            container(
                pick_list(choices, selected, {
                    let key = property.key.clone();
                    move |choice| DetailMessage::PropertyChanged {
                        key: key.clone(),
                        value: choice.value,
                    }
                })
                .text_size(14)
                .padding([9, 10])
                .width(Fill)
                .style(detail::md_pick_list_style)
                .menu_style(detail::md_menu_style),
            )
            .id(format!("detail.property.{}.choice", property.key))
            .into()
        }
        UserPropertyKind::Color => {
            let color = color_preview(current);
            row![
                container(text("")).width(30).height(30).style(move |_| container::Style {
                    background: Some(Background::Color(color)),
                    border: Border {
                        radius: 5.0.into(),
                        width: 1.0,
                        color: Color::from_rgb8(100, 102, 108)
                    },
                    ..Default::default()
                }),
                text_control(property, current, language.text(Text::Value)),
            ]
            .spacing(10)
            .align_y(Alignment::Center)
            .into()
        }
        UserPropertyKind::Text => text_control(property, current, language.text(Text::Value)),
        UserPropertyKind::File | UserPropertyKind::Directory => row![
            text_control(property, current, language.text(Text::Path)),
            container(
                button(text(language.text(Text::Browse)).size(13))
                    .on_press(DetailMessage::PickPath {
                        key: property.key.clone(),
                        directory: matches!(property.kind, UserPropertyKind::Directory)
                    })
                    .padding([9, 10])
                    .style(detail::outlined_button_style),
            )
            .id(format!("detail.property.{}.browse", property.key)),
        ]
        .spacing(8)
        .into(),
        UserPropertyKind::Html => {
            text(plain_text(&value_text(current))).size(13).style(text::secondary).into()
        }
        UserPropertyKind::Unsupported(_) => {
            text(language.text(Text::UnsupportedProperty)).size(13).style(text::secondary).into()
        }
    }
}

fn text_control<'a>(
    property: &'a UserProperty,
    current: &Value,
    placeholder: &'a str,
) -> Element<'a, DetailMessage> {
    text_input(placeholder, &value_text(current))
        .id(format!("detail.property.{}.value", property.key))
        .on_input({
            let key = property.key.clone();
            move |value| DetailMessage::PropertyChanged {
                key: key.clone(),
                value: Value::String(value),
            }
        })
        .padding([9, 10])
        .size(14)
        .width(Fill)
        .style(detail::md_text_input_style)
        .into()
}

fn value_text(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        _ => value.to_string(),
    }
}

fn color_preview(value: &Value) -> Color {
    let values = value
        .as_str()
        .unwrap_or("")
        .split_whitespace()
        .filter_map(|part| part.parse::<f32>().ok())
        .take(3)
        .collect::<Vec<_>>();
    if values.len() == 3 && values.iter().all(|value| value.is_finite()) {
        Color::from_rgb(
            values[0].clamp(0.0, 1.0),
            values[1].clamp(0.0, 1.0),
            values[2].clamp(0.0, 1.0),
        )
    } else {
        Color::from_rgb8(100, 102, 108)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::advanced::widget::{tree::Tag, Tree};
    type InputState =
        text_input::State<<iced::Renderer as iced::advanced::text::Renderer>::Paragraph>;

    fn input_state(tree: &mut Tree) -> Option<&mut InputState> {
        if tree.tag == Tag::of::<InputState>() {
            return Some(tree.state.downcast_mut::<InputState>());
        }
        tree.children.iter_mut().find_map(input_state)
    }

    #[test]
    fn filtering_out_an_input_neither_transfers_nor_loses_another_inputs_focus() {
        for focused in [0, 1] {
            let schema = UserPropertySchema::from_project_json(
                r#"{"general":{"properties":{
                "first":{"type":"file","value":"","order":1},
                "second":{"type":"file","value":"","order":2}}}}"#,
            )
            .unwrap();
            let mut settings = WallpaperSettings::default();
            settings.user_properties.insert("first".into(), serde_json::json!("a.png"));
            settings.user_properties.insert("second".into(), serde_json::json!("b.png"));
            let filter =
                PropertyFilter { query: String::new(), modified_only: true, ..Default::default() };
            let before = view(&schema, &settings, &filter, "wallpaper-a", Language::English);
            let mut tree = Tree::new(before.as_widget());
            input_state(&mut tree.children[focused]).unwrap().focus();
            drop(before);
            settings.user_properties.insert("first".into(), serde_json::json!(""));
            let after = view(&schema, &settings, &filter, "wallpaper-a", Language::English);
            tree.diff(after.as_widget());
            assert_eq!(tree.children.len(), 1);
            assert_eq!(input_state(&mut tree.children[0]).unwrap().is_focused(), focused == 1);
        }
    }

    #[test]
    fn changing_wallpapers_does_not_reuse_a_same_named_propertys_input_focus() {
        let schema = UserPropertySchema::from_project_json(
            r#"{"general":{"properties":{
            "name":{"type":"textinput","value":""}}}}"#,
        )
        .unwrap();
        let settings = WallpaperSettings::default();
        let filter = PropertyFilter::default();
        let before = view(&schema, &settings, &filter, "wallpaper-a", Language::English);
        let mut tree = Tree::new(before.as_widget());
        input_state(&mut tree.children[0]).unwrap().focus();
        let after = view(&schema, &settings, &filter, "wallpaper-b", Language::English);
        tree.diff(after.as_widget());
        assert!(!input_state(&mut tree.children[0]).unwrap().is_focused());
    }

    #[test]
    fn searching_across_groups_keeps_native_widget_state_types_consistent() {
        fn same_shape(expected: &Tree, actual: &Tree) {
            assert_eq!(expected.tag, actual.tag, "stale native widget state type");
            assert_eq!(expected.children.len(), actual.children.len());
            for (expected, actual) in expected.children.iter().zip(&actual.children) {
                same_shape(expected, actual);
            }
        }
        let schema = UserPropertySchema::from_project_json(r#"{"general":{"properties":{
            "scene":{"type":"group","text":"Scene","order":1},
            "mode":{"type":"combo","value":"day","order":2,"options":[{"label":"Day","value":"day"}]},
            "clock":{"type":"group","text":"Clock","order":3},
            "clock_color":{"type":"color","text":"Color","value":"1 1 1","order":4},
            "audio":{"type":"group","text":"Audio","order":5},
            "audio_color":{"type":"color","text":"Color","value":"1 1 1","order":6}}}}"#).unwrap();
        let settings = WallpaperSettings::default();
        let filter = PropertyFilter::default();
        let before = view(&schema, &settings, &filter, "wallpaper", Language::English);
        let mut tree = Tree::new(before.as_widget());
        for query in ["clock color", "audio", "", "color", "scene"] {
            let filter =
                PropertyFilter { query: query.into(), modified_only: false, ..Default::default() };
            let after = view(&schema, &settings, &filter, "wallpaper", Language::English);
            tree.diff(after.as_widget());
            same_shape(&Tree::new(after.as_widget()), &tree);
        }
    }
    #[test]
    fn conditional_visibility_does_not_transfer_or_drop_surviving_input_focus() {
        let schema = UserPropertySchema::from_project_json(
            r#"{"general":{"properties":{
            "first":{"type":"file","value":"a.png","condition":"enabled.value","order":1},
            "second":{"type":"file","value":"b.png","order":2},
            "enabled":{"type":"bool","value":true,"order":3}}}}"#,
        )
        .unwrap();
        for focused in [0, 1] {
            let mut settings = WallpaperSettings::default();
            let filter = PropertyFilter::default();
            let before = view(&schema, &settings, &filter, "wallpaper", Language::English);
            let mut tree = Tree::new(before.as_widget());
            input_state(&mut tree.children[focused]).unwrap().focus();
            drop(before);
            settings.user_properties.insert("enabled".into(), serde_json::json!(false));
            let after = view(&schema, &settings, &filter, "wallpaper", Language::English);
            tree.diff(after.as_widget());
            assert_eq!(tree.children.len(), 2);
            assert_eq!(input_state(&mut tree.children[0]).unwrap().is_focused(), focused == 1);
        }
    }
}
