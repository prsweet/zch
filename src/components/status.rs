//! The one status line, and the vocabulary every message is written in.
//!
//! A message is a [`Notice`]: a [`Tone`] plus plain text. The tone alone
//! decides the glyph and colour, so the kind of a message survives
//! `NO_COLOR` and no caller can pass a glyph or colour that disagrees with
//! what it says. Text style, used by every message in the app:
//!
//! * lowercase, no trailing period, facts joined with ` · `
//! * a cause after ` — ` (`cannot submit — no problem URL found`)
//! * units spelled the same everywhere: `412 ms`, `1.24 s`, `3.1 MB`
//!
//! Three lanes decide what shows: an error or warning stays until the next
//! result replaces it (or Esc dismisses it), a result stays until the next
//! action, and live progress shows only while nothing more important does.
//! Every notice is also kept in a short log (`m` shows it).

use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

use ratatui::{
    style::{Color, Style},
    text::{Line, Span},
};

use super::{motion, theme, ui};

const LOG_LIMIT: usize = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    /// Work in progress: a spinner.
    Progress,
    Success,
    /// A good-but-not-final outcome (Codeforces "Pretests passed").
    Partial,
    Info,
    Warning,
    Error,
}

impl Tone {
    pub fn rgb(self) -> theme::Rgb {
        match self {
            Tone::Progress => theme::p().accent,
            Tone::Success => theme::p().success,
            Tone::Partial => theme::p().warning,
            Tone::Info => theme::p().info,
            Tone::Warning => theme::p().warning,
            Tone::Error => theme::p().error,
        }
    }

    pub fn color(self) -> Color {
        theme::paint(self.rgb())
    }

    /// The glyph that carries the tone without colour.
    pub fn glyph(self, epoch: Instant, now: Instant) -> &'static str {
        match self {
            Tone::Progress => motion::spinner(epoch, now),
            Tone::Success | Tone::Partial => "✓",
            Tone::Info => "·",
            Tone::Warning => "!",
            Tone::Error => "✗",
        }
    }

    /// Errors and warnings wait for the user; everything else may fade.
    pub fn sticky(self) -> bool {
        matches!(self, Tone::Warning | Tone::Error)
    }
}

#[derive(Debug, Clone)]
pub struct Notice {
    pub tone: Tone,
    pub text: String,
    pub at: Instant,
}

impl Notice {
    fn new(tone: Tone, text: impl Into<String>) -> Self {
        Self {
            tone,
            text: tidy(&text.into()),
            at: Instant::now(),
        }
    }
}

/// One line, no trailing period: multi-line causes become ` · ` facts.
fn tidy(text: &str) -> String {
    let joined = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" · ");
    joined
        .strip_suffix('.')
        .map(str::to_string)
        .unwrap_or(joined)
}

#[derive(Default)]
pub struct Status {
    live: Option<Notice>,
    result: Option<Notice>,
    alert: Option<Notice>,
    log: VecDeque<Notice>,
}

impl Status {
    /// Live work in progress. Repeating the same text keeps its start time,
    /// so the elapsed counter does not reset on every runner update.
    pub fn progress(&mut self, text: impl Into<String>) {
        let notice = Notice::new(Tone::Progress, text);
        if self
            .live
            .as_ref()
            .is_some_and(|live| live.text == notice.text)
        {
            return;
        }
        let started = self.live.as_ref().map(|live| live.at);
        self.live = Some(Notice {
            // A phase change is the same piece of work: keep its clock.
            at: started.unwrap_or(notice.at),
            ..notice
        });
    }

    pub fn success(&mut self, text: impl Into<String>) {
        self.set_result(Notice::new(Tone::Success, text));
    }

    pub fn info(&mut self, text: impl Into<String>) {
        self.set_result(Notice::new(Tone::Info, text));
    }

    pub fn warn(&mut self, text: impl Into<String>) {
        self.set_alert(Notice::new(Tone::Warning, text));
    }

    pub fn error(&mut self, text: impl Into<String>) {
        self.set_alert(Notice::new(Tone::Error, text));
    }

    fn set_result(&mut self, notice: Notice) {
        self.record(&notice);
        // A new result answers whatever went wrong before it.
        self.alert = None;
        self.result = Some(notice);
    }

    fn set_alert(&mut self, notice: Notice) {
        if self
            .alert
            .as_ref()
            .is_some_and(|alert| alert.text == notice.text && alert.tone == notice.tone)
        {
            return;
        }
        self.record(&notice);
        self.alert = Some(notice);
    }

    fn record(&mut self, notice: &Notice) {
        self.log.push_back(notice.clone());
        while self.log.len() > LOG_LIMIT {
            self.log.pop_front();
        }
    }

    pub fn clear_progress(&mut self) {
        self.live = None;
    }

    /// Esc dismisses an error or warning. Returns whether one was showing.
    pub fn dismiss(&mut self) -> bool {
        self.alert.take().is_some()
    }

    /// Start of a new action: nothing from the last one is current any more.
    pub fn clear(&mut self) {
        self.live = None;
        self.result = None;
        self.alert = None;
    }

    /// What the status line shows now.
    pub fn current(&self) -> Option<&Notice> {
        self.alert
            .as_ref()
            .or(self.live.as_ref())
            .or(self.result.as_ref())
    }

    /// The text of the current notice (tests and `zch send` replies).
    pub fn text(&self) -> Option<&str> {
        self.current().map(|notice| notice.text.as_str())
    }

    #[cfg(test)]
    pub fn tone(&self) -> Option<Tone> {
        self.current().map(|notice| notice.tone)
    }

