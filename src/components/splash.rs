use ratatui::{
    Frame,
    layout::{
        Alignment,
        Constraint::{Fill, Length},
        Layout, Rect,
    },
    style::{Color, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

use crate::components::theme;

pub fn render(frame: &mut Frame, area: Rect, spinner: &str, tick: i64, port: u16) {
    // The gradient breathes slowly (≈13s cycle at the 110ms tick).
    let phase = tick.rem_euclid(120) as f32 / 120.0;
    let logo = logo_lines(phase);

    let chunks =
        Layout::vertical([Fill(1), Length(logo.len() as u16), Length(7), Fill(1)]).split(area);
    frame.render_widget(Paragraph::new(logo).alignment(Alignment::Center), chunks[1]);

    let version = concat!("v", env!("CARGO_PKG_VERSION"));
    let lines = vec![
        Line::styled(
            "the competitive programming helper",
            Style::new().fg(theme::TEXT).bold(),
        ),
        Line::default(),
        Line::styled(
            "compile · run · submit — right from your editor",
            Style::new().fg(theme::MUTED).italic(),
        ),
        Line::default(),
        Line::default(),
        Line::from(vec![
            Span::styled(format!("{spinner} "), Style::new().fg(theme::CYAN)),
            Span::styled(
                format!("listening on 127.0.0.1:{port}"),
                Style::new().fg(theme::CYAN),
            ),
        ]),
        Line::from(vec![
            Span::styled(version, Style::new().fg(theme::BORDER)),
            Span::styled(
                " — send a problem with Competitive Companion",
                Style::new().fg(theme::MUTED),
            ),
        ]),
    ];

    frame.render_widget(
        Paragraph::new(lines).alignment(Alignment::Center),
        chunks[2],
    );
}

/// ANSI Shadow style "ZCH": fat 3D block letters.
const LOGO: [&str; 6] = [
    "███████╗ ██████╗██╗  ██╗",
    "╚══███╔╝██╔════╝██║  ██║",
    "  ███╔╝ ██║     ███████║",
    " ███╔╝  ██║     ██╔══██║",
    "███████╗╚██████╗██║  ██║",
    "╚══════╝ ╚═════╝╚═╝  ╚═╝",
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

/// Vibrant Catppuccin gradient stops: blue → mauve → pink → peach.
const STOPS: [[u8; 3]; 4] = [
    [0x89, 0xb4, 0xfa],
    [0xcb, 0xa6, 0xf7],
    [0xf5, 0xc2, 0xe7],
    [0xfa, 0xb3, 0x87],
];

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
    diagonal + 0.12 * (phase * std::f32::consts::TAU).sin()
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
                    render(frame, area, "⠋", 0, 10043);
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
            assert!(text.contains('█'), "logo missing at {width}x{height}");
            assert!(text.contains("listening on 127.0.0.1:10043"));
        }
    }

    #[test]
    fn gradient_runs_through_the_vibrant_stops() {
        assert_eq!(gradient(0.0), Color::Rgb(0x89, 0xb4, 0xfa));
        assert_eq!(gradient(1.0), Color::Rgb(0xfa, 0xb3, 0x87));
        assert_eq!(gradient(0.5), Color::Rgb(0xe0, 0xb4, 0xef));
        assert_eq!(gradient(-1.0), gradient(0.0), "clamps below");
        assert_eq!(gradient(2.0), gradient(1.0), "clamps above");
    }
}
