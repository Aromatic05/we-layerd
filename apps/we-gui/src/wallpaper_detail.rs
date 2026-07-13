use iced::{
    widget::{button, checkbox, column, container, pick_list, row, scrollable, slider, text, text_input},
    alignment::Horizontal, Background, Border, Color, Element, Fill, Theme,
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
        DetailTab::Actions => actions_view(settings, resolution_width, resolution_height),
        DetailTab::UserProperties => properties_view(schema, settings),
    };

    let actions = container(row![
        icon_action(include_bytes!("../assets/icons/check.svg"), DetailMessage::Apply, primary_button_style),
        icon_action(if !is_running || is_paused { include_bytes!("../assets/icons/play_arrow.svg") } else { include_bytes!("../assets/icons/pause.svg") }, DetailMessage::TogglePlayback, tonal_button_style),
        icon_action(include_bytes!("../assets/icons/stop.svg"), DetailMessage::Stop, outlined_button_style),
    ].spacing(12)).width(Fill).align_x(Horizontal::Right);

    container(column![
        column![text(&entry.title).size(24), text(entry.id.as_str()).size(12)].spacing(4),
        tabs,
        scrollable(container(body).padding(iced::Padding { top: 0.0, right: 18.0, bottom: 0.0, left: 0.0 }))
            .height(Fill)
            .direction(iced::widget::scrollable::Direction::Vertical(iced::widget::scrollable::Scrollbar::new().width(12).margin(6).scroller_width(6)))
            .style(md_scrollable_style),
        actions,
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
) -> Element<'a, DetailMessage> {
    let resolution_mode = match settings.render_resolution {
        RenderResolution::Automatic => ResolutionMode::Automatic,
        RenderResolution::Fixed { .. } => ResolutionMode::Fixed,
    };
    let playback = section("Playback", column![
        field_label("Frame rate"),
        text_input("60", &settings.fps.to_string()).on_input(DetailMessage::FpsChanged).padding([14, 10]).style(md_text_input_style),
        text(format!("Speed  {:.2}×", settings.speed)).size(13).color(Color::from_rgb8(196, 199, 204)),
        slider(0.1..=3.0, settings.speed, DetailMessage::SpeedChanged).style(md_slider_style),
        text(format!("Volume  {:.0}%", settings.volume * 100.0)).size(13).color(Color::from_rgb8(196, 199, 204)),
        slider(0.0..=1.0, settings.volume, DetailMessage::VolumeChanged).style(md_slider_style),
        checkbox(settings.muted).label("Mute wallpaper audio").on_toggle(DetailMessage::MutedChanged).style(md_checkbox_style),
    ].spacing(10));
    let presentation = section("Display", column![
        field_label("Render resolution"),
        pick_list(vec![ResolutionMode::Automatic, ResolutionMode::Fixed], Some(resolution_mode), DetailMessage::ResolutionModeChanged).padding([14, 10]).width(Fill).style(md_pick_list_style).menu_style(md_menu_style),
        row![
            text_input("Width", resolution_width).on_input(DetailMessage::ResolutionWidthChanged).padding([14, 10]).width(Fill).style(md_text_input_style),
            text_input("Height", resolution_height).on_input(DetailMessage::ResolutionHeightChanged).padding([14, 10]).width(Fill).style(md_text_input_style),
        ].spacing(8),
        field_label("Scaling"),
        pick_list(vec![WallpaperFillMode::Cover, WallpaperFillMode::Fit, WallpaperFillMode::Stretch, WallpaperFillMode::Center], Some(settings.fill_mode), DetailMessage::FillModeChanged).padding([14, 10]).width(Fill).style(md_pick_list_style).menu_style(md_menu_style),
        field_label("Rotation"),
        pick_list(vec![Rotation::Deg0, Rotation::Deg90, Rotation::Deg180, Rotation::Deg270], Some(settings.rotation_degrees), DetailMessage::RotationChanged).padding([14, 10]).width(Fill).style(md_pick_list_style).menu_style(md_menu_style),
    ].spacing(10));

    column![playback, presentation].spacing(16).into()
}

