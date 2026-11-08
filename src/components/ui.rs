//! Shared visual language.
//!
//! The canvas is pure black, like the editor beside it. Structure comes from
//! whitespace first, a single hairline where two regions meet, and one quiet
//! rounded border around a sheet — never boxes inside boxes. Text carries
//! three weights: `text` for what you read, `muted` for labels, `faint` for
//! metadata. Colour marks exceptions (a verdict, the focus), not structure.

use ratatui::{
    Frame,
    buffer::Buffer,
    layout::{Alignment, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Padding, Paragraph, Widget},
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

/// A centred sheet: rounded border, title in the top edge, padded body.
/// `opened` is the open transition in `[0, 1]`: the border and title fade
/// up out of the black so a dialog arrives instead of popping.
pub fn sheet(
    frame: &mut Frame,
    area: Rect,
    title: &str,
    width: u16,
    height: u16,
    opened: f32,
) -> Rect {
    sheet_with(frame, area, title, theme::p().border, width, height, opened)
}

/// [`sheet`] with a border tone, for destructive dialogs.
pub fn sheet_with(
    frame: &mut Frame,
    area: Rect,
    title: &str,
    border: theme::Rgb,
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
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(theme::blend(theme::p().bg, border, opened)))
        .title(Line::from(Span::styled(
            format!(" {title} "),
            Style::new()
                .fg(theme::blend(theme::p().faint, theme::p().text, opened))
                .bold(),
        )))
        .style(Style::new().bg(theme::bg()).fg(theme::text()))
        .padding(Padding::new(2, 2, 1, 1));
    let body = block.inner(popup);
    frame.render_widget(block, popup);
    body
}

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
