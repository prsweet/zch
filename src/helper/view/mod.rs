//! Everything zch draws.
//!
//! ```text
//!   ◆ zch  v3.0.0                                   ● E_Prime_Destruction    ┐ header
//!     ~ › cp › round912                                                       ┘ (surface)
//! ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━──────────────────────────────────────────────  progress
//!
//!   01 ▸ accepted                                               4 ms · 3.1 MB
//! ▎ 02 ▾ wrong answer                                           9 ms · 3.0 MB
//!     in 5
//!    exp 10                          │ got 0
//!
//! ── submissions ─────────────────────────────────────────────────────────────
//!   ⠹ E_Prime_Destruction   running on test 4                          12 s
//!  NORMAL  ❯ 1 of 3 failed · 1.24 s          [r] run  [s] submit  [h] help  console
//! ```
//!
//! One column that only gets narrower. The minimum size is what the splash
//! needs, so nothing is ever drawn half-clipped; below it a "too small"
//! screen says what is needed.
//!
//! | module    | draws                                                 |
//! |-----------|-------------------------------------------------------|
//! | `chrome`  | header, progress rule, submissions, console           |
//! | `session` | the interactive transcript                            |
//! | `sheets`  | help, messages, submit, delete, quit, the pane viewer |
//! | `motion`  | transition state shared by all of them                |

use std::time::{Duration, Instant};

use ratatui::{
    Frame,
    layout::{Alignment, Rect},
    style::Style,
    text::{Line, Span, Text},
    widgets::{Paragraph, ScrollbarOrientation, ScrollbarState},
};

use super::*;

mod chrome;
mod motion;
mod session;
mod sheets;

pub(super) use motion::Motion;
use motion::Sheet;

use crate::components::{
    dir_terminal::PATH_GLOW, motion as anim, scrollbar, splash, status::format_elapsed,
    tc_list::ListLook, testcase::CaseFocus, ui,
};

/// The padded header, rule and gap above the body; gap, rule and console
/// below it.
const CHROME_ROWS: u16 = chrome::header_rows(1) + 5;
/// Blank rows between the submissions, when there are any, and the console.
const FOOTER_PAD: u16 = 1;
const MIN_WIDTH: u16 = splash::MIN_WIDTH;
const MIN_HEIGHT: u16 = splash::MIN_HEIGHT + CHROME_ROWS;

/// Where each part of the screen goes this frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Regions {
    header: Rect,
    rule: Rect,
    body: Rect,
    footer_rule: Rect,
    submissions: Rect,
    console: Rect,
}

/// The frame's geometry: a header block of `header_rows`, the progress
/// rule, the body, then `submission_rows` under a labelled rule, and the
/// console on the last row. Submissions give way before the body does.
fn regions(area: Rect, header_rows: u16, submission_rows: u16) -> Regions {
    let row = |y: u16, height: u16| Rect {
        x: area.x,
        y,
        width: area.width,
        height,
    };
    let header = row(area.y, header_rows);
    let rule = row(header.bottom(), 1);
    let body_top = rule.bottom() + 1;
    let console = row(area.bottom() - 1, 1);
    // The footer, bottom up: the console, then the rule right above it.
    // Submissions open a block between the two: a title row right under
    // the rule, one row each, and a blank row above the console. Keep at
    // least a few body rows: submission rows give way first, the title
    // stays while any row does.
    // Above the block: 4 body rows, a quiet row, the rule, the title row
    // and the padding.
    let room = console
        .y
        .saturating_sub(body_top + 4 + 1 + 1 + 1 + FOOTER_PAD);
    let submission_rows = submission_rows.min(room);
    let (submissions, footer_rule) = if submission_rows > 0 {
        let block = submission_rows + 1;
        let submissions = row(console.y - FOOTER_PAD - block, block);
        (submissions, row(submissions.y - 1, 1))
    } else {
        (row(console.y, 0), row(console.y - 1, 1))
    };
    let body = Rect {
        x: area.x + ui::GUTTER,
        y: body_top,
        width: area.width.saturating_sub(ui::GUTTER * 2),
        height: footer_rule.y.saturating_sub(body_top + 1),
    };
    Regions {
        header,
        rule,
        body,
        footer_rule,
        submissions,
        console,
    }
}

impl Helper {
    /// Any background work that deserves an animated loader.
    pub fn busy(&self) -> bool {
        self.running
            || self.session.as_ref().is_some_and(|session| session.alive)
            || self.submissions.iter().any(|s| !s.done())
    }

