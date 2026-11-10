//! The empty state: nothing loaded yet.
//!
//! The one memorable thing in zch: a big ZCH in the theme's signature
//! gradient, drawn in heavy braille. The mark is a constant banner; drawing
//! only colours it, cell by cell, so the gradient runs smoothly and the
//! animation is a change of colour.
//!
//! At launch the mark draws itself in left to right; after that a slow band
//! of light drifts across it. Below sit one line of purpose and the
//! listener. The app's minimum terminal size is this screen's size, so the
//! mark is never cut or swapped for something smaller. Reduced motion shows
//! the finished screen, still.

use std::time::{Duration, Instant};

use ratatui::{
    Frame,
    buffer::Buffer,
    layout::{
        Alignment,
        Constraint::{Fill, Length},
        Layout, Rect,
    },
    style::Style,
    text::{Line, Span, Text},
    widgets::Paragraph,
};

use crate::components::{motion, theme};

/// The big mark, as it appears on screen, in braille: each cell holds a
/// 2 × 4 grid of near-square dots, so the Z gets a true diagonal; every
/// stroke is 8 dots thick and the corners are square. Dots are separate by design, so
/// a tall line height cannot open a seam in them the way it does in block
/// glyphs. Drawing only colours it.
const BANNER: [&str; BIG_ROWS as usize] = [
    "⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿    ⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿    ⣿⣿⣿⣿   ⣿⣿⣿⣿",
    "⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿    ⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿    ⣿⣿⣿⣿   ⣿⣿⣿⣿",
    "      ⢀⣴⣿⣿⠟⠁    ⣿⣿⣿⣿           ⣿⣿⣿⣿⣤⣤⣤⣿⣿⣿⣿",
    "   ⢀⣴⣿⣿⠟⠁       ⣿⣿⣿⣿           ⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿",
    "⢀⣴⣿⣿⠟⠁          ⣿⣿⣿⣿           ⣿⣿⣿⣿⠛⠛⠛⣿⣿⣿⣿",
    "⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿    ⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿    ⣿⣿⣿⣿   ⣿⣿⣿⣿",
    "⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿    ⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿    ⣿⣿⣿⣿   ⣿⣿⣿⣿",
];
const BIG_COLUMNS: u16 = 42;
const BIG_ROWS: u16 = 7;
/// Blank rows between the big mark and the tagline.
const BIG_GAP: u16 = 1;

const TAGLINE: &str = "competitive programming, beside your code";
/// The tagline, the listener (two lines) and the gaps between them.
const TEXT_ROWS: u16 = 5;

/// The smallest area the splash needs; the app's minimum builds on it.
pub const MIN_WIDTH: u16 = BIG_COLUMNS + 6;
pub const MIN_HEIGHT: u16 = BIG_ROWS + BIG_GAP + TEXT_ROWS;

const BIG_REVEAL: Duration = Duration::from_millis(700);
/// How long one band takes to cross the mark. The next band enters as it
/// leaves, so the shimmer never stops.
const SHIMMER: Duration = Duration::from_millis(2400);
/// Half the band's width, as a share of the mark.
const SHIMMER_WIDTH: f32 = 0.16;
/// How far the band leans: the top runs this much ahead of the bottom.
const SHIMMER_SLANT: f32 = 0.25;
/// How close to the text colour the band's centre gets.
const SHIMMER_LIGHT: f32 = 0.8;
const BREATH: Duration = Duration::from_secs(3);

/// What the listener is doing, for the one line that says so.
pub enum Listener {
    Up(u16),
    Down(String),
}

pub fn render(frame: &mut Frame, area: Rect, epoch: Instant, now: Instant, listener: &Listener) {
    let [_, block, _] = Layout::vertical([Fill(2), Length(MIN_HEIGHT), Fill(3)]).areas(area);
    let mark = Rect {
        x: block.x + block.width.saturating_sub(BIG_COLUMNS) / 2,
        width: BIG_COLUMNS,
        height: BIG_ROWS,
        ..block
    };
    draw_big_mark(frame.buffer_mut(), mark, epoch, now);
    let text_area = Rect {
        y: block.y + BIG_ROWS + BIG_GAP,
        height: block.height.saturating_sub(BIG_ROWS + BIG_GAP),
        ..block
    };
    frame.render_widget(
        Paragraph::new(Text::from(text_lines(listener, epoch, now))).alignment(Alignment::Center),
        text_area,
    );
}