    pub fn log(&self) -> impl DoubleEndedIterator<Item = &Notice> + ExactSizeIterator {
        self.log.iter()
    }

    /// Whether a notice is mid-transition and needs another frame.
    pub fn animating(&self, now: Instant) -> bool {
        let Some(notice) = self.current() else {
            return false;
        };
        if notice.tone == Tone::Progress {
            return true;
        }
        let age = now.saturating_duration_since(notice.at);
        motion::active(notice.at, motion::SETTLE, now)
            || (!notice.tone.sticky()
                && age >= motion::NOTICE_HOLD
                && age < motion::NOTICE_HOLD + motion::NOTICE_FADE)
    }

    /// The status line: glyph in the tone's colour, text in the reading
    /// colour. A fresh notice settles in from faint; a success or info
    /// notice quiets to muted after a few seconds instead of vanishing, so
    /// the last result stays readable. Live progress shows its elapsed time.
    pub fn line(
        &self,
        width: usize,
        epoch: Instant,
        now: Instant,
    ) -> (Line<'static>, Line<'static>) {
        let Some(notice) = self.current() else {
            return (Line::default(), Line::default());
        };
        let settle = motion::ease_out(notice.at, motion::SETTLE, now);
        let age = now.saturating_duration_since(notice.at);
        let quiet = if notice.tone.sticky() || notice.tone == Tone::Progress {
            0.0
        } else {
            motion::progress(notice.at + motion::NOTICE_HOLD, motion::NOTICE_FADE, now)
                * f32::from(age >= motion::NOTICE_HOLD)
        };
        let resting = if notice.tone == Tone::Progress {
            theme::p().muted
        } else {
            theme::mix(theme::p().text, theme::p().muted, quiet)
        };
        let text_color = theme::blend(theme::p().faint, resting, settle);
        let glyph_color = theme::blend(
            theme::p().faint,
            theme::mix(notice.tone.rgb(), theme::p().muted, quiet * 0.6),
            settle,
        );

        let right = if notice.tone == Tone::Progress && age >= Duration::from_secs(1) {
            Line::from(Span::styled(
                format_elapsed(age),
                Style::new().fg(theme::faint()),
            ))
        } else {
            Line::default()
        };
        let room = width.saturating_sub(right.width() + 4);
        let left = Line::from(vec![
            Span::styled(
                format!("{} ", notice.tone.glyph(epoch, now)),
                Style::new().fg(glyph_color).bold(),
            ),
            Span::styled(
                ui::truncate(&notice.text, room),
                Style::new().fg(text_color),
            ),
        ]);
        (left, right)
    }
}

/// `0.4 s`, `12 s`, `2 m 05 s`.
pub fn format_elapsed(elapsed: Duration) -> String {
    let seconds = elapsed.as_secs();
    if seconds >= 60 {
        format!("{} m {:02} s", seconds / 60, seconds % 60)
    } else if seconds >= 10 {
        format!("{seconds} s")
    } else {
        format!("{:.1} s", elapsed.as_secs_f32())
    }
}

/// `412 ms`, `1.24 s`, and whole seconds plainly: `2 s`.
pub fn format_ms(ms: u128) -> String {
    if ms < 1000 {
        format!("{ms} ms")
    } else if ms.is_multiple_of(1000) {
        format!("{} s", ms / 1000)
    } else {
        format!("{:.2} s", ms as f64 / 1000.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_outrank_progress_and_results_answer_errors() {
        let mut status = Status::default();
        status.progress("running case 1 of 2");
        status.error("listener failed");
        assert_eq!(status.text(), Some("listener failed"));
        assert_eq!(status.tone(), Some(Tone::Error));

        status.success("2 of 2 passed");
        assert_eq!(
            status.text(),
            Some("running case 1 of 2"),
            "live work shows while it runs"
        );
        status.clear_progress();
        assert_eq!(status.text(), Some("2 of 2 passed"));
        assert_eq!(status.tone(), Some(Tone::Success));
    }

    #[test]
    fn esc_dismisses_an_alert() {
        let mut status = Status::default();
        status.success("saved");
        status.warn("a run is already in progress");
        assert_eq!(status.text(), Some("a run is already in progress"));
        assert!(status.dismiss());
        assert_eq!(
            status.text(),
            Some("saved"),
            "dismissing a refusal reveals the result it was covering"
        );
        assert!(!status.dismiss());
    }

    #[test]
    fn text_is_tidied_into_one_line() {
        let mut status = Status::default();
        status.error("first line\n  second line.\n");
        assert_eq!(status.text(), Some("first line · second line"));
    }

    #[test]
    fn every_notice_is_logged() {
        let mut status = Status::default();
        status.success("a");
        status.error("b");
        status.error("b");
        status.info("c");
        let texts: Vec<_> = status.log().map(|n| n.text.as_str()).collect();
        assert_eq!(texts, ["a", "b", "c"]);
    }

    #[test]
    fn progress_keeps_its_clock_across_phases() {
        let mut status = Status::default();
        status.progress("compiling");
        let started = status.current().unwrap().at;
        status.progress("running case 1 of 3");
        assert_eq!(status.current().unwrap().at, started);
    }

    #[test]
    fn units_are_spelled_one_way() {
        assert_eq!(format_ms(412), "412 ms");
        assert_eq!(format_ms(1240), "1.24 s");
        assert_eq!(format_ms(2000), "2 s");
        assert_eq!(format_elapsed(Duration::from_millis(400)), "0.4 s");
        assert_eq!(format_elapsed(Duration::from_secs(12)), "12 s");
        assert_eq!(format_elapsed(Duration::from_secs(125)), "2 m 05 s");
    }
}
