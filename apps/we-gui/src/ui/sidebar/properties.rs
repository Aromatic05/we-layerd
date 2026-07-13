use iced::{
    widget::{button, checkbox, column, container, pick_list, row, slider, text, text_input},
    Alignment, Element, Fill,
};
use serde_json::Value;
use we_core::wallpaper::{
    properties::{UserProperty, UserPropertyKind, UserPropertySchema},
    settings::WallpaperSettings,
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

pub(crate) fn view<'a>(schema: &'a UserPropertySchema, settings: &'a WallpaperSettings) -> Element<'a, DetailMessage> {
    let mut properties = column![
        row![
            text("User properties").size(20),
            button(text("↺").size(18))
                .on_press(DetailMessage::ResetProperties)
                .style(detail::outlined_button_style),
        ]
        .spacing(12),
        text("Wallpaper-specific controls are saved when applied.").size(13),
    ]
    .spacing(12);
    for property in &schema.entries {
        properties = properties.push(
            container(
                row![text(&property.label).size(14).width(Fill), control(property, settings)]
                    .align_y(Alignment::Center)
                    .spacing(12),
            )
            .padding(12)
            .style(detail::section_style),
        );
    }
    if schema.entries.is_empty() {
        properties = properties.push(
            container(text("This wallpaper does not declare user properties.").size(14))
                .padding(16)
                .style(detail::section_style),
        );
    }
    properties.into()
}

fn control<'a>(property: &'a UserProperty, settings: &'a WallpaperSettings) -> Element<'a, DetailMessage> {
    let current = settings.user_properties.get(&property.key).unwrap_or(&property.default);
    match property.kind {
        UserPropertyKind::Boolean => checkbox(current.as_bool().unwrap_or(false))
            .label("Enabled")
            .on_toggle({
                let key = property.key.clone();
                move |value| DetailMessage::PropertyChanged { key: key.clone(), value: Value::Bool(value) }
            })
            .style(detail::md_checkbox_style)
            .into(),
        UserPropertyKind::Slider => {
            let minimum = property.minimum.unwrap_or(0.0) as f32;
            let maximum = property.maximum.unwrap_or(1.0) as f32;
            let value = current.as_f64().unwrap_or(minimum as f64) as f32;
            slider(minimum..=maximum.max(minimum), value.clamp(minimum, maximum.max(minimum)), {
                let key = property.key.clone();
                move |value| DetailMessage::PropertyChanged { key: key.clone(), value: serde_json::json!(value) }
            })
            .style(detail::md_slider_style)
            .into()
        }
        UserPropertyKind::Combo => {
            let choices = property.options.iter().map(|option| PropertyOption { label: option.label.clone(), value: option.value.clone() }).collect::<Vec<_>>();
            let selected = choices.iter().find(|choice| choice.value == *current).cloned();
            pick_list(choices, selected, {
                let key = property.key.clone();
                move |choice| DetailMessage::PropertyChanged { key: key.clone(), value: choice.value }
            })
            .padding([14, 10])
            .width(Fill)
            .style(detail::md_pick_list_style)
            .menu_style(detail::md_menu_style)
            .into()
        }
        UserPropertyKind::Color | UserPropertyKind::Text => text_input("Value", &value_text(current))
            .on_input({
                let key = property.key.clone();
                move |value| DetailMessage::PropertyChanged { key: key.clone(), value: Value::String(value) }
            })
            .padding([14, 10])
            .width(Fill)
            .style(detail::md_text_input_style)
            .into(),
        UserPropertyKind::File | UserPropertyKind::Directory => row![
            text_input("Path", &value_text(current))
                .on_input({
                    let key = property.key.clone();
                    move |value| DetailMessage::PropertyChanged { key: key.clone(), value: Value::String(value) }
                })
                .padding([14, 10])
                .width(Fill)
                .style(detail::md_text_input_style),
            button(text("…").size(20))
                .on_press(DetailMessage::PickPath { key: property.key.clone(), directory: matches!(property.kind, UserPropertyKind::Directory) })
                .style(detail::outlined_button_style),
        ]
        .spacing(8)
        .into(),
        UserPropertyKind::Html => container(text(render_html(&value_text(current))).size(14))
            .width(Fill)
            .padding(8)
            .style(detail::section_style)
            .into(),
        UserPropertyKind::Unsupported(_) => text("Unsupported by this renderer").size(13).into(),
    }
}

fn value_text(value: &Value) -> String {
    match value { Value::String(value) => value.clone(), _ => value.to_string() }
}

fn render_html(value: &str) -> String {
    html2text::from_read(value.as_bytes(), 56).unwrap_or_else(|_| value.to_string())
}
