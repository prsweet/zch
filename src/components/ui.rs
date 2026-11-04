//! Shared visual language for the dashboard.
//!
//! Every surface in zch is built from a small number of primitives: a canvas,
//! raised panes, hairlines, quiet labels and one accent rail. Keeping those
//! here means the list, dialogs and chrome cannot drift apart.

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

/// Horizontal page gutter used by the main column.
pub const GUTTER: u16 = 2;

/// The app fills the whole terminal with its own canvas so it looks the same
/// in every terminal profile and on both dark and light desktop themes.
pub fn canvas(frame: &mut Frame, area: Rect) {
    frame.render_widget(Block::default().style(Style::new().bg(theme::BG())), area);
}

pub fn fill(frame: &mut Frame, area: Rect, color: Color) {
    frame.render_widget(Block::default().style(Style::new().bg(color)), area);
}

pub fn fill_buf(buf: &mut Buffer, area: Rect, color: Color) {
    buf.set_style(area, Style::new().bg(color));
}

pub fn hairline(frame: &mut Frame, area: Rect) {
    hairline_colored(frame, area, theme::LINE());
}

pub fn hairline_colored(frame: &mut Frame, area: Rect, color: Color) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    frame.render_widget(
        Paragraph::new(Span::styled(
            "─".repeat(area.width as usize),
            Style::new().fg(color),
        )),
        area,
    );
}

/// Renders `left` and `right` on one row, pinned to opposite edges. The right
/// side wins when they would collide, which keeps status text readable.
pub fn row(frame: &mut Frame, area: Rect, left: Line<'static>, right: Line<'static>) {
    row_buf(frame.buffer_mut(), area, left, right);
}

/// Buffer variant of [`row`], for widgets that draw into a scratch buffer.
pub fn row_buf(buf: &mut Buffer, area: Rect, left: Line<'static>, right: Line<'static>) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let right_width = right.width().min(area.width as usize) as u16;
    if right_width > 0 {
        Paragraph::new(right).alignment(Alignment::Right).render(
            Rect {
                x: area.x + area.width - right_width,
                y: area.y,
                width: right_width,
                height: 1,
            },
            buf,
        );
    }
    if area.width > right_width {
        // Keep a small breathing gap so a long left value never touches the
        // right-aligned status.
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

/// A keycap. Square brackets are legible in every font and keep the console
/// bar compact without relying on colour alone.
pub fn key(key: &str) -> Span<'static> {
    Span::styled(format!("[{key}]"), Style::new().fg(theme::ACCENT()).bold())
}

pub fn badge(text: &str, color: Color) -> Span<'static> {
    Span::styled(
        format!(" {text} "),
        Style::new().fg(Color::Black).bg(color).bold(),
    )
}

/// Letter-spacing implemented as real spaces, so it survives every terminal.
pub fn spaced(text: &str) -> String {
    text.chars()
        .flat_map(|ch| [ch, ' '])
        .collect::<String>()
        .trim_end()
        .to_string()
}

pub fn dot(color: Color) -> Span<'static> {
    Span::styled("●", Style::new().fg(color))
}

/// Truncates to `max` display cells, keeping the start of the string.
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

/// Truncates a path from the left so the most specific directory stays
/// visible: `/very/long/path/to/questions` → `…/path/to/questions`.
pub fn fit_path(text: &str, max: usize) -> String {
    if text.width() <= max {
        return text.to_string();
    }
    if max <= 1 {
        return "…".to_string();
    }
    let budget = max - 1;
    let mut used = 0;
    let mut out = Vec::new();
    for ch in text.chars().rev() {
        let width = UnicodeWidthStr::width(ch.to_string().as_str());
        if used + width > budget {
            break;
        }
        used += width;
        out.push(ch);
    }
    out.reverse();
    format!("…{}", out.into_iter().collect::<String>())
}

pub struct Sheet {
    /// Content region below the title rule.
    pub body: Rect,
}

/// A centered sheet used by every modal: dimmed backdrop, raised surface,
/// a compact title row and content below it. Key hints live in the console
/// bar, so sheets do not reserve a footer row.
pub fn sheet(
    frame: &mut Frame,
    area: Rect,
    title: &str,
    accent: Color,
    width: u16,
    height: u16,
) -> Sheet {
    dim(frame, area);
    let width = width.min(area.width.saturating_sub(2)).max(12);
    let height = height.min(area.height.saturating_sub(2)).max(6);
    let popup = Rect {
        x: area.x + area.width.saturating_sub(width) / 2,
        y: area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    };
    frame.render_widget(Clear, popup);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Plain)
        .border_style(Style::new().fg(theme::LINE_STRONG()))
        .style(Style::new().bg(theme::SURFACE_ALT()))
        .padding(Padding::new(1, 1, 0, 0));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    if inner.height >= 2 {
        let title_row = Rect { height: 1, ..inner };
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled("▎ ", Style::new().fg(accent).bold()),
                Span::styled(title.to_uppercase(), Style::new().fg(accent).bold()),
            ])),
            title_row,
        );
        frame.render_widget(
            Paragraph::new(Span::styled(
                "─".repeat(inner.width as usize),
                Style::new().fg(theme::LINE()),
            )),
            Rect {
                y: inner.y + 1,
                height: 1,
                ..inner
            },
        );
    }

    let body = Rect {
        x: inner.x,
        y: inner.y.saturating_add(2).min(inner.bottom()),
        width: inner.width,
        height: inner.height.saturating_sub(2),
    };
    Sheet { body }
}

pub fn dim(frame: &mut Frame, area: Rect) {
    frame.render_widget(
        Block::default().style(Style::new().add_modifier(Modifier::DIM)),
        area,
    );
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

    #[test]
    fn fitting_paths_keeps_the_specific_tail() {
        let path = "/very/long/path/to/questions";
        assert_eq!(fit_path(path, path.width()), path);
        assert_eq!(fit_path(path, 10), "…questions");
        assert!(fit_path(path, 10).width() <= 10);
        assert_eq!(fit_path("abc", 0), "…");
    }
}
