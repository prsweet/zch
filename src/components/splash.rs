use ratatui::{
    Frame,
    layout::{
        Alignment,
        Constraint::{Fill, Length},
        Layout, Rect,
    },
    style::{Color, Style},
    text::{Line, Span, Text},
    widgets::Paragraph,
};

use crate::components::{theme, ui};

pub fn render(frame: &mut Frame, area: Rect, tick: i64, port: u16) {
    // The mark breathes slowly (≈13s cycle at the 110ms tick).
    let phase = tick.rem_euclid(120) as f32 / 120.0;

    let mut lines: Vec<Line> = logo_lines(phase);
    lines.push(Line::default());
    lines.push(Line::styled(
        ui::spaced("competitive programming"),
        Style::new().fg(theme::MUTED()).bold(),
    ));
    lines.push(Line::default());
    lines.push(Line::styled(
        "─".repeat(38),
        Style::new().fg(theme::LINE_STRONG()),
    ));
    lines.push(Line::default());
    lines.push(Line::from(vec![
        ui::dot(theme::GREEN()),
        Span::styled(
            format!(" listening on 127.0.0.1:{port}"),
            Style::new().fg(theme::FAINT()),
        ),
    ]));
    lines.push(Line::styled(
        "send a problem with Competitive Companion",
        Style::new().fg(theme::TEXT()),
    ));
    lines.push(Line::default());
    lines.push(Line::from(vec![
        ui::key("r"),
        Span::styled("un       ", Style::new().fg(theme::MUTED())),
        ui::key("s"),
        Span::styled("ubmit    ", Style::new().fg(theme::MUTED())),
        ui::key("h"),
        Span::styled("elp      ", Style::new().fg(theme::MUTED())),
        ui::key("e"),
        Span::styled("dit      ", Style::new().fg(theme::MUTED())),
    ]));
    lines.push(Line::from(vec![
        ui::key("a"),
        Span::styled("dd       ", Style::new().fg(theme::MUTED())),
        ui::key("c"),
        Span::styled("onfig    ", Style::new().fg(theme::MUTED())),
        ui::key("d"),
        Span::styled("irectory ", Style::new().fg(theme::MUTED())),
        ui::key("q"),
        Span::styled("uit      ", Style::new().fg(theme::MUTED())),
    ]));

    let height = lines.len() as u16;
    let chunks = Layout::vertical([Fill(1), Length(height), Fill(1)]).split(area);
    frame.render_widget(
        Paragraph::new(Text::from(lines))
            .alignment(Alignment::Center)
            .style(Style::new().bg(theme::BG())),
        chunks[1],
    );

    let version = concat!("v", env!("CARGO_PKG_VERSION"));
    if area.width > 40 && area.height > 2 {
        frame.render_widget(
            Paragraph::new(Span::styled(version, Style::new().fg(theme::FAINT())))
                .alignment(Alignment::Right)
                .style(Style::new().bg(theme::BG())),
            Rect {
                x: area.x + ui::GUTTER,
                width: area.width.saturating_sub(ui::GUTTER * 2),
                y: area.bottom() - 1,
                height: 1,
            },
        );
    }
}

/// A clean five-row slab mark: wide enough to read, quiet enough for a
/// dashboard that is usually open all day.
const LOGO: [&str; 5] = [
    "███████  ██████  ██   ██",
    "   ███  ██       ██   ██",
    "  ███   ██       ███████",
    " ███    ██       ██   ██",
    "███████  ██████  ██   ██",
];

fn logo_lines(phase: f32) -> Vec<Line<'static>> {
    let (rows, cols) = (LOGO.len(), LOGO[0].chars().count());
    LOGO.iter()
        .enumerate()
        .map(|(row, art)| {
            Line::from(
                art.chars()
                    .enumerate()
                    .map(|(col, ch)| {
                        let t = cell_t(col, row, cols, rows, phase);
                        Span::styled(ch.to_string(), Style::new().fg(gradient(t)).bold())
                    })
                    .collect::<Vec<_>>(),
            )
        })
        .collect()
}

/// A cool duotone: periwinkle into soft lilac. No rainbow, no noise.
const STOPS: [[u8; 3]; 3] = [[0x6f, 0x9e, 0xdd], [0x8c, 0xb8, 0xff], [0xc8, 0xa9, 0xff]];

fn gradient(t: f32) -> Color {
    let t = t.clamp(0.0, 1.0) * (STOPS.len() - 1) as f32;
    let index = (t as usize).min(STOPS.len() - 2);
    let local = t - index as f32;
    let mix = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * local).round() as u8;
    let (low, high) = (STOPS[index], STOPS[index + 1]);
    Color::Rgb(
        mix(low[0], high[0]),
        mix(low[1], high[1]),
        mix(low[2], high[2]),
    )
}

fn cell_t(col: usize, row: usize, cols: usize, rows: usize, phase: f32) -> f32 {
    let diagonal = 0.55 * row as f32 / (rows - 1) as f32 + 0.45 * col as f32 / (cols - 1) as f32;
    diagonal + 0.10 * (phase * std::f32::consts::TAU).sin()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend};

    #[test]
    fn splash_renders_on_narrow_and_wide_terminals() {
        for (width, height) in [(80, 24), (120, 40), (200, 50)] {
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal
                .draw(|frame| {
                    let area = frame.area();
                    render(frame, area, 0, 10043);
                })
                .unwrap();
            let buffer = terminal.backend().buffer();
            let mut text = String::new();
            for y in 0..buffer.area.height {
                for x in 0..buffer.area.width {
                    text.push_str(buffer[(x, y)].symbol());
                }
                text.push('\n');
            }
            if std::env::var("ZCH_DUMP").is_ok() {
                println!("{width}x{height}\n{text}");
            }
            assert!(text.contains('█'), "logo missing at {width}x{height}");
            assert!(text.contains("listening on"));
            assert!(text.contains("127.0.0.1:10043"));
        }
    }

    #[test]
    fn gradient_runs_through_the_duotone_stops() {
        assert_eq!(gradient(0.0), Color::Rgb(0x6f, 0x9e, 0xdd));
        assert_eq!(gradient(1.0), Color::Rgb(0xc8, 0xa9, 0xff));
        assert_eq!(gradient(-1.0), gradient(0.0), "clamps below");
        assert_eq!(gradient(2.0), gradient(1.0), "clamps above");
    }
}
