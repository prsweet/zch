//! Shared visual language.
//!
//! The canvas is pure black, like the editor beside it. Structure comes from
//! whitespace first, a single hairline where two regions meet, and one quiet
//! bar over a sheet — never boxes inside boxes. Text carries
//! three weights: `text` for what you read, `muted` for labels, `faint` for
//! metadata. Colour marks exceptions (a verdict, the focus), not structure.

use std::cell::RefCell;

use ratatui::{
    Frame,
    buffer::Buffer,
    layout::{Alignment, Constraint, Layout, Margin, Position, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Clear, Paragraph, Widget},
};
use unicode_width::UnicodeWidthStr;

use super::theme;

/// Horizontal page gutter.
pub const GUTTER: u16 = 2;

pub fn canvas(frame: &mut Frame, area: Rect) {
    frame.render_widget(Block::default().style(Style::new().bg(theme::bg())), area);
}

pub fn fill(frame: &mut Frame, area: Rect, color: Color) {
    frame.render_widget(Block::default().style(Style::new().bg(color)), area);
}

pub fn fill_buf(buf: &mut Buffer, area: Rect, color: Color) {
    buf.set_style(area, Style::new().bg(color));
}

/// A one-row rule.
pub fn hairline(frame: &mut Frame, area: Rect, color: Color) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    frame.render_widget(
        Paragraph::new(Span::styled(
            "─".repeat(area.width as usize),
            Style::new().fg(color),
        )),
        Rect { height: 1, ..area },
    );
}

/// `left` and `right` on one row, pinned to opposite edges. The right side
/// wins a collision, so status stays readable.
pub fn row(frame: &mut Frame, area: Rect, left: Line<'static>, right: Line<'static>) {
    row_buf(frame.buffer_mut(), area, left, right);
}

pub fn row_buf(buf: &mut Buffer, area: Rect, left: Line<'static>, right: Line<'static>) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let right_width = right.width().min(area.width as usize) as u16;
    if right_width > 0 {
        Paragraph::new(right).alignment(Alignment::Right).render(
            Rect {
                x: area.x + area.width - right_width,
                width: right_width,
                height: 1,
                ..area
            },
            buf,
        );
    }
    if area.width > right_width {
        let gap = if right_width > 0 {
            2.min(area.width - right_width)
        } else {
            0
        };
        Paragraph::new(left).render(
            Rect {
                width: area.width - right_width - gap,
                height: 1,
                ..area
            },
            buf,
        );
    }
}

/// A keycap. Square brackets read in every font and keep the console bar
/// compact without relying on colour alone.
pub fn key(key: &str) -> Span<'static> {
    Span::styled(format!("[{key}]"), Style::new().fg(theme::accent()).bold())
}

/// Truncates to `max` display cells, keeping the start.
pub fn truncate(text: &str, max: usize) -> String {
    if max == 0 {
        return String::new();
    }
    if text.width() <= max {
        return text.to_string();
    }
    let budget = max.saturating_sub(1);
    let mut used = 0;
    let mut out = String::new();
    for ch in text.chars() {
        let width = UnicodeWidthStr::width(ch.to_string().as_str());
        if used + width > budget {
            break;
        }
        used += width;
        out.push(ch);
    }
    out.push('…');
    out
}

/// Text the screen had to cut short, and where it was drawn: a double-click
/// there opens it whole. Only cut text registers, so a double-click on text
/// that fits does nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reveal {
    pub area: Rect,
    pub title: String,
    pub text: String,
}

// Rebuilt every frame by whatever draws; one per thread so parallel tests
// never see each other's.
thread_local! {
    static REVEALS: RefCell<Vec<Reveal>> = const { RefCell::new(Vec::new()) };
}

/// Forget last frame's cut text: call before drawing, and again before a
/// sheet so nothing under it stays clickable.
pub fn clear_reveals() {
    REVEALS.with(|reveals| reveals.borrow_mut().clear());
}

/// Offers `text` whole at `area`, where it was drawn cut.
pub fn reveal(area: Rect, title: &str, text: &str) {
    if area.is_empty() {
        return;
    }
    REVEALS.with(|reveals| {
        reveals.borrow_mut().push(Reveal {
            area,
            title: title.to_string(),
            text: text.to_string(),
        });
    });
}

/// The cut text drawn at `position`, topmost first.
pub fn reveal_at(position: Position) -> Option<Reveal> {
    REVEALS.with(|reveals| {
        reveals
            .borrow()
            .iter()
            .rev()
            .find(|reveal| reveal.area.contains(position))
            .cloned()
    })
}

