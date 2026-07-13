use iced::{
    widget::{button, checkbox, column, container, pick_list, row, scrollable, slider, text, text_input},
    Element, Fill,
};
use serde_json::Value;
use we_core::{
    wallpaper::{
        properties::{UserProperty, UserPropertyKind, UserPropertySchema},
        settings::{RenderResolution, Rotation, WallpaperFillMode, WallpaperSettings},
        WallpaperEntry,
    },
};

#[derive(Debug, Clone)]
pub enum DetailMessage {
    Apply,
    FpsChanged(String),
    SpeedChanged(f32),
    VolumeChanged(f32),
    MutedChanged(bool),
    ResolutionModeChanged(ResolutionMode),
    ResolutionWidthChanged(String),
    ResolutionHeightChanged(String),
    FillModeChanged(WallpaperFillMode),
    RotationChanged(Rotation),
    PropertyChanged { key: String, value: Value },
    PickPath { key: String, directory: bool },
    PathPicked { key: String, path: Option<String> },
    ResetProperties,
}

#[derive(Debug, Clone, PartialEq)]
struct PropertyOption {
    label: String,
    value: Value,
}

impl std::fmt::Display for PropertyOption {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.label)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolutionMode {
    Automatic,
    Fixed,
}

impl std::fmt::Display for ResolutionMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Automatic => "Follow output",
            Self::Fixed => "Fixed resolution",
        })
    }
}

pub fn view<'a>(
    entry: &'a WallpaperEntry,
    settings: &'a WallpaperSettings,
    schema: &'a UserPropertySchema,
    resolution_width: &'a str,
    resolution_height: &'a str,
) -> Element<'a, DetailMessage> {
    let resolution_mode = match settings.render_resolution {
        RenderResolution::Automatic => ResolutionMode::Automatic,
        RenderResolution::Fixed { .. } => ResolutionMode::Fixed,
    };

    let playback = column![
        text("Playback").size(20),
        text("Frame rate").size(13),
        text_input("60", &settings.fps.to_string()).on_input(DetailMessage::FpsChanged),
        text(format!("Speed  {:.2}×", settings.speed)).size(13),
        slider(0.1..=3.0, settings.speed, DetailMessage::SpeedChanged),
        text(format!("Volume  {:.0}%", settings.volume * 100.0)).size(13),
        slider(0.0..=1.0, settings.volume, DetailMessage::VolumeChanged),
        checkbox(settings.muted).label("Mute wallpaper audio").on_toggle(DetailMessage::MutedChanged),
    ]
    .spacing(8);

    let presentation = column![
        text("Presentation").size(20),
        text("Render resolution").size(13),
        pick_list(
            vec![ResolutionMode::Automatic, ResolutionMode::Fixed],
            Some(resolution_mode),
            DetailMessage::ResolutionModeChanged,
        ),
        row![
            text_input("Width", resolution_width).on_input(DetailMessage::ResolutionWidthChanged).width(Fill),
            text_input("Height", resolution_height).on_input(DetailMessage::ResolutionHeightChanged).width(Fill),
        ]
        .spacing(8),
        text("Scaling").size(13),
        pick_list(
            vec![WallpaperFillMode::Cover, WallpaperFillMode::Fit, WallpaperFillMode::Stretch, WallpaperFillMode::Center],
            Some(settings.fill_mode),
            DetailMessage::FillModeChanged,
        ),
        text("Rotation").size(13),
        pick_list(
            vec![Rotation::Deg0, Rotation::Deg90, Rotation::Deg180, Rotation::Deg270],
            Some(settings.rotation_degrees),
            DetailMessage::RotationChanged,
        ),
    ]
    .spacing(8);

    let mut properties = column![
        row![text("User properties").size(20), button(text("Reset")).on_press(DetailMessage::ResetProperties)]
            .spacing(12),
    ]
    .spacing(10);
    for property in &schema.entries {
        properties = properties.push(property_view(property, settings));
    }
    if schema.entries.is_empty() {
        properties = properties.push(text("This wallpaper does not declare user properties.").size(14));
    }

    let content = column![
        text(&entry.title).size(26),
        text(entry.id.as_str()).size(13),
        playback,
        presentation,
        properties,
        button(text("Apply wallpaper")).on_press(DetailMessage::Apply).width(Fill),
    ]
    .spacing(18);

    container(scrollable(content).height(Fill))
        .width(400)
        .height(Fill)
        .padding(20)
        .into()
}

fn property_view<'a>(property: &'a UserProperty, settings: &'a WallpaperSettings) -> Element<'a, DetailMessage> {
    let current = settings
        .user_properties
        .get(&property.key)
        .unwrap_or(&property.default);
    let label = text(&property.label).size(15);
    let control: Element<'a, DetailMessage> = match property.kind {
        UserPropertyKind::Boolean => checkbox(current.as_bool().unwrap_or(false))
            .label("Enabled")
            .on_toggle({
                let key = property.key.clone();
                move |value| DetailMessage::PropertyChanged { key: key.clone(), value: Value::Bool(value) }
            })
            .into(),
        UserPropertyKind::Slider => {
            let minimum = property.minimum.unwrap_or(0.0) as f32;
            let maximum = property.maximum.unwrap_or(1.0) as f32;
            let value = current.as_f64().unwrap_or(minimum as f64) as f32;
            slider(minimum..=maximum.max(minimum), value.clamp(minimum, maximum.max(minimum)), {
                let key = property.key.clone();
                move |value| DetailMessage::PropertyChanged {
                    key: key.clone(),
                    value: serde_json::json!(value),
                }
            })
            .into()
        }
        UserPropertyKind::Combo => {
            let choices = property
                .options
                .iter()
                .map(|option| PropertyOption { label: option.label.clone(), value: option.value.clone() })
                .collect::<Vec<_>>();
            let selected = choices.iter().find(|choice| choice.value == *current).cloned();
            pick_list(choices, selected, {
                let key = property.key.clone();
                move |choice| DetailMessage::PropertyChanged { key: key.clone(), value: choice.value }
            })
            .into()
        }
        UserPropertyKind::Color | UserPropertyKind::Text => {
            text_input("Value", &value_text(current))
                .on_input({
                    let key = property.key.clone();
                    move |value| DetailMessage::PropertyChanged { key: key.clone(), value: Value::String(value) }
                })
                .into()
        }
        UserPropertyKind::File | UserPropertyKind::Directory => row![
            text_input("Path", &value_text(current))
                .on_input({
                    let key = property.key.clone();
                    move |value| DetailMessage::PropertyChanged { key: key.clone(), value: Value::String(value) }
                })
                .width(Fill),
            button(text("Browse")).on_press(DetailMessage::PickPath {
                key: property.key.clone(),
                directory: matches!(property.kind, UserPropertyKind::Directory),
            }),
        ]
        .spacing(8)
        .into(),
        UserPropertyKind::Unsupported(_) => text("Unsupported by this renderer").size(13).into(),
    };
    column![label, control].spacing(5).into()
}

fn value_text(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        _ => value.to_string(),
    }
}
