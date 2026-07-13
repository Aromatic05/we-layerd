use iced::{widget::scrollable, Background, Border, Color, Theme};

pub(crate) fn md_style(_theme: &Theme, _status: scrollable::Status) -> scrollable::Style {
    scrollable::Style {
        container: iced::widget::container::Style::default(),
        vertical_rail: rail(),
        horizontal_rail: rail(),
        gap: None,
        auto_scroll: scrollable::AutoScroll {
            background: Background::Color(Color::from_rgb8(48, 49, 53)),
            border: Border::default(),
            shadow: iced::Shadow::default(),
            icon: Color::from_rgb8(230, 225, 229),
        },
    }
}

fn rail() -> scrollable::Rail {
    scrollable::Rail {
        background: Some(Background::Color(Color::from_rgb8(39, 40, 44))),
        border: Border { radius: 6.0.into(), ..Default::default() },
        scroller: scrollable::Scroller {
            background: Background::Color(Color::from_rgb8(143, 147, 156)),
            border: Border { radius: 3.0.into(), ..Default::default() },
        },
    }
}