/// [`truncate`], offering the whole text at `area` when it had to be cut.
pub fn truncate_revealing(text: &str, max: usize, area: Rect, title: &str) -> String {
    let shown = truncate(text, max);
    if shown != text {
        reveal(area, title, text);
    }
    shown
}

/// Dim everything already drawn: the backdrop behind a sheet. Dimming is
/// a courtesy, not the cue — the border and the hint row say "dialog".
pub fn dim(frame: &mut Frame, area: Rect) {
    let buf = frame.buffer_mut();
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            let cell = &mut buf[(x, y)];
            cell.modifier.insert(Modifier::DIM);
        }
    }
}

/// A centred sheet, like an open scroll: a sheet of card colour hung from
/// a thick bar, the title under it, then the padded body. No other edges.
/// `opened` is the open transition in `[0, 1]`: the bar and title fade up
/// out of the black so a dialog arrives instead of popping.
pub fn sheet(
    frame: &mut Frame,
    area: Rect,
    title: &str,
    width: u16,
    height: u16,
    opened: f32,
) -> Rect {
    sheet_with(frame, area, title, theme::p().accent, width, height, opened)
}

/// [`sheet`] with a bar tone, for destructive dialogs.
pub fn sheet_with(
    frame: &mut Frame,
    area: Rect,
    title: &str,
    bar: theme::Rgb,
    width: u16,
    height: u16,
    opened: f32,
) -> Rect {
    dim(frame, area);
    let width = width
        .min(area.width.saturating_sub(4))
        .max(20.min(area.width));
    let height = height
        .min(area.height.saturating_sub(2))
        .max(5.min(area.height));
    // Slightly above centre reads as "in front", not "sinking".
    let free = area.height.saturating_sub(height);
    let popup = Rect {
        x: area.x + area.width.saturating_sub(width) / 2,
        y: area.y + free * 2 / 5,
        width,
        height,
    };
    frame.render_widget(Clear, popup);
    let p = theme::p();
    frame.render_widget(
        Block::default().style(Style::new().bg(theme::card()).fg(theme::text())),
        popup,
    );
    // The bar: the upper half of the first row, the card showing under it.
    let buf = frame.buffer_mut();
    for x in popup.left()..popup.right() {
        buf[(x, popup.y)]
            .set_symbol("▀")
            .set_fg(theme::paint(theme::mix(p.card, bar, opened)));
    }
    let [_, title_row, _, body] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Fill(1),
    ])
    .areas(popup.inner(Margin::new(SHEET_PAD, 0)));
    frame.render_widget(
        Span::styled(
            truncate(title, title_row.width as usize),
            Style::new()
                .fg(theme::blend(p.faint, p.text, opened))
                .bold(),
        ),
        title_row,
    );
    // A blank row under the body, like the one under the title.
    Rect {
        height: body.height.saturating_sub(1),
        ..body
    }
}

/// Space left and right of a sheet's title and body.
const SHEET_PAD: u16 = 3;

/// A label/value row in a sheet: `label` muted in a fixed column.
pub fn field(label: &str, label_width: usize, value: Vec<Span<'static>>) -> Line<'static> {
    let mut spans = vec![Span::styled(
        format!("{label:<label_width$}"),
        Style::new().fg(theme::muted()),
    )];
    spans.extend(value);
    Line::from(spans)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_cut_text_can_be_revealed() {
        clear_reveals();
        let area = Rect::new(4, 2, 10, 1);
        assert_eq!(truncate_revealing("fits", 10, area, "Error"), "fits");
        assert_eq!(reveal_at(Position::new(5, 2)), None, "nothing was cut");

        let long = "this one does not fit";
        assert_eq!(truncate_revealing(long, 10, area, "Error"), "this one …");
        let reveal = reveal_at(Position::new(5, 2)).expect("cut text is offered");
        assert_eq!(
            (reveal.title.as_str(), reveal.text.as_str()),
            ("Error", long)
        );
        assert_eq!(
            reveal_at(Position::new(5, 3)),
            None,
            "only where it was drawn"
        );
        clear_reveals();
        assert_eq!(reveal_at(Position::new(5, 2)), None);
    }

    #[test]
    fn truncation_never_exceeds_display_cells() {
        for text in ["hello", "コンテスト", "aコンテストb", "👩‍💻 code"] {
            for max in 1..12 {
                let short = truncate(text, max);
                assert!(
                    short.width() <= max,
                    "{text:?} at {max} produced {short:?} ({} cells)",
                    short.width()
                );
            }
        }
    }
}