    /// How soon the screen needs another frame with no input, or `None`
    /// when nothing on it is moving.
    pub fn frame_interval(&self, now: Instant) -> Option<Duration> {
        if !anim::enabled() {
            // Elapsed counters still tick, just without the motion.
            return self.busy().then_some(Duration::from_millis(500));
        }
        let m = &self.motion;
        let transitioning = m
            .sheet
            .is_some_and(|(_, since)| anim::active(since, anim::SETTLE, now))
            || m.flash
                .is_some_and(|(_, since)| anim::active(since, anim::FLASH, now))
            || m.problem_at
                .is_some_and(|since| anim::active(since, anim::REVEAL, now))
            || self
                .dir_terminal
                .changed_at
                .is_some_and(|since| anim::active(since, PATH_GLOW, now))
            || (self.running && (m.progress - self.progress_target()).abs() > 0.001)
            || self.tc_list.testcases.iter().any(|tc| tc.revealing(now))
            || self.status.animating(now);
        if transitioning || self.running {
            Some(anim::FRAME)
        } else if self.busy() {
            Some(anim::SPINNER_FRAME)
        } else if self.active_problem.is_none() && self.tc_list.testcases.is_empty() {
            Some(anim::AMBIENT_FRAME)
        } else {
            None
        }
    }

    fn progress_target(&self) -> f32 {
        let p = self.run_progress;
        if p.total == 0 {
            return 0.0;
        }
        let running = if p.current.is_some_and(|current| current >= p.done) {
            0.5
        } else {
            0.0
        };
        ((p.done as f32 + running) / p.total as f32).clamp(0.0, 1.0)
    }

    pub(in crate::helper) fn open_sheet(&self) -> Option<Sheet> {
        if self.show_help {
            Some(Sheet::Help)
        } else if self.pane_dialog.is_some() {
            Some(Sheet::Pane)
        } else if self.show_log {
            Some(Sheet::Log)
        } else {
            match self.input_mode {
                InputMode::Config => Some(Sheet::Config),
                InputMode::ConfirmSubmit => Some(Sheet::Submit),
                InputMode::ConfirmDelete => Some(Sheet::Delete),
                InputMode::ConfirmQuit => Some(Sheet::Quit),
                _ => None,
            }
        }
    }

    pub fn draw(&mut self, frame: &mut Frame) {
        let now = Instant::now();
        let area = frame.area();
        ui::canvas(frame, area);
        ui::clear_reveals();
        self.text_regions.clear();

        if area.width < MIN_WIDTH || area.height < MIN_HEIGHT {
            draw_too_small(frame, area);
            self.paint_selection(frame);
            return;
        }

        // Glide the progress rule towards where the run really is.
        if self.running {
            let elapsed = now.saturating_duration_since(self.motion.progress_at);
            self.motion.progress =
                anim::approach(self.motion.progress, self.progress_target(), elapsed);
        }
        self.motion.progress_at = now;

        let header_rows = chrome::header_rows(self.dir_terminal.input_height());
        let submission_rows = self.submissions.len().min(SUBMISSION_VIEWPORT) as u16;
        let layout = regions(area, header_rows, submission_rows);

        self.draw_header(frame, layout.header, now);
        self.draw_rule(frame, layout.rule, now);
        self.draw_footer_rule(frame, layout.footer_rule);
        self.draw_submissions(frame, layout.submissions, now);
        self.draw_body(frame, layout.body, now);

        // Sheets go on top of everything, one at a time.
        let sheet = self.open_sheet();
        let opened = self.motion.sheet(sheet, now);
        if sheet.is_some() {
            // Only the sheet's own cut text answers a double-click.
            ui::clear_reveals();
        }
        let sheet_text = match sheet {
            Some(Sheet::Pane) => self.draw_pane_dialog(frame, area, opened),
            Some(Sheet::Help) => self.draw_help(frame, area, opened),
            Some(Sheet::Log) => self.draw_log(frame, area, opened, now),
            Some(Sheet::Config) => {
                self.config_menu.draw(frame, area, opened);
                Rect::default()
            }
            Some(Sheet::Submit) => self.draw_submit_sheet(frame, area, opened),
            Some(Sheet::Delete) => self.draw_delete_sheet(frame, area, opened),
            Some(Sheet::Quit) => self.draw_quit_sheet(frame, area, opened),
            None => Rect::default(),
        };
        self.text_regions.push(sheet_text);

        // The console is drawn last, so the keys that leave a sheet stay
        // readable above its dimmed backdrop.
        frame.render_widget(ratatui::widgets::Clear, layout.console);
        self.draw_console(frame, layout.console, now);

        // A text selection made with the mouse sits on top of everything.
        self.paint_selection(frame);
    }

