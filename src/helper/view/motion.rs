//! Transition state for everything drawn.

use super::*;

/// Which sheet is open, so a new sheet plays its entrance and a redraw of
/// the same sheet does not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::helper) enum Sheet {
    Help,
    Log,
    Config,
    Submit,
    Delete,
    Quit,
    Pane,
}

/// Transition state. Everything is a timestamp; frames are computed from
/// "now", so nothing depends on how often the loop happens to redraw.
pub(in crate::helper) struct Motion {
    /// Phase origin for spinners and ambient motion.
    pub(in crate::helper) epoch: Instant,
    pub(in crate::helper) sheet: Option<(Sheet, Instant)>,
    pub(in crate::helper) flash: Option<(Tone, Instant)>,
    /// The progress rule's displayed fraction, gliding to the real one.
    pub(in crate::helper) progress: f32,
    pub(in crate::helper) progress_at: Instant,
    pub(in crate::helper) problem_at: Option<Instant>,
}

impl Motion {
    pub(in crate::helper) fn new() -> Self {
        let now = Instant::now();
        Self {
            epoch: now,
            sheet: None,
            flash: None,
            progress: 0.0,
            progress_at: now,
            problem_at: None,
        }
    }

    pub(in crate::helper) fn run_started(&mut self) {
        self.flash = None;
        self.progress = 0.0;
        self.progress_at = Instant::now();
    }

    /// The progress rule briefly takes the outcome's colour.
    pub(in crate::helper) fn flash(&mut self, tone: Tone) {
        self.flash = Some((tone, Instant::now()));
    }

    pub(in crate::helper) fn problem_changed(&mut self) {
        self.problem_at = Some(Instant::now());
    }

    /// Open-transition progress for `sheet`, restarting when it changes.
    pub(in crate::helper) fn sheet(&mut self, sheet: Option<Sheet>, now: Instant) -> f32 {
        match (sheet, self.sheet) {
            (None, _) => {
                self.sheet = None;
                1.0
            }
            (Some(open), Some((current, since))) if open == current => {
                anim::ease_out(since, anim::SETTLE, now)
            }
            (Some(open), _) => {
                self.sheet = Some((open, now));
                anim::ease_out(now, anim::SETTLE, now)
            }
        }
    }
}