fn properties_view<'a>(schema: &'a UserPropertySchema, settings: &'a WallpaperSettings) -> Element<'a, DetailMessage> {
    let mut properties = column![
        row![text("User properties").size(20), button(text("↺").size(18)).on_press(DetailMessage::ResetProperties).style(outlined_button_style)].spacing(12),
        text("Wallpaper-specific controls are saved when applied.").size(13),
    ]
    .spacing(12);
    for property in &schema.entries {
        properties = properties.push(container(row![text(&property.label).size(14).width(Fill), property_control(property, settings)].align_y(iced::Alignment::Center).spacing(12)).padding(12).style(section_style));
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
        }).style(md_checkbox_style).into(),
        UserPropertyKind::Slider => {
            let minimum = property.minimum.unwrap_or(0.0) as f32;
            let maximum = property.maximum.unwrap_or(1.0) as f32;
            let value = current.as_f64().unwrap_or(minimum as f64) as f32;
            slider(minimum..=maximum.max(minimum), value.clamp(minimum, maximum.max(minimum)), {
                let key = property.key.clone();
                move |value| DetailMessage::PropertyChanged { key: key.clone(), value: serde_json::json!(value) }
            }).style(md_slider_style).into()
        }
        UserPropertyKind::Combo => {
            let choices = property.options.iter().map(|option| PropertyOption { label: option.label.clone(), value: option.value.clone() }).collect::<Vec<_>>();
            let selected = choices.iter().find(|choice| choice.value == *current).cloned();
            pick_list(choices, selected, {
                let key = property.key.clone();
                move |choice| DetailMessage::PropertyChanged { key: key.clone(), value: choice.value }
            }).padding([14, 10]).width(Fill).style(md_pick_list_style).menu_style(md_menu_style).into()
        }
        UserPropertyKind::Color | UserPropertyKind::Text => text_input("Value", &value_text(current)).on_input({
            let key = property.key.clone();
            move |value| DetailMessage::PropertyChanged { key: key.clone(), value: Value::String(value) }
        }).padding([14, 10]).width(Fill).style(md_text_input_style).into(),
        UserPropertyKind::File | UserPropertyKind::Directory => row![
            text_input("Path", &value_text(current)).on_input({
                let key = property.key.clone();
                move |value| DetailMessage::PropertyChanged { key: key.clone(), value: Value::String(value) }
            }).padding([14, 10]).width(Fill).style(md_text_input_style),
            button(text("…").size(20)).on_press(DetailMessage::PickPath { key: property.key.clone(), directory: matches!(property.kind, UserPropertyKind::Directory) }).style(outlined_button_style),
        ].spacing(8).into(),
        UserPropertyKind::Html => container(text(render_html(&value_text(current))).size(14)).width(Fill).padding(8).style(section_style).into(),
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

fn icon_action<'a>(icon: &'static [u8], message: DetailMessage, style: fn(&Theme, button::Status) -> button::Style) -> iced::widget::Button<'a, DetailMessage> {
    button(iced::widget::svg(iced::widget::svg::Handle::from_memory(icon)).width(22).height(22)).on_press(message).width(48).height(48).style(style)
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
    button::Style { background: Some(Background::Color(Color::from_rgb8(65, 83, 116))), text_color: Color::from_rgb8(220, 231, 255), border: Border { radius: 24.0.into(), ..Default::default() }, ..Default::default() }
}

fn outlined_button_style(_theme: &Theme, _status: button::Status) -> button::Style {
    button::Style { text_color: Color::from_rgb8(198, 210, 242), border: Border { radius: 24.0.into(), width: 1.0, color: Color::from_rgb8(143, 147, 156) }, ..Default::default() }
}

fn md_text_input_style(_theme: &Theme, status: iced::widget::text_input::Status) -> iced::widget::text_input::Style {
    let border = if matches!(status, iced::widget::text_input::Status::Focused { .. }) { Color::from_rgb8(174, 198, 255) } else { Color::from_rgb8(143, 147, 156) };
    iced::widget::text_input::Style {
        background: Background::Color(Color::from_rgb8(43, 44, 48)),
        border: Border { radius: 8.0.into(), width: 1.0, color: border },
        icon: Color::from_rgb8(196, 199, 204),
        placeholder: Color::from_rgb8(196, 199, 204),
        value: Color::from_rgb8(230, 225, 229),
        selection: Color::from_rgb8(78, 99, 139),
    }
}

