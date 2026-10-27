use ratatui::{
    Frame,
    layout::{Alignment, Rect},
    style::Style,
    text::{Line, Span},
    widgets::Paragraph,
};

use crate::components::theme;

/// The listener is ready, not loading. Keep this screen still so the next
/// action, rather than an idle animation, owns the user's attention.
pub fn render(frame: &mut Frame, area: Rect, _spinner: &str, _tick: i64, port: u16) {
    if area.is_empty() {
        return;
    }
    let full = area.width >= 56 && area.height >= 12;
    let lines = if full {
        vec![
            Line::from(vec![
                Span::styled(
                    " ZCH ",
                    Style::new()
                        .fg(theme::ACCENT())
                        .bg(theme::ELEVATED())
                        .bold(),
                ),
                Span::styled("  COMPETITIVE WORKSPACE", Style::new().fg(theme::MUTED())),
            ]),
            Line::default(),
            Line::styled(
                "Ready for your next problem",
                Style::new().fg(theme::TEXT()).bold(),
            ),
            Line::styled(
                "Your samples, verdicts, and submissions. In one place.",
                Style::new().fg(theme::MUTED()),
            ),
            Line::default(),
            step("01", "Open a problem in your browser"),
            step("02", "Send it with Competitive Companion"),
            step("03", "Write your solution, then press [r] to test"),
            Line::default(),
            Line::from(vec![
                Span::styled("● ", Style::new().fg(theme::GREEN())),
                Span::styled("Listener ready", Style::new().fg(theme::TEXT())),
                Span::styled(
                    format!("  ·  127.0.0.1:{port}"),
                    Style::new().fg(theme::MUTED()),
                ),
            ]),
            Line::default(),
            Line::styled(
                concat!("ZCH v", env!("CARGO_PKG_VERSION")),
                Style::new().fg(theme::MUTED()),
            ),
        ]
    } else {
        let mut lines = Vec::new();
        if area.height >= 6 {
            lines.push(Line::styled(
                if area.width >= 30 {
                    "Ready for your next problem"
                } else {
                    "Ready for a problem"
                },
                Style::new().fg(theme::TEXT()).bold(),
            ));
            lines.push(Line::default());
        }
        lines.push(Line::styled(
            "Send a problem with",
            Style::new().fg(theme::MUTED()),
        ));
        lines.push(Line::styled(
            "Competitive Companion",
            Style::new().fg(theme::TEXT()).bold(),
        ));
        lines.push(Line::styled(
            format!("● 127.0.0.1:{port}"),
            Style::new().fg(theme::GREEN()),
        ));
        lines
    };
    let content_height = (lines.len() as u16).min(area.height);
    let content = Rect {
        y: area.y + area.height.saturating_sub(content_height) / 2,
        height: content_height,
        ..area
    };
    frame.render_widget(Paragraph::new(lines).alignment(Alignment::Center), content);
}

fn step(number: &'static str, text: &'static str) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{number}  "), Style::new().fg(theme::ACCENT())),
        Span::styled(format!("{text:<43}"), Style::new().fg(theme::TEXT())),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend};

    fn screen(width: u16, height: u16, tick: i64) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| render(frame, frame.area(), "⠋", tick, 10043))
            .unwrap();
        let buffer = terminal.backend().buffer();
        (0..height)
            .map(|y| {
                (0..width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn welcome_prioritizes_the_next_action_at_every_size() {
        for (width, height) in [(24, 8), (32, 10), (40, 12), (80, 24), (120, 40), (200, 50)] {
            let text = screen(width, height, 0);
            for expected in ["127.0.0.1:10043", "Competitive Companion"] {
                assert!(
                    text.contains(expected),
                    "{expected} missing at {width}x{height}:\n{text}"
                );
            }
            if width >= 56 && height >= 14 {
                assert!(text.contains("Ready for your next problem"));
                assert!(text.contains("01"));
                assert!(text.contains("02"));
                assert!(text.contains("03"));
            }
        }
    }

    #[test]
    fn idle_listener_does_not_animate() {
        assert_eq!(screen(80, 24, 0), screen(80, 24, 60));
    }
}
