use iced::{
    widget::{button, checkbox, column, container, pick_list, row, scrollable, slider, text, text_input},
    Background, Border, Color, Element, Fill, Theme,
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
    TogglePlayback,
    Stop,
    SelectTab(DetailTab),
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetailTab {
    Actions,
    UserProperties,
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
    active_tab: DetailTab,
    is_running: bool,
    is_paused: bool,
) -> Element<'a, DetailMessage> {
    let tabs = row![
        tab_button("Actions", DetailTab::Actions, active_tab),
        tab_button("User properties", DetailTab::UserProperties, active_tab),
    ]
    .spacing(8);

    let body = match active_tab {
        DetailTab::Actions => actions_view(settings, resolution_width, resolution_height, is_running, is_paused),
        DetailTab::UserProperties => properties_view(schema, settings),
    };

    let apply = button(text("Apply wallpaper").size(15))
        .on_press(DetailMessage::Apply)
        .width(Fill)
        .padding([13, 20])
        .style(primary_button_style);

    container(column![
        column![text(&entry.title).size(24), text(entry.id.as_str()).size(12)].spacing(4),
        tabs,
        scrollable(body).height(Fill),
        apply,
    ]
    .spacing(18))
    .width(Fill)
    .height(Fill)
    .padding(20)
    .style(sidebar_style)
    .into()
}

fn actions_view<'a>(
    settings: &'a WallpaperSettings,
    resolution_width: &'a str,
    resolution_height: &'a str,
    is_running: bool,
    is_paused: bool,
) -> Element<'a, DetailMessage> {
    let resolution_mode = match settings.render_resolution {
        RenderResolution::Automatic => ResolutionMode::Automatic,
        RenderResolution::Fixed { .. } => ResolutionMode::Fixed,
    };
    let playback_label = if !is_running || is_paused { "Play" } else { "Pause" };
    let playback = section("Playback", column![
        row![
            button(text(playback_label)).on_press(DetailMessage::TogglePlayback).style(tonal_button_style),
            button(text("Stop")).on_press(DetailMessage::Stop).style(outlined_button_style),
        ]
        .spacing(10),
        field_label("Frame rate"),
        text_input("60", &settings.fps.to_string()).on_input(DetailMessage::FpsChanged).padding(10),
        text(format!("Speed  {:.2}×", settings.speed)).size(13).color(Color::from_rgb8(196, 199, 204)),
        slider(0.1..=3.0, settings.speed, DetailMessage::SpeedChanged),
        text(format!("Volume  {:.0}%", settings.volume * 100.0)).size(13).color(Color::from_rgb8(196, 199, 204)),
        slider(0.0..=1.0, settings.volume, DetailMessage::VolumeChanged),
        checkbox(settings.muted).label("Mute wallpaper audio").on_toggle(DetailMessage::MutedChanged),
    ].spacing(10));
    let presentation = section("Display", column![
        field_label("Render resolution"),
        pick_list(vec![ResolutionMode::Automatic, ResolutionMode::Fixed], Some(resolution_mode), DetailMessage::ResolutionModeChanged).padding(10),
        row![
            text_input("Width", resolution_width).on_input(DetailMessage::ResolutionWidthChanged).width(Fill),
            text_input("Height", resolution_height).on_input(DetailMessage::ResolutionHeightChanged).width(Fill),
        ].spacing(8),
        field_label("Scaling"),
        pick_list(vec![WallpaperFillMode::Cover, WallpaperFillMode::Fit, WallpaperFillMode::Stretch, WallpaperFillMode::Center], Some(settings.fill_mode), DetailMessage::FillModeChanged).padding(10),
        field_label("Rotation"),
        pick_list(vec![Rotation::Deg0, Rotation::Deg90, Rotation::Deg180, Rotation::Deg270], Some(settings.rotation_degrees), DetailMessage::RotationChanged).padding(10),
    ].spacing(10));

    column![playback, presentation].spacing(16).into()
}

fn properties_view<'a>(schema: &'a UserPropertySchema, settings: &'a WallpaperSettings) -> Element<'a, DetailMessage> {
    let mut properties = column![
        row![text("User properties").size(20), button(text("Reset")).on_press(DetailMessage::ResetProperties).style(outlined_button_style)].spacing(12),
        text("Wallpaper-specific controls are saved when applied.").size(13),
    ]
    .spacing(12);
    for property in &schema.entries {
        properties = properties.push(section(&property.label, column![property_control(property, settings)]));
    }
    if schema.entries.is_empty() {
        properties = properties.push(container(text("This wallpaper does not declare user properties.").size(14)).padding(16).style(section_style));
    }
    properties.into()
}