/// How much light the shimmer adds at (`xf`, `yf`), both 0–1 across the
/// mark, at `at` (0–1) through its cycle. Bands slanted like the Z travel
/// left to right without a break: they repeat one mark-width (plus a band)
/// apart, so as one leaves on the right the next is already entering on the
/// left. Each band's edges ease in and out.
fn shimmer(at: f32, xf: f32, yf: f32) -> f32 {
    let spacing = 1.0 + 2.0 * SHIMMER_WIDTH;
    // Slanted like the Z: the top runs ahead of the bottom.
    let along = xf - (1.0 - yf) * SHIMMER_SLANT + SHIMMER_SLANT / 2.0;
    let offset = (along + SHIMMER_WIDTH - at * spacing).rem_euclid(spacing);
    let distance = offset.min(spacing - offset);
    let t = (1.0 - distance / SHIMMER_WIDTH).clamp(0.0, 1.0);
    // Smoothstep, so the band has no hard edge.
    t * t * (3.0 - 2.0 * t) * SHIMMER_LIGHT
}

/// Colour the banner into exactly `area`. The gradient runs per cell,
/// mostly across and a little down; at launch it draws in left to right,
/// then a band of light drifts across.
fn draw_big_mark(buf: &mut Buffer, area: Rect, epoch: Instant, now: Instant) {
    let p = theme::p();
    let revealed = motion::ease_out(epoch, BIG_REVEAL, now);
    let shimmer_at = motion::enabled().then(|| {
        let since = now.saturating_duration_since(epoch);
        (since.as_secs_f32() / SHIMMER.as_secs_f32()).fract()
    });
    for (cy, line) in BANNER.iter().enumerate() {
        for (cx, ch) in line.chars().enumerate() {
            let (x, y) = (area.x + cx as u16, area.y + cy as u16);
            if ch == ' ' || !buf.area.contains(ratatui::layout::Position::new(x, y)) {
                continue;
            }
            let xf = cx as f32 / f32::from(BIG_COLUMNS - 1);
            let yf = cy as f32 / f32::from(BIG_ROWS - 1);
            // The leading edge of the reveal arrives soft.
            let shown = ((revealed * 1.1 - xf) / 0.1).clamp(0.0, 1.0);
            if shown == 0.0 {
                continue;
            }
            let mut color = theme::gradient(&p.signature, (xf * 0.85 + yf * 0.15).clamp(0.0, 1.0));
            if let Some(at) = shimmer_at {
                color = theme::mix(color, p.text, shimmer(at, xf, yf));
            }
            let color = theme::paint(theme::mix(p.bg, color, shown));
            buf[(x, y)]
                .set_symbol(&ch.to_string())
                .set_fg(color)
                .set_bg(theme::bg());
        }
    }
}