    fn draw_body(&mut self, frame: &mut Frame, body: Rect, now: Instant) {
        if body.height == 0 {
            return;
        }
        if self.tc_list.testcases.is_empty() && self.active_problem.is_some() {
            // The transcript, under its title row and gap.
            self.text_regions.push(Rect {
                y: body.y + 2,
                height: body.height.saturating_sub(2),
                ..body
            });
            self.draw_session(frame, body, now);
        } else if self.tc_list.testcases.is_empty() {
            self.text_regions.push(body);
            splash::render(frame, body, self.motion.epoch, now, &self.listener);
        } else {
            let focus = match self.input_mode {
                InputMode::Editing => CaseFocus::Edit,
                InputMode::DeleteSelect | InputMode::ConfirmDelete => CaseFocus::Delete,
                _ => CaseFocus::None,
            };
            let look = ListLook {
                selection: self.selection_visible && self.input_mode == InputMode::Normal,
                running: self.running.then_some(self.run_progress.current),
                epoch: self.motion.epoch,
                now,
            };
            // The list runs edge to edge: the focus rail sits in the left
            // gutter, the cards pad themselves on the right, and the
            // scrollbar (when needed) takes the last column of the screen.
            let list = Rect {
                x: body.x.saturating_sub(ui::GUTTER),
                width: body.width + ui::GUTTER * 2,
                ..body
            };
            self.tc_list.draw(frame, list, focus, &look);
        }
    }
}

fn draw_too_small(frame: &mut Frame, area: Rect) {
    let lines = vec![
        Line::from(Span::styled(
            "terminal too small",
            Style::new().fg(theme::text()).bold(),
        )),
        Line::from(Span::styled(
            format!(
                "zch needs {MIN_WIDTH} × {MIN_HEIGHT} · now {} × {}",
                area.width, area.height
            ),
            Style::new().fg(theme::muted()),
        )),
        Line::from(Span::styled("q quits", Style::new().fg(theme::faint()))),
    ];
    let height = (lines.len() as u16).min(area.height);
    frame.render_widget(
        Paragraph::new(lines).alignment(Alignment::Center),
        Rect {
            y: area.y + area.height.saturating_sub(height) / 2,
            height,
            ..area
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_frame_stacks_header_body_and_footer() {
        let area = Rect::new(0, 0, 100, 30);
        let r = regions(area, 2, 0);
        assert_eq!(r.header, Rect::new(0, 0, 100, 2));
        assert_eq!(r.rule.y, 2);
        assert_eq!(r.body.y, 4, "one quiet row under the rule");
        assert_eq!(r.console, Rect::new(0, 29, 100, 1));
        assert_eq!(
            r.footer_rule.y, 28,
            "no submissions: the rule sits right on the console"
        );
        assert_eq!(r.body.bottom(), 27, "one quiet row above the footer rule");

        let r = regions(area, 2, 2);
        assert_eq!(
            r.submissions,
            Rect::new(0, 25, 100, 3),
            "a title and two rows"
        );
        assert_eq!(r.footer_rule.y, 24, "the title sits right under the rule");
    }

    #[test]
    fn submissions_give_way_before_the_body() {
        let r = regions(Rect::new(0, 0, 100, 30), 2, 4);
        assert_eq!(r.submissions.height, 5, "a title and four rows");

        let tight = regions(Rect::new(0, 0, 100, 17), 3, 4);
        assert!(tight.body.height >= 4, "{tight:?}");
        assert!(tight.submissions.height < 5, "{tight:?}");
        assert!(
            tight.submissions.height >= 2,
            "the title stays with a row: {tight:?}"
        );
    }

    #[test]
    fn the_minimum_is_what_the_splash_needs() {
        let r = regions(
            Rect::new(0, 0, MIN_WIDTH, MIN_HEIGHT),
            chrome::header_rows(1),
            0,
        );
        assert!(r.body.height >= splash::MIN_HEIGHT, "{r:?}");
        assert!(r.body.width + ui::GUTTER * 2 >= splash::MIN_WIDTH);
    }
}
