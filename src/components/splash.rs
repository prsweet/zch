//! The empty state: nothing loaded yet.
//!
//! The one memorable thing in zch: a ZCH set in half-block pixels. Every
//! half-cell is its own pixel with its own colour, so the theme's signature
//! gradient runs smoothly across and down the letters, and a soft offset
//! shadow gives them weight. At launch the mark draws itself in left to
//! right once; after that a slow band of light drifts across it. Everything
//! around it stays quiet: one line of purpose and the listener.
//!
//! The mark is never clipped: its size is the app's minimum terminal size,
//! and an area smaller than the mark gets the text alone. Reduced motion
//! shows the finished mark, still.

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

use crate::components::{
    motion,
    theme::{self, Rgb},
};

/// 14 pixels tall (7 rows), on a 1 × ½ cell grid so pixels are near
/// square. Bars are 3 pixels tall, stems 3 wide; the Z's diagonal steps one
/// pixel per row so its edge stays smooth.
const Z: [&str; PIXEL_ROWS] = [
    "##############",
    "##############",
    "##############",
    ".........####.",
    "........####..",
    ".......####...",
    "......####....",
    ".....####.....",
    "....####......",
    "...####.......",
    "..####........",
    "##############",
    "##############",
    "##############",
];
const C: [&str; PIXEL_ROWS] = [
    "...#########",
    ".###########",
    "############",
    "###.........",
    "###.........",
    "###.........",
    "###.........",
    "###.........",
    "###.........",
    "###.........",
    "###.........",
    "############",
    ".###########",
    "...#########",
];
const H: [&str; PIXEL_ROWS] = [
    "###......###",
    "###......###",
    "###......###",
    "###......###",
    "###......###",
    "############",
    "############",
    "############",
    "###......###",
    "###......###",
    "###......###",
    "###......###",
    "###......###",
    "###......###",
];
const LETTER_GAP: usize = 2;
const PIXEL_ROWS: usize = 14;
/// The mark's own size: 42 pixels plus the shadow column, 7 rows plus the
/// shadow's half row.
const MARK_COLUMNS: u16 = 43;
const MARK_ROWS: u16 = 8;

const TAGLINE: &str = "competitive programming, beside your code";
/// The tagline, the listener (two lines) and the gaps between them.
const TEXT_ROWS: u16 = 5;

/// The smallest area the splash needs; the app's minimum builds on it.
pub const MIN_WIDTH: u16 = MARK_COLUMNS + 5;
pub const MIN_HEIGHT: u16 = MARK_ROWS + TEXT_ROWS;

const REVEAL: Duration = Duration::from_millis(700);
const SWEEP: Duration = Duration::from_secs(7);
const BREATH: Duration = Duration::from_secs(3);

/// What the listener is doing, for the one line that says so.
pub enum Listener {
    Up(u16),
    Down(String),
}

/// The wordmark as a pixel grid.
struct Mark {
    width: usize,
    lit: Vec<Vec<bool>>,
}

impl Mark {
    fn new() -> Self {
        let letters = [&Z, &C, &H];
        let width = letters.iter().map(|l| l[0].len()).sum::<usize>() + LETTER_GAP * 2;
        let mut lit = vec![vec![false; width]; PIXEL_ROWS];
        let mut x0 = 0;
        for letter in letters {
            for (y, row) in letter.iter().enumerate() {
                for (x, ch) in row.chars().enumerate() {
                    lit[y][x0 + x] = ch == '#';
                }
            }
            x0 += letter[0].len() + LETTER_GAP;
        }
        Self { width, lit }
    }

    fn on(&self, x: isize, y: isize) -> bool {
        x >= 0
            && y >= 0
            && self
                .lit
                .get(y as usize)
                .and_then(|row| row.get(x as usize))
                .copied()
                .unwrap_or(false)
    }
}

