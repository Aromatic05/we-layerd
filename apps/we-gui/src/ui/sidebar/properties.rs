use iced::{
    widget::{button, checkbox, column, container, pick_list, row, slider, text, text_input},
    Alignment, Element, Fill,
};
use serde_json::Value;
use we_core::wallpaper::{
    properties::{UserProperty, UserPropertyKind, UserPropertySchema},
    settings::WallpaperSettings,
};

use crate::domain::i18n::{Language, Text};

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

pub(crate) fn view<'a>(
    schema: &'a UserPropertySchema,
    settings: &'a WallpaperSettings,
    language: Language,
) -> Element<'a, DetailMessage> {
    let mut properties = column![
        row![
            text(language.text(Text::UserProperties)).size(20),
            container(
                button(text(format!("↺  {}", language.text(Text::ResetProperties))).size(13))
                    .on_press(DetailMessage::ResetProperties)
                    .style(detail::outlined_button_style),
            )
            .id("detail.properties.reset"),
        ]
        .spacing(12),
        text(language.text(Text::PropertiesSavedAutomatically)).size(13),
    ]
    .spacing(12);
    for property in &schema.entries {
        properties = properties.push(
            container(
                row![
                    text(&property.label).size(14).width(Fill),
                    control(property, settings, language)
                ]
                .align_y(Alignment::Center)
                .spacing(12),
            )
            .id(format!("detail.property.{}", property.key))
            .padding(12)
            .style(detail::section_style),
        );
    }
    if schema.entries.is_empty() {
        properties = properties.push(
            container(text(language.text(Text::NoUserProperties)).size(14))
                .id("detail.properties.empty")
                .padding(16)
                .style(detail::section_style),
        );
    }
    properties.into()
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
                    },
                )
                .style(detail::md_slider_style),
            )
            .id(format!("detail.property.{}.slider", property.key))
            .into()
        }
        UserPropertyKind::Combo => {
            let choices = property
                .options
                .iter()
                .map(|option| PropertyOption {
                    label: option.label.clone(),
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
                .padding([14, 10])
                .width(Fill)
                .style(detail::md_pick_list_style)
                .menu_style(detail::md_menu_style),
            )
            .id(format!("detail.property.{}.choice", property.key))
            .into()
        }
        UserPropertyKind::Color | UserPropertyKind::Text => {
            text_input(language.text(Text::Value), &value_text(current))
                .id(format!("detail.property.{}.value", property.key))
                .on_input({
                    let key = property.key.clone();
                    move |value| DetailMessage::PropertyChanged {
                        key: key.clone(),
                        value: Value::String(value),
                    }
                })
                .padding([14, 10])
                .width(Fill)
                .style(detail::md_text_input_style)
                .into()
        }
        UserPropertyKind::File | UserPropertyKind::Directory => row![
            text_input(language.text(Text::Path), &value_text(current))
                .id(format!("detail.property.{}.path", property.key))
                .on_input({
                    let key = property.key.clone();
                    move |value| DetailMessage::PropertyChanged {
                        key: key.clone(),
                        value: Value::String(value),
                    }
                })
                .padding([14, 10])
                .width(Fill)
                .style(detail::md_text_input_style),
            container(
                button(text(language.text(Text::Browse)).size(13))
                    .on_press(DetailMessage::PickPath {
                        key: property.key.clone(),
                        directory: matches!(property.kind, UserPropertyKind::Directory)
                    })
                    .style(detail::outlined_button_style),
            )
            .id(format!("detail.property.{}.browse", property.key)),
        ]
        .spacing(8)
        .into(),
        UserPropertyKind::Html => container(text(render_html(&value_text(current))).size(14))
            .width(Fill)
            .padding(8)
            .style(detail::section_style)
            .into(),
        UserPropertyKind::Unsupported(_) => {
            text(language.text(Text::UnsupportedProperty)).size(13).into()
        }
    }
}

fn value_text(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        _ => value.to_string(),
    }
}

fn render_html(value: &str) -> String {
    html2text::from_read(value.as_bytes(), 56).unwrap_or_else(|_| value.to_string())
}
