use ratatui::{
    crossterm::event::MouseEvent,
    layout::{Position, Rect},
    style::Style,
    widgets::{Scrollbar, ScrollbarOrientation},
};

pub mod config;
pub mod dir_terminal;
pub mod motion;
pub mod splash;
pub mod status;
pub mod tc_list;
pub mod testcase;
pub mod theme;
pub mod ui;

/// Move a cursor by one, wrapping at both ends.
pub fn step_wrapped(current: usize, total: usize, backwards: bool) -> usize {
    if total == 0 {
        return 0;
    }
    let current = current.min(total - 1);
    if backwards {
        (current + total - 1) % total
    } else {
        (current + 1) % total
    }
}

/// Keep an index-based cursor inside a scrolling window of `visible` rows.
pub fn scroll_to_show(cursor: usize, scroll: usize, visible: usize, total: usize) -> usize {
    if total == 0 || visible == 0 {
        return 0;
    }
    let cursor = cursor.min(total - 1);
    let max_scroll = total.saturating_sub(visible);
    let mut scroll = scroll.min(max_scroll);
    if cursor < scroll {
        scroll = cursor;
    } else if cursor >= scroll + visible {
        scroll = cursor + 1 - visible;
    }
    scroll.min(max_scroll)
}

/// A scrollbar grab in progress. The first press records the thumb offset so
/// dragging moves the thumb with the pointer instead of snapping it to the
/// cursor, which is what makes the drag feel smooth.
#[derive(Debug, Clone, Copy)]
pub struct ScrollbarDrag {
    vertical: bool,
    track_origin: u16,
    track_len: usize,
    thumb_len: usize,
    max_scroll: usize,
    grab_offset: usize,
    anchor_thumb_start: usize,
    anchor_scroll: usize,
}

impl ScrollbarDrag {
    /// Start dragging the scrollbar in `area`. `content_length` is the same
    /// value passed to `ScrollbarState`, i.e. the maximum scroll offset.
    /// Returns the drag state and the scroll position for the press.
    pub fn begin(
        area: Rect,
        vertical: bool,
        content_length: usize,
        position: usize,
        pointer: Position,
    ) -> Option<(Self, usize)> {
        // `scrollbar()` draws no arrows, so the track is the whole area.
        let track_len = if vertical { area.height } else { area.width } as usize;
        if content_length == 0 || track_len == 0 {
            return None;
        }

        let origin = if vertical { area.y } else { area.x };
        let pointer_index = if vertical { pointer.y } else { pointer.x };
        let index = pointer_index
            .saturating_sub(origin)
            .min(track_len as u16 - 1) as usize;
        let (thumb_start, thumb_len) = thumb_geometry(track_len, content_length, position);

        let (grab_offset, start, scroll) =
            if index >= thumb_start && index < thumb_start + thumb_len {
                // Grabbing the thumb itself must not move the content at all.
                (
                    index - thumb_start,
                    thumb_start,
                    position.min(content_length),
                )
            } else {
                let start = index
                    .saturating_sub(thumb_len / 2)
                    .min(track_len.saturating_sub(thumb_len));
                (
                    index - start,
                    start,
                    scroll_for(start, track_len, thumb_len, content_length),
                )
            };

        let drag = Self {
            vertical,
            track_origin: origin,
            track_len,
            thumb_len,
            max_scroll: content_length,
            grab_offset,
            anchor_thumb_start: start,
            anchor_scroll: scroll,
        };
        Some((drag, scroll))
    }

    pub fn scroll_at(&self, pointer: Position) -> usize {
        let pointer_index = if self.vertical { pointer.y } else { pointer.x };
        let index = pointer_index
            .saturating_sub(self.track_origin)
            .min(self.track_len as u16 - 1) as usize;
        let start = index
            .saturating_sub(self.grab_offset)
            .min(self.track_len.saturating_sub(self.thumb_len));
        if start == self.anchor_thumb_start {
            return self.anchor_scroll;
        }

        // Map each side of the grab point linearly, so the press never jumps
        // and the thumb still reaches both ends of the track.
        let travel = self.track_len.saturating_sub(self.thumb_len);
        if start < self.anchor_thumb_start {
            if self.anchor_thumb_start == 0 {
                0
            } else {
                round_div(
                    self.anchor_scroll.saturating_mul(start),
                    self.anchor_thumb_start,
                )
            }
        } else {
            let remaining = travel.saturating_sub(self.anchor_thumb_start);
            if remaining == 0 {
                self.max_scroll
            } else {
                self.anchor_scroll
                    + round_div(
                        (self.max_scroll - self.anchor_scroll)
                            .saturating_mul(start - self.anchor_thumb_start),
                        remaining,
                    )
            }
        }
    }
}

fn scroll_for(thumb_start: usize, track_len: usize, thumb_len: usize, max_scroll: usize) -> usize {
    let travel = track_len.saturating_sub(thumb_len);
    if travel == 0 {
        return 0;
    }
    round_div(thumb_start.saturating_mul(max_scroll), travel)
}