fn property_control<'a>(property: &'a UserProperty, settings: &'a WallpaperSettings) -> Element<'a, DetailMessage> {
    let current = settings.user_properties.get(&property.key).unwrap_or(&property.default);
    match property.kind {
        UserPropertyKind::Boolean => checkbox(current.as_bool().unwrap_or(false)).label("Enabled").on_toggle({
            let key = property.key.clone();
            move |value| DetailMessage::PropertyChanged { key: key.clone(), value: Value::Bool(value) }
        }).into(),
        UserPropertyKind::Slider => {
            let minimum = property.minimum.unwrap_or(0.0) as f32;
            let maximum = property.maximum.unwrap_or(1.0) as f32;
            let value = current.as_f64().unwrap_or(minimum as f64) as f32;
            slider(minimum..=maximum.max(minimum), value.clamp(minimum, maximum.max(minimum)), {
                let key = property.key.clone();
                move |value| DetailMessage::PropertyChanged { key: key.clone(), value: serde_json::json!(value) }
            }).into()
        }
        UserPropertyKind::Combo => {
            let choices = property.options.iter().map(|option| PropertyOption { label: option.label.clone(), value: option.value.clone() }).collect::<Vec<_>>();
            let selected = choices.iter().find(|choice| choice.value == *current).cloned();
            pick_list(choices, selected, {
                let key = property.key.clone();
                move |choice| DetailMessage::PropertyChanged { key: key.clone(), value: choice.value }
            }).padding(10).into()
        }
        UserPropertyKind::Color | UserPropertyKind::Text => text_input("Value", &value_text(current)).on_input({
            let key = property.key.clone();
            move |value| DetailMessage::PropertyChanged { key: key.clone(), value: Value::String(value) }
        }).padding(10).into(),
        UserPropertyKind::File | UserPropertyKind::Directory => row![
            text_input("Path", &value_text(current)).on_input({
                let key = property.key.clone();
                move |value| DetailMessage::PropertyChanged { key: key.clone(), value: Value::String(value) }
            }).padding(10).width(Fill),
            button(text("Browse")).on_press(DetailMessage::PickPath { key: property.key.clone(), directory: matches!(property.kind, UserPropertyKind::Directory) }).style(outlined_button_style),
        ].spacing(8).into(),
        UserPropertyKind::Unsupported(_) => text("Unsupported by this renderer").size(13).into(),
    }
}

fn field_label<'a>(value: &'a str) -> iced::widget::Text<'a> {
    text(value).size(13).color(Color::from_rgb8(196, 199, 204))
}

fn section<'a>(title: &'a str, content: impl Into<Element<'a, DetailMessage>>) -> Element<'a, DetailMessage> {
    container(column![text(title).size(18), content.into()].spacing(12)).padding(16).style(section_style).into()
}

fn tab_button<'a>(label: &'a str, tab: DetailTab, active: DetailTab) -> iced::widget::Button<'a, DetailMessage> {
    button(text(label).size(14)).on_press(DetailMessage::SelectTab(tab)).padding([9, 14]).style(move |_theme, status| {
        let selected = tab == active;
        let background = if selected { Color::from_rgb8(70, 92, 130) } else if matches!(status, button::Status::Hovered) { Color::from_rgb8(48, 50, 55) } else { Color::TRANSPARENT };
        button::Style { background: Some(Background::Color(background)), text_color: if selected { Color::from_rgb8(224, 232, 255) } else { Color::from_rgb8(198, 199, 204) }, border: Border { radius: 20.0.into(), ..Default::default() }, ..Default::default() }
    })
}

fn sidebar_style(_theme: &Theme) -> container::Style {
    container::Style { background: Some(Background::Color(Color::from_rgb8(30, 31, 34))), ..Default::default() }
}

fn section_style(_theme: &Theme) -> container::Style {
    container::Style { background: Some(Background::Color(Color::from_rgb8(39, 40, 44))), border: Border { radius: 16.0.into(), ..Default::default() }, ..Default::default() }
}

fn primary_button_style(_theme: &Theme, status: button::Status) -> button::Style {
    let background = if matches!(status, button::Status::Hovered) { Color::from_rgb8(175, 199, 255) } else { Color::from_rgb8(153, 178, 241) };
    button::Style { background: Some(Background::Color(background)), text_color: Color::from_rgb8(20, 37, 68), border: Border { radius: 24.0.into(), ..Default::default() }, ..Default::default() }
}

fn tonal_button_style(_theme: &Theme, _status: button::Status) -> button::Style {
    button::Style { background: Some(Background::Color(Color::from_rgb8(65, 83, 116))), text_color: Color::from_rgb8(220, 231, 255), border: Border { radius: 20.0.into(), ..Default::default() }, ..Default::default() }
}

fn outlined_button_style(_theme: &Theme, _status: button::Status) -> button::Style {
    button::Style { text_color: Color::from_rgb8(198, 210, 242), border: Border { radius: 20.0.into(), width: 1.0, color: Color::from_rgb8(143, 147, 156) }, ..Default::default() }
}

fn value_text(value: &Value) -> String {
    match value { Value::String(value) => value.clone(), _ => value.to_string() }
}
