//! Motion: short, eased transitions driven by wall-clock time.
//!
//! Animation is decoration with an off switch. Every transition is a pure
//! function of "how long ago did this start", so a frame can be drawn at any
//! moment and a stalled event loop never leaves a half-played effect behind.
//! `ZCH_REDUCED_MOTION=1` (or `ZCH_ANIMATIONS=off`) turns it all off:
//! transitions jump to their end state and spinners become a static mark.

use std::{
    sync::OnceLock,
    time::{Duration, Instant},
};

/// Sheets and the status line settle in this long.
pub const SETTLE: Duration = Duration::from_millis(180);
/// A verdict colours in over this long once its result arrives.
pub const REVEAL: Duration = Duration::from_millis(320);
/// The progress rule flashes the run's outcome, then fades back.
pub const FLASH: Duration = Duration::from_millis(900);
/// A success or info notice stays bright this long, then quiets down.
pub const NOTICE_HOLD: Duration = Duration::from_secs(4);
/// …over this long.
pub const NOTICE_FADE: Duration = Duration::from_millis(900);
/// Spinner frame length (cli-spinners `dots`).
pub const SPINNER_FRAME: Duration = Duration::from_millis(80);
/// Redraw cadence while something is moving.
pub const FRAME: Duration = Duration::from_millis(33);
/// Redraw cadence for slow ambient motion (the idle splash).
pub const AMBIENT_FRAME: Duration = Duration::from_millis(50);

const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

pub fn enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| {
        let var = |name: &str| {
            std::env::var(name)
                .ok()
                .map(|value| value.trim().to_ascii_lowercase())
        };
        let reduced = var("ZCH_REDUCED_MOTION")
            .is_some_and(|value| !matches!(value.as_str(), "" | "0" | "false" | "no"));
        let off = var("ZCH_ANIMATIONS")
            .is_some_and(|value| matches!(value.as_str(), "off" | "none" | "0" | "false" | "no"));
        !(reduced || off)
    })
}

/// Progress of a transition that started at `since`, in `[0, 1]`.
pub fn progress(since: Instant, duration: Duration, now: Instant) -> f32 {
    if !enabled() || duration.is_zero() {
        return 1.0;
    }
    let elapsed = now.saturating_duration_since(since).as_secs_f32();
    (elapsed / duration.as_secs_f32()).clamp(0.0, 1.0)
}

/// [`progress`] through an ease-out curve: quick to respond, soft to land.
pub fn ease_out(since: Instant, duration: Duration, now: Instant) -> f32 {
    let t = progress(since, duration, now);
    1.0 - (1.0 - t).powi(3)
}

/// Whether a transition that started at `since` is still playing.
pub fn active(since: Instant, duration: Duration, now: Instant) -> bool {
    enabled() && now.saturating_duration_since(since) < duration
}

/// The spinner glyph for `now`, phase-locked to `epoch` so every spinner on
/// screen turns together.
pub fn spinner(epoch: Instant, now: Instant) -> &'static str {
    if !enabled() {
        return "◦";
    }
    let frame = now.saturating_duration_since(epoch).as_millis() / SPINNER_FRAME.as_millis();
    SPINNER[(frame as usize) % SPINNER.len()]
}

/// A slow breath in `[0, 1]` with the given period, for ambient motion.
pub fn breath(epoch: Instant, now: Instant, period: Duration) -> f32 {
    if !enabled() {
        return 0.5;
    }
    let phase = now.saturating_duration_since(epoch).as_secs_f32() / period.as_secs_f32();
    0.5 - 0.5 * (phase * std::f32::consts::TAU).cos()
}

/// Exponential approach of `current` towards `target`, frame-rate independent.
/// Used to glide the progress rule instead of jumping case by case.
pub fn approach(current: f32, target: f32, elapsed: Duration) -> f32 {
    if !enabled() {
        return target;
    }
    let rate = 1.0 - (-elapsed.as_secs_f32() * 14.0).exp();
    let next = current + (target - current) * rate;
    if (target - next).abs() < 0.002 {
        target
    } else {
        next
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transitions_run_from_zero_to_one() {
        let start = Instant::now();
        assert_eq!(progress(start, SETTLE, start), 0.0);
        assert_eq!(progress(start, SETTLE, start + SETTLE), 1.0);
        assert_eq!(progress(start, SETTLE, start + SETTLE * 3), 1.0);
        let mid = ease_out(start, SETTLE, start + SETTLE / 2);
        assert!(mid > 0.5 && mid < 1.0, "ease-out leads: {mid}");
        assert!(active(start, SETTLE, start));
        assert!(!active(start, SETTLE, start + SETTLE));
    }

    #[test]
    fn approach_converges() {
        let mut value = 0.0;
        for _ in 0..60 {
            value = approach(value, 1.0, FRAME);
        }
        assert_eq!(value, 1.0);
        assert_eq!(approach(0.3, 0.3, FRAME), 0.3);
    }

    #[test]
    fn spinner_frames_advance_with_time() {
        let epoch = Instant::now();
        assert_ne!(spinner(epoch, epoch), spinner(epoch, epoch + SPINNER_FRAME));
    }
}
