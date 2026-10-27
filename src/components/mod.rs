use ratatui::{
    crossterm::event::MouseEvent,
    layout::{Position, Rect},
    style::{Color, Style},
    widgets::{Block, BorderType, Borders, Padding, Scrollbar, ScrollbarOrientation},
};

pub mod config;
pub mod dir_terminal;
pub mod splash;
pub mod tc_list;
pub mod testcase;

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
        let area_len = if vertical { area.height } else { area.width } as usize;
        let begin_len = 1.min(area_len);
        let end_len = 1.min(area_len.saturating_sub(begin_len));
        let track_len = area_len.saturating_sub(begin_len + end_len);
        if content_length == 0 || track_len == 0 {
            return None;
        }

        let origin = if vertical { area.y } else { area.x } + begin_len as u16;
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

/// Active palette. Truecolor only, so the UI does not follow the terminal theme.
/// `apply` selects Mocha or prsweet's favourite for this thread.
#[allow(non_snake_case)]
pub mod theme {
    use std::cell::Cell;

    use ratatui::style::Color;

    use crate::core::AppTheme;

    #[derive(Clone, Copy)]
    struct Palette {
        base: Color,
        surface: Color,
        elevated: Color,
        border: Color,
        muted: Color,
        text: Color,
        accent: Color,
        green: Color,
        red: Color,
        yellow: Color,
        orange: Color,
        purple: Color,
        cyan: Color,
        /// Section titles. Mocha keeps these quiet; prsweet uses the syntax title hue.
        title: Color,
    }

    /// Catppuccin Mocha. Status colors sit in one lightness band on a dark ground.
    const MOCHA: Palette = Palette {
        base: Color::Rgb(0x18, 0x18, 0x25),
        surface: Color::Rgb(0x1e, 0x1e, 0x2e),
        elevated: Color::Rgb(0x31, 0x32, 0x44),
        border: Color::Rgb(0x45, 0x47, 0x5a),
        muted: Color::Rgb(0x93, 0x99, 0xb2),
        text: Color::Rgb(0xcd, 0xd6, 0xf4),
        accent: Color::Rgb(0x89, 0xb4, 0xfa),
        green: Color::Rgb(0xa6, 0xe3, 0xa1),
        red: Color::Rgb(0xf3, 0x8b, 0xa8),
        yellow: Color::Rgb(0xf9, 0xe2, 0xaf),
        orange: Color::Rgb(0xfa, 0xb3, 0x87),
        purple: Color::Rgb(0xcb, 0xa6, 0xf7),
        cyan: Color::Rgb(0x94, 0xe2, 0xd5),
        title: Color::Rgb(0xa6, 0xad, 0xc8),
    };

    /// Raw syntax colors from the prsweet Zed theme, the same ones in the editor:
    /// pink keywords, lime types, teal functions, gold strings, cyan operators.
    const PRSWEET: Palette = Palette {
        base: Color::Rgb(0x15, 0x19, 0x23),
        surface: Color::Rgb(0x1c, 0x21, 0x2e),
        elevated: Color::Rgb(0x2a, 0x32, 0x42),
        border: Color::Rgb(0x3a, 0x41, 0x50),
        muted: Color::Rgb(0x9c, 0xa5, 0xb8),
        text: Color::Rgb(0xe8, 0xee, 0xfc),
        accent: Color::Rgb(0x7d, 0xcf, 0xff),
        green: Color::Rgb(0x9e, 0xce, 0x6a),
        red: Color::Rgb(0xff, 0x4d, 0x6d),
        yellow: Color::Rgb(0xe0, 0xaf, 0x68),
        orange: Color::Rgb(0xff, 0x9e, 0x64),
        purple: Color::Rgb(0xbb, 0x9a, 0xf7),
        cyan: Color::Rgb(0x4f, 0xd6, 0xbe),
        title: Color::Rgb(0xb4, 0xbe, 0xd3),
    };

    thread_local! {
        static CURRENT: Cell<AppTheme> = const { Cell::new(AppTheme::Mocha) };
    }

    pub fn apply(theme: AppTheme) {
        CURRENT.with(|current| current.set(theme));
    }

    fn palette() -> Palette {
        CURRENT.with(|current| match current.get() {
            AppTheme::Mocha => MOCHA,
            AppTheme::Prsweet => PRSWEET,
        })
    }

    pub fn BASE() -> Color {
        palette().base
    }
    pub fn SURFACE() -> Color {
        palette().surface
    }
    pub fn ELEVATED() -> Color {
        palette().elevated
    }
    pub fn BORDER() -> Color {
        palette().border
    }
    pub fn MUTED() -> Color {
        palette().muted
    }
    pub fn TEXT() -> Color {
        palette().text
    }
    pub fn ACCENT() -> Color {
        palette().accent
    }
    pub fn GREEN() -> Color {
        palette().green
    }
    pub fn RED() -> Color {
        palette().red
    }
    pub fn YELLOW() -> Color {
        palette().yellow
    }
    pub fn ORANGE() -> Color {
        palette().orange
    }
    pub fn PURPLE() -> Color {
        palette().purple
    }
    pub fn CYAN() -> Color {
        palette().cyan
    }
    pub fn TITLE() -> Color {
        palette().title
    }
}

/// Nerd Font glyphs — private-use codepoints, so every machine with a patched
/// font shows the exact same icons (no OS-dependent emoji artwork).
pub mod icons {
    pub const FOLDER: &str = "\u{f07b}";
    pub const FILE: &str = "\u{f15b}";
}

pub fn block(title: Option<&str>) -> Block<'static> {
    let if_title = title.unwrap_or("");
    Block::default()
        .style(Style::new().fg(theme::TEXT()).bg(theme::SURFACE()))
        .title(if_title.to_string())
        .borders(Borders::ALL)
        .title_style(Style::new().fg(theme::TITLE()).bold())
        .border_style(theme::BORDER())
        .border_type(BorderType::Rounded)
        .padding(Padding::symmetric(1, 0))
}

/// A one-cell scrollbar with light track and heavy thumb glyphs.
pub fn scrollbar(orientation: ScrollbarOrientation) -> Scrollbar<'static> {
    let (thumb, track) = if orientation.is_vertical() {
        ("┃", "│")
    } else {
        ("━", "─")
    };
    Scrollbar::new(orientation)
        .thumb_symbol(thumb)
        .track_symbol(Some(track))
        .begin_symbol(None)
        .end_symbol(None)
        .style(theme::MUTED())
        .track_style(theme::BORDER())
}

#[derive(Debug, Clone)]
pub struct Message {
    pub color: Color,
    pub text: String,
}

impl Message {
    pub fn new(color: Color, text: impl Into<String>) -> Self {
        Self {
            color,
            text: text.into(),
        }
    }
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
        let (thumb_start, thumb_len) = thumb_geometry(20, content_length, position);
        let grab_y = 1 + thumb_start + thumb_len / 2;
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
        let (thumb_start, thumb_len) = thumb_geometry(20, content_length, position);
        let grab_y = 1 + thumb_start + thumb_len / 2;
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