pub fn render(frame: &mut Frame, area: Rect, epoch: Instant, now: Instant, listener: &Listener) {
    let text = text_lines(listener, epoch, now);
    let with_mark = area.width >= MARK_COLUMNS && area.height >= MIN_HEIGHT;
    let height = if with_mark { MIN_HEIGHT } else { TEXT_ROWS };
    let [_, block, _] = Layout::vertical([Fill(2), Length(height), Fill(3)]).areas(area);

    let text_area = if with_mark {
        let mark_area = Rect {
            x: block.x + (block.width - MARK_COLUMNS) / 2,
            width: MARK_COLUMNS,
            height: MARK_ROWS,
            ..block
        };
        draw_mark(frame.buffer_mut(), mark_area, &Mark::new(), epoch, now);
        Rect {
            y: mark_area.bottom(),
            height: block.height - MARK_ROWS,
            ..block
        }
    } else {
        block
    };
    frame.render_widget(
        Paragraph::new(Text::from(text)).alignment(Alignment::Center),
        text_area,
    );
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

/// Paint the mark pixel by pixel into exactly `area` (its own size). A cell
/// holds two pixels: `▀` takes the upper one's colour as foreground and the
/// lower one's as background, so the gradient resolves at half-cell height.
fn draw_mark(buf: &mut Buffer, area: Rect, mark: &Mark, epoch: Instant, now: Instant) {
    let p = theme::p();
    let width = mark.width as f32;
    let since = now.saturating_duration_since(epoch);
    let revealed = motion::ease_out(epoch, REVEAL, now);
    let sweep_at = if motion::enabled() {
        Some((since.as_secs_f32() / SWEEP.as_secs_f32()).fract())
    } else {
        None
    };
    let shadow = theme::mix(p.bg, p.signature[0], 0.16);

    let pixel = |x: isize, y: isize| -> Option<Rgb> {
        let xf = x as f32 / width;
        if xf > revealed * 1.05 {
            return None;
        }
        if !mark.on(x, y) {
            return mark.on(x - 1, y - 1).then_some(shadow);
        }
        let yf = y as f32 / PIXEL_ROWS as f32;
        // Diagonal gradient: mostly across, a little down.
        let mut color = theme::gradient(&p.signature, (xf * 0.85 + yf * 0.15).clamp(0.0, 1.0));
        if let Some(sweep_at) = sweep_at {
            // The light band travels past both ends, so it fades in and out.
            let band = -0.3 + sweep_at * 1.6;
            let light = (1.0 - (xf + yf * 0.25 - band).abs() / 0.14).clamp(0.0, 1.0);
            color = theme::mix(color, p.text, light * light * 0.45);
        }
        // The leading edge of the reveal arrives soft.
        let edge = ((revealed * 1.05 - xf) / 0.06).clamp(0.0, 1.0);
        Some(theme::mix(p.bg, color, edge))
    };

    let visible = area.intersection(buf.area);
    for y in visible.top()..visible.bottom() {
        for x in visible.left()..visible.right() {
            let (px, py) = ((x - area.x) as isize, (y - area.y) as isize * 2);
            let (top, bottom) = (pixel(px, py), pixel(px, py + 1));
            let cell = &mut buf[(x, y)];
            match (top, bottom) {
                (None, None) => {}
                (Some(top), bottom) => {
                    cell.set_symbol("▀")
                        .set_fg(theme::paint(top))
                        .set_bg(bottom.map_or_else(theme::bg, theme::paint));
                }
                (None, Some(bottom)) => {
                    cell.set_symbol("▄")
                        .set_fg(theme::paint(bottom))
                        .set_bg(theme::bg());
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend};

    fn screen(width: u16, height: u16, listener: Listener) -> (String, Terminal<TestBackend>) {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        // Long after launch: the reveal has finished.
        let epoch = Instant::now();
        let now = epoch + Duration::from_secs(30);
        terminal
            .draw(|frame| {
                let area = frame.area();
                render(frame, area, epoch, now, &listener);
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
        (text, terminal)
    }

    #[test]
    fn the_mark_is_the_size_the_letters_say() {
        let mark = Mark::new();
        assert_eq!(mark.width as u16 + 1, MARK_COLUMNS);
        assert_eq!((PIXEL_ROWS as u16).div_ceil(2) + 1, MARK_ROWS);
        assert!(TAGLINE.len() as u16 <= MIN_WIDTH);
    }

    #[test]
    fn one_mark_at_every_size_that_fits_it() {
        for (width, height) in [(MIN_WIDTH, MIN_HEIGHT), (120, 40)] {
            let (text, _) = screen(width, height, Listener::Up(10043));
            assert!(text.contains('▀'), "mark at {width}x{height}:\n{text}");
            assert!(text.contains("listening on 127.0.0.1:10043"));
        }
    }

    #[test]
    fn the_mark_never_draws_outside_its_area() {
        // Narrower than the mark: text only, nothing half-drawn.
        let (text, _) = screen(MARK_COLUMNS - 1, 30, Listener::Up(10043));
        assert!(!text.contains('▀') && !text.contains('▄'), "{text}");
        assert!(text.contains("listening on"));
    }

    #[test]
    fn the_mark_is_painted_per_pixel() {
        let (_, terminal) = screen(120, 40, Listener::Up(10043));
        let mut colours = std::collections::HashSet::new();
        for cell in terminal.backend().buffer().content() {
            if cell.symbol() == "▀" {
                colours.insert(format!("{:?}{:?}", cell.fg, cell.bg));
            }
        }
        assert!(colours.len() > 20, "a smooth gradient: {}", colours.len());
    }

    #[test]
    fn the_reveal_starts_empty() {
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
        let now = Instant::now();
        terminal
            .draw(|frame| {
                let area = frame.area();
                render(frame, area, now, now, &Listener::Up(1));
            })
            .unwrap();
        let painted = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .filter(|cell| cell.symbol() == "▀")
            .count();
        if motion::enabled() {
            assert!(painted < 10, "nothing is drawn at t = 0: {painted}");
        }
    }

    #[test]
    fn a_dead_listener_says_so() {
        let (text, _) = screen(80, 24, Listener::Down("port 10043 is in use".into()));
        assert!(text.contains("not listening"));
        assert!(text.contains("port 10043 is in use"));
        assert!(!text.contains("listening on"));
    }
}