fn text_lines(listener: &Listener, epoch: Instant, now: Instant) -> Vec<Line<'static>> {
    let mut lines = vec![
        Line::default(),
        Line::styled(TAGLINE, Style::new().fg(theme::muted())),
        Line::default(),
    ];
    match listener {
        Listener::Up(port) => {
            let glow = motion::breath(epoch, now, BREATH);
            let p = theme::p();
            lines.push(Line::from(vec![
                Span::styled(
                    "● ",
                    Style::new().fg(theme::blend(p.faint, p.success, 0.45 + 0.55 * glow)),
                ),
                Span::styled(
                    format!("listening on 127.0.0.1:{port}"),
                    Style::new().fg(theme::faint()),
                ),
            ]));
            lines.push(Line::styled(
                "send a problem with Competitive Companion",
                Style::new().fg(theme::text()),
            ));
        }
        Listener::Down(reason) => {
            lines.push(Line::from(vec![
                Span::styled("✗ ", Style::new().fg(theme::error()).bold()),
                Span::styled("not listening", Style::new().fg(theme::text())),
            ]));
            lines.push(Line::styled(
                reason.clone(),
                Style::new().fg(theme::muted()),
            ));
        }
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend};

    fn screen_at(
        width: u16,
        height: u16,
        listener: Listener,
        after: Duration,
    ) -> (Vec<String>, Terminal<TestBackend>) {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        let epoch = Instant::now();
        terminal
            .draw(|frame| {
                let area = frame.area();
                render(frame, area, epoch, epoch + after, &listener);
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        let rows: Vec<String> = (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect()
            })
            .collect();
        if std::env::var("ZCH_DUMP").is_ok() {
            println!("{width}x{height}\n{}", rows.join("\n"));
        }
        (rows, terminal)
    }

    /// Long after launch: the reveal has finished.
    fn screen(width: u16, height: u16, listener: Listener) -> (Vec<String>, Terminal<TestBackend>) {
        screen_at(width, height, listener, Duration::from_secs(30))
    }

    /// Cells the big mark painted.
    fn painted(terminal: &Terminal<TestBackend>) -> Vec<ratatui::buffer::Cell> {
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .filter(|cell| cell.bg != ratatui::style::Color::Reset)
            .cloned()
            .collect()
    }

    #[test]
    fn the_big_mark_is_the_banner_coloured_in() {
        let (_, terminal) = screen(100, 30, Listener::Up(10043));
        let cells = painted(&terminal);
        let glyphs: usize = BANNER
            .iter()
            .map(|line| line.chars().filter(|ch| *ch != ' ').count())
            .sum();
        assert_eq!(cells.len(), glyphs, "exactly the banner's glyphs");
        assert!(
            cells.iter().all(|cell| cell.bg == theme::bg()),
            "dots sit on the canvas"
        );
        let colours: std::collections::HashSet<_> =
            cells.iter().map(|cell| format!("{:?}", cell.fg)).collect();
        assert!(colours.len() > 30, "a smooth gradient: {}", colours.len());
    }

    #[test]
    fn the_banner_is_braille_only() {
        for line in BANNER {
            for ch in line.chars() {
                assert!(
                    ch == ' ' || ('\u{2801}'..='\u{28ff}').contains(&ch),
                    "{ch:?} in {line}: braille dots never leave a seam"
                );
            }
        }
    }

    #[test]
    fn the_shimmer_never_stops() {
        // At every moment some part of the mark is lit.
        for step in 0..100 {
            let at = step as f32 / 100.0;
            let lit = (0..=20)
                .flat_map(|x| (0..=6).map(move |y| (x as f32 / 20.0, y as f32 / 6.0)))
                .any(|(x, y)| shimmer(at, x, y) > 0.3);
            assert!(lit, "dark at {at}");
        }
        // It moves left to right, and wraps without a jump.
        let peak = |at: f32| {
            (0..=100)
                .map(|x| x as f32 / 100.0)
                .max_by(|a, b| shimmer(at, *a, 0.5).total_cmp(&shimmer(at, *b, 0.5)))
                .unwrap()
        };
        assert!(peak(0.3) < peak(0.6));
        assert!((shimmer(0.0, 0.4, 0.5) - shimmer(1.0, 0.4, 0.5)).abs() < 1e-4);
        // Slanted: the top is lit further right than the bottom.
        assert!(shimmer(0.5, 0.6, 0.0) > shimmer(0.5, 0.6, 1.0));
    }

    #[test]
    fn the_banner_is_the_size_it_says() {
        for line in BANNER {
            assert_eq!(line.chars().count(), BIG_COLUMNS as usize, "{line}");
        }
    }

    #[test]
    fn everything_is_centred() {
        let (rows, terminal) = screen(80, 24, Listener::Up(10043));
        let centre = |needle: &str| {
            let row = rows.iter().find(|row| row.contains(needle)).unwrap();
            let start = row.find(needle).unwrap() as i32;
            start + needle.chars().count() as i32 / 2
        };
        for needle in [TAGLINE, "send a problem with Competitive Companion"] {
            assert!((centre(needle) - 40).abs() <= 1, "{needle}: {rows:?}");
        }
        let xs: Vec<u16> = (0..80)
            .filter(|&x| {
                (0..24).any(|y| {
                    terminal.backend().buffer()[(x, y)].symbol() != " "
                        && terminal.backend().buffer()[(x, y)].bg == theme::bg()
                })
            })
            .collect();
        let (left, right) = (xs[0], 79 - xs[xs.len() - 1]);
        assert!(
            left.abs_diff(right) <= 1,
            "the mark is centred: {left} vs {right}"
        );
    }

    #[test]
    fn the_reveal_starts_empty() {
        let (_, terminal) = screen_at(80, 24, Listener::Up(1), Duration::ZERO);
        if motion::enabled() {
            assert!(
                painted(&terminal).is_empty(),
                "the mark draws in from nothing"
            );
        }
    }

    #[test]
    fn the_minimum_fits_the_whole_screen() {
        let (rows, terminal) = screen(MIN_WIDTH, MIN_HEIGHT, Listener::Up(10043));
        let glyphs: usize = BANNER
            .iter()
            .map(|line| line.chars().filter(|ch| *ch != ' ').count())
            .sum();
        assert_eq!(painted(&terminal).len(), glyphs, "the whole mark");
        for needle in [TAGLINE, "send a problem with Competitive Companion"] {
            assert!(
                rows.iter().any(|row| row.contains(needle)),
                "{needle}: {rows:?}"
            );
        }
    }

    #[test]
    fn a_dead_listener_says_so() {
        let (rows, _) = screen(80, 24, Listener::Down("port 10043 is in use".into()));
        let text = rows.join("\n");
        assert!(text.contains("not listening"));
        assert!(text.contains("port 10043 is in use"));
        assert!(!text.contains("listening on"));
    }
}
