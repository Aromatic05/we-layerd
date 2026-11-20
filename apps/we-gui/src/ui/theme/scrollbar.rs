use crate::ui::theme::colors;
use iced::{widget::scrollable, Background, Border, Theme};

pub(crate) fn md_style(_theme: &Theme, _status: scrollable::Status) -> scrollable::Style {
    scrollable::Style {
        container: iced::widget::container::Style::default(),
        vertical_rail: rail(),
        horizontal_rail: rail(),
        gap: None,
        auto_scroll: scrollable::AutoScroll {
            background: Background::Color(colors().popover_bg),
            border: Border::default(),
            shadow: iced::Shadow::default(),
            icon: colors().fg,
        },
    }
}

fn rail() -> scrollable::Rail {
    scrollable::Rail {
        background: Some(Background::Color(colors().card_bg)),
        border: Border { radius: 6.0.into(), ..Default::default() },
        scroller: scrollable::Scroller {
            background: Background::Color(colors().outline),
            border: Border { radius: 3.0.into(), ..Default::default() },
        },
    }
}
