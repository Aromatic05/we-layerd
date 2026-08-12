#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct GridWindow {
    pub cols: usize,
    pub card_width: f32,
    pub card_height: f32,
    pub row_pitch: f32,
    pub total_rows: usize,
    pub start_row: usize,
    pub end_row: usize,
    pub start_item: usize,
    pub end_item: usize,
    pub top_spacer: f32,
    pub bottom_spacer: f32,
}

impl GridWindow {
    pub(crate) fn visible_item_count(self) -> usize {
        self.end_item.saturating_sub(self.start_item)
    }
}

pub(crate) fn grid_window(
    total_items: usize,
    width: f32,
    _scroll_y: f32,
    _viewport_height: f32,
    playlist_controls: bool,
) -> GridWindow {
    let spacing = 12.0;
    let target_card_width = 360.0;
    let cols = ((width + spacing) / (target_card_width + spacing)).floor().max(1.0) as usize;
    let card_width =
        ((width - spacing * cols.saturating_sub(1) as f32) / cols as f32).max(180.0);
    let card_height = (card_width * 9.0 / 16.0).round();
    let controls_height = if playlist_controls { 38.0 } else { 0.0 };
    let row_pitch = card_height + controls_height + spacing;
    let total_rows = total_items.div_ceil(cols);

    GridWindow {
        cols,
        card_width,
        card_height,
        row_pitch,
        total_rows,
        start_row: 0,
        end_row: total_rows,
        start_item: 0,
        end_item: total_items,
        top_spacer: 0.0,
        bottom_spacer: 0.0,
    }
}

pub(crate) fn gif_tick_needed(animated_preview_count: usize) -> bool {
    animated_preview_count > 0
}

#[cfg(test)]
mod tests {
    use super::{gif_tick_needed, grid_window};

    #[test]
    fn large_library_builds_only_a_bounded_visible_window() {
        let window = grid_window(53_571, 1_280.0, 0.0, 720.0, false);

        assert!(window.visible_item_count() <= 30, "rendered {} cards", window.visible_item_count());
        assert!(window.end_row < window.total_rows);
        assert!(window.bottom_spacer > 0.0);
    }

    #[test]
    fn deep_scroll_keeps_the_full_extent_and_original_item_indexes() {
        let total = 53_571;
        let first = grid_window(total, 1_280.0, 0.0, 720.0, false);
        let scroll_y = first.row_pitch * 8_000.0;
        let window = grid_window(total, 1_280.0, scroll_y, 720.0, false);

        assert!(window.start_row > 7_990);
        assert_eq!(window.start_item, window.start_row * window.cols);
        assert!(window.end_item <= total);
        assert!(window.visible_item_count() <= 30);

        let represented_height = window.top_spacer
            + (window.end_row - window.start_row) as f32 * window.row_pitch
            + window.bottom_spacer;
        let total_height = window.total_rows as f32 * window.row_pitch;
        assert!((represented_height - total_height).abs() < 1.0);
    }

    #[test]
    fn gif_tick_is_disabled_when_no_animated_preview_is_visible() {
        assert!(!gif_tick_needed(0));
        assert!(gif_tick_needed(1));
    }
}