fn round_div(numerator: usize, denominator: usize) -> usize {
    if denominator == 0 {
        return 0;
    }
    (numerator + denominator / 2) / denominator
}

/// Mirror ratatui's `Scrollbar` thumb geometry so the hit target matches what
/// is drawn on screen. The widget uses the track length as the viewport unless
/// `viewport_content_length` is set, which this app never does.
fn thumb_geometry(track_len: usize, content_length: usize, position: usize) -> (usize, usize) {
    let viewport = track_len;
    let max_position = content_length.saturating_sub(1);
    let start_position = position.min(max_position);
    let max_viewport_position = max_position.saturating_add(viewport);
    if max_viewport_position == 0 {
        return (0, track_len);
    }
    let thumb_len =
        round_div(viewport.saturating_mul(track_len), max_viewport_position).clamp(1, track_len);
    let thumb_start = round_div(
        start_position.saturating_mul(track_len),
        max_viewport_position,
    )
    .clamp(0, track_len.saturating_sub(thumb_len));
    (thumb_start, thumb_len)
}

/// Nerd Font glyphs — private-use codepoints, so every machine with a patched
/// font shows the exact same icons (no OS-dependent emoji artwork). The
/// arrows are narrow-class characters that render one cell wide everywhere.
pub mod icons {
    pub const FOLDER: &str = "\u{f07b}";
    pub const FOLDER_OPEN: &str = "\u{f07c}";
    pub const FILE: &str = "\u{f15b}";
    pub const CPP: &str = "\u{e61d}";
    pub const PYTHON: &str = "\u{e606}";
    pub const JAVA: &str = "\u{e256}";
    pub const RUST: &str = "\u{e7a8}";
    pub const JSON: &str = "\u{e60b}";
    pub const TEXT: &str = "\u{f15c}";
    /// Test data: `.in`, `.out`, `.ans`.
    pub const DATA: &str = "\u{f0ce}";
    pub const EXPANDED: &str = "-";
    pub const COLLAPSED: &str = "+";
    pub const PROMPT: &str = "❯";
    /// The focus rail at the left edge of the selected case or pane.
    pub const RAIL: &str = "▎";
}

/// A one-cell scrollbar: no track, a thin thumb in the border colour —
/// present when you look for it, invisible when you don't.
pub fn scrollbar(orientation: ScrollbarOrientation) -> Scrollbar<'static> {
    let thumb = if orientation.is_vertical() {
        "┃"
    } else {
        "━"
    };
    Scrollbar::new(orientation)
        .thumb_symbol(thumb)
        .track_symbol(None)
        .begin_symbol(None)
        .end_symbol(None)
        .thumb_style(Style::new().fg(theme::border()))
}

pub struct VisualState {
    pub rect: Rect,
}

impl VisualState {
    pub fn new() -> Self {
        Self {
            rect: Rect::default(),
        }
    }

    pub fn check_hover(&self, mouse: &MouseEvent) -> bool {
        self.rect.width > 0
            && self.rect.height > 0
            && mouse.column >= self.rect.x
            && mouse.column < self.rect.x + self.rect.width
            && mouse.row >= self.rect.y
            && mouse.row < self.rect.y + self.rect.height
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dragging_a_scrollbar_thumb_moves_relative_to_the_grab_point() {
        let area = Rect::new(0, 0, 1, 22);
        let content_length = 100;
        let position = 50;
        let (thumb_start, thumb_len) = thumb_geometry(22, content_length, position);
        let grab_y = thumb_start + thumb_len / 2;
        let (drag, scroll) = ScrollbarDrag::begin(
            area,
            true,
            content_length,
            position,
            Position::new(0, grab_y as u16),
        )
        .expect("the scrollbar must accept a grab");

        assert_eq!(scroll, position, "grabbing the thumb must not jump");
        let moved = drag.scroll_at(Position::new(0, (grab_y + 1) as u16));
        assert!(
            moved > scroll,
            "dragging one row must move the scroll position"
        );
        assert!(
            moved - scroll < 10,
            "one row must be a small step, not a jump: {moved} vs {scroll}"
        );
    }
    #[test]
    fn a_small_drag_on_a_long_scrollbar_is_a_small_step() {
        let area = Rect::new(0, 0, 1, 22);
        let content_length = 19_980;
        let position = 10_000;
        let (thumb_start, thumb_len) = thumb_geometry(22, content_length, position);
        let grab_y = thumb_start + thumb_len / 2;
        let (drag, scroll) = ScrollbarDrag::begin(
            area,
            true,
            content_length,
            position,
            Position::new(0, grab_y as u16),
        )
        .expect("the scrollbar must accept a grab");

        assert_eq!(scroll, position);
        let moved = drag.scroll_at(Position::new(0, (grab_y + 1) as u16));
        assert!(
            moved > scroll && moved - scroll < 2_000,
            "one row must not jump across the file: {moved} vs {scroll}"
        );
    }
}