fn md_checkbox_style(_theme: &Theme, status: iced::widget::checkbox::Status) -> iced::widget::checkbox::Style {
    let checked = matches!(status, iced::widget::checkbox::Status::Active { is_checked: true } | iced::widget::checkbox::Status::Hovered { is_checked: true } | iced::widget::checkbox::Status::Disabled { is_checked: true });
    iced::widget::checkbox::Style {
        background: Background::Color(if checked { Color::from_rgb8(174, 198, 255) } else { Color::from_rgb8(43, 44, 48) }),
        icon_color: Color::from_rgb8(26, 39, 64),
        border: Border { radius: 2.0.into(), width: if checked { 0.0 } else { 2.0 }, color: Color::from_rgb8(196, 199, 204) },
        text_color: Some(Color::from_rgb8(230, 225, 229)),
    }
}

fn md_pick_list_style(_theme: &Theme, status: iced::widget::pick_list::Status) -> iced::widget::pick_list::Style {
    let focused = matches!(status, iced::widget::pick_list::Status::Opened { .. });
    iced::widget::pick_list::Style {
        text_color: Color::from_rgb8(230, 225, 229),
        placeholder_color: Color::from_rgb8(196, 199, 204),
        handle_color: Color::from_rgb8(196, 199, 204),
        background: Background::Color(Color::from_rgb8(43, 44, 48)),
        border: Border { radius: 8.0.into(), width: 1.0, color: if focused { Color::from_rgb8(174, 198, 255) } else { Color::from_rgb8(143, 147, 156) } },
    }
}

fn md_menu_style(_theme: &Theme) -> iced::overlay::menu::Style {
    iced::overlay::menu::Style {
        background: Background::Color(Color::from_rgb8(48, 49, 53)),
        border: Border { radius: 8.0.into(), width: 1.0, color: Color::from_rgb8(72, 74, 80) },
        text_color: Color::from_rgb8(230, 225, 229),
        selected_text_color: Color::from_rgb8(26, 39, 64),
        selected_background: Background::Color(Color::from_rgb8(174, 198, 255)),
        shadow: iced::Shadow { color: Color::from_rgba8(0, 0, 0, 0.35), blur_radius: 12.0, offset: iced::Vector::new(0.0, 4.0) },
    }
}

fn md_slider_style(_theme: &Theme, _status: iced::widget::slider::Status) -> iced::widget::slider::Style {
    iced::widget::slider::Style {
        rail: iced::widget::slider::Rail {
            backgrounds: (Background::Color(Color::from_rgb8(174, 198, 255)), Background::Color(Color::from_rgb8(75, 77, 84))),
            width: 4.0,
            border: Border::default(),
        },
        handle: iced::widget::slider::Handle {
            shape: iced::widget::slider::HandleShape::Circle { radius: 10.0 },
            background: Background::Color(Color::from_rgb8(174, 198, 255)),
            border_width: 0.0,
            border_color: Color::TRANSPARENT,
        },
    }
}

fn md_scrollable_style(_theme: &Theme, _status: iced::widget::scrollable::Status) -> iced::widget::scrollable::Style {
    let rail = iced::widget::scrollable::Rail {
        background: Some(Background::Color(Color::from_rgb8(39, 40, 44))),
        border: Border { radius: 6.0.into(), ..Default::default() },
        scroller: iced::widget::scrollable::Scroller {
            background: Background::Color(Color::from_rgb8(143, 147, 156)),
            border: Border { radius: 3.0.into(), ..Default::default() },
        },
    };
    iced::widget::scrollable::Style { container: iced::widget::container::Style::default(), vertical_rail: rail, horizontal_rail: rail, gap: None, auto_scroll: iced::widget::scrollable::AutoScroll { background: Background::Color(Color::from_rgb8(48, 49, 53)), border: Border::default(), shadow: iced::Shadow::default(), icon: Color::from_rgb8(230, 225, 229) } }
}

fn value_text(value: &Value) -> String {
    match value { Value::String(value) => value.clone(), _ => value.to_string() }
}

fn render_html(value: &str) -> String {
    html2text::from_read(value.as_bytes(), 56).unwrap_or_else(|_| value.to_string())
}
