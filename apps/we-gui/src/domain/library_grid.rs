const GRID_SPACING: f32 = 12.0;
const GRID_PADDING: f32 = 12.0;
const TARGET_CARD_WIDTH: f32 = 360.0;
const PLAYLIST_CONTROLS_HEIGHT: f32 = 38.0;
const OVERSCAN_ROWS: usize = 2;
pub(crate) const MAX_ANIMATED_PREVIEWS: usize = 8;

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

#[cfg(test)]
impl GridWindow {
    pub(crate) fn visible_item_count(self) -> usize {
        self.end_item.saturating_sub(self.start_item)
    }
}

pub(crate) fn grid_window(
    total_items: usize,
    width: f32,
    scroll_y: f32,
    viewport_height: f32,
    playlist_controls: bool,
) -> GridWindow {
    let available_width = (width - GRID_PADDING * 2.0).max(180.0);
    let cols = ((available_width + GRID_SPACING) / (TARGET_CARD_WIDTH + GRID_SPACING))
        .floor()
        .max(1.0) as usize;
    let card_width =
        ((available_width - GRID_SPACING * cols.saturating_sub(1) as f32) / cols as f32).max(180.0);
    let card_height = (card_width * 9.0 / 16.0).round();
    let controls_height = if playlist_controls { PLAYLIST_CONTROLS_HEIGHT } else { 0.0 };
    let row_pitch = card_height + controls_height + GRID_SPACING;
    let total_rows = total_items.div_ceil(cols);

    if total_rows == 0 {
        return GridWindow {
            cols,
            card_width,
            card_height,
            row_pitch,
            total_rows: 0,
            start_row: 0,
            end_row: 0,
            start_item: 0,
            end_item: 0,
            top_spacer: 0.0,
            bottom_spacer: 0.0,
        };
    }

    let viewport_height = viewport_height.max(row_pitch);
    let first_visible =
        ((scroll_y.max(0.0) / row_pitch).floor() as usize).min(total_rows.saturating_sub(1));
    let last_visible_exclusive = (((scroll_y.max(0.0) + viewport_height) / row_pitch).ceil()
        as usize)
        .max(first_visible + 1)
        .min(total_rows);
    let start_row = first_visible.saturating_sub(OVERSCAN_ROWS);
    let end_row = last_visible_exclusive.saturating_add(OVERSCAN_ROWS).min(total_rows);
    let start_item = start_row.saturating_mul(cols).min(total_items);
    let end_item = end_row.saturating_mul(cols).min(total_items);

    GridWindow {
        cols,
        card_width,
        card_height,
        row_pitch,
        total_rows,
        start_row,
        end_row,
        start_item,
        end_item,
        top_spacer: start_row as f32 * row_pitch,
        bottom_spacer: total_rows.saturating_sub(end_row) as f32 * row_pitch,
    }
}

pub(crate) fn gif_tick_needed(animated_preview_count: usize) -> bool {
    animated_preview_count > 0
}

pub(crate) fn gif_result_is_current(
    task_generation: u64,
    current_generation: u64,
    still_desired: bool,
) -> bool {
    still_desired && task_generation == current_generation
}

pub(crate) fn bounded_animation_candidates<T>(candidates: impl IntoIterator<Item = T>) -> Vec<T> {
    candidates.into_iter().take(MAX_ANIMATED_PREVIEWS).collect()
}

#[cfg(test)]
mod tests {
    use super::{
        bounded_animation_candidates, gif_result_is_current, gif_tick_needed, grid_window,
        MAX_ANIMATED_PREVIEWS,
    };

    #[test]
    fn large_library_builds_only_a_bounded_visible_window() {
        let window = grid_window(53_571, 1_280.0, 0.0, 720.0, false);

        assert!(
            window.visible_item_count() <= 30,
            "rendered {} cards",
            window.visible_item_count()
        );
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

    #[test]
    fn animated_preview_candidates_have_a_global_hard_limit() {
        let candidates = bounded_animation_candidates(0..64);

        assert_eq!(candidates.len(), MAX_ANIMATED_PREVIEWS);
        assert_eq!(candidates, (0..MAX_ANIMATED_PREVIEWS).collect::<Vec<_>>());
    }

    #[test]
    fn gif_result_from_an_old_library_generation_is_never_accepted() {
        assert!(!gif_result_is_current(4, 5, true));
        assert!(!gif_result_is_current(5, 5, false));
        assert!(gif_result_is_current(5, 5, true));
    }
}
