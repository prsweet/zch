//! Palettes, as semantic tokens.
//!
//! Four themes ship: **prsweet** (pure black and the user's Zed syntax
//! colours), **catppuccin** (Mocha), **obsidian** (graphite and
//! periwinkle) and **daylight** (the one light theme). Every colour in the UI is a role on the active palette —
//! "the accent", "an error" — never a literal, so switching the theme in the
//! config sheet recolours everything at once.
//!
//! Roles are RGB so motion can blend between them; [`paint`] turns one into
//! a terminal colour, falling back to the nearest 256-colour index on
//! terminals without truecolor. `NO_COLOR` is honoured by crossterm, and no
//! state is told by colour alone.

use std::sync::{
    OnceLock,
    atomic::{AtomicU8, Ordering},
};

use ratatui::style::Color;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);

const fn hex(value: u32) -> Rgb {
    Rgb((value >> 16) as u8, (value >> 8) as u8, value as u8)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemeName {
    #[default]
    Prsweet,
    #[serde(alias = "mocha", alias = "Mocha", alias = "catppuccin-mocha")]
    Catppuccin,
    Obsidian,
    Daylight,
}

impl ThemeName {
    pub const ALL: [ThemeName; 4] = [
        ThemeName::Prsweet,
        ThemeName::Catppuccin,
        ThemeName::Obsidian,
        ThemeName::Daylight,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ThemeName::Prsweet => "prsweet",
            ThemeName::Catppuccin => "catppuccin",
            ThemeName::Obsidian => "obsidian",
            ThemeName::Daylight => "daylight",
        }
    }

    pub fn next(self) -> ThemeName {
        let index = Self::ALL.iter().position(|t| *t == self).unwrap_or(0);
        Self::ALL[(index + 1) % Self::ALL.len()]
    }

    pub fn prev(self) -> ThemeName {
        let index = Self::ALL.iter().position(|t| *t == self).unwrap_or(0);
        Self::ALL[(index + Self::ALL.len() - 1) % Self::ALL.len()]
    }

    fn palette(self) -> &'static Palette {
        match self {
            ThemeName::Prsweet => &PRSWEET,
            ThemeName::Catppuccin => &CATPPUCCIN,
            ThemeName::Obsidian => &OBSIDIAN,
            ThemeName::Daylight => &DAYLIGHT,
        }
    }
}

/// Every role the UI paints with.
#[derive(Debug, Clone, Copy)]
pub struct Palette {
    pub bg: Rgb,
    /// Bars that frame the screen (app bar, console, submissions).
    pub surface: Rgb,
    /// Behind every dialog: lifted off the canvas.
    pub card: Rgb,
    /// Inside a case's got box: a quiet lift, so the program's answer reads
    /// apart from the input and expected boxes beside it.
    pub got: Rgb,
    pub border: Rgb,
    pub faint: Rgb,
    pub muted: Rgb,
    pub text: Rgb,
    pub accent: Rgb,
    pub success: Rgb,
    pub error: Rgb,
    pub warning: Rgb,
    pub info: Rgb,
    /// Compile errors: the user's to fix, not a verdict.
    pub modified: Rgb,
    /// Submissions and other "out in the world" states.
    pub purple: Rgb,
    /// Background behind characters that differ in a wrong answer.
    pub diff: Rgb,
    /// The wordmark's colour sweep, left to right.
    pub signature: [Rgb; 3],
}

/// The user's Zed theme: a pure black canvas and the syntax palette —
/// method cyan, function teal, type green, keyword rose, string gold, macro
/// orange, variant violet — with cool slate greys from comments and
/// punctuation instead of warm gruvbox ones.
pub const PRSWEET: Palette = Palette {
    bg: hex(0x000000),
    surface: hex(0x10131b),
    card: hex(0x1b1f2c),
    got: hex(0x131824),
    border: hex(0x30364a),
    faint: hex(0x6b7280),
    muted: hex(0x8b93a7),
    text: hex(0xedf2f7),
    accent: hex(0x2ac3de),
    success: hex(0x9ece6a),
    error: hex(0xff4d6d),
    warning: hex(0xe0af68),
    info: hex(0x7dcfff),
    modified: hex(0xff9e64),
    purple: hex(0xbb9af7),
    diff: hex(0x3d0f1a),
    signature: [hex(0x2ac3de), hex(0x4fd6be), hex(0xbb9af7)],
};

/// Catppuccin Mocha, layered darkest-first like the other themes: the
/// canvas is crust, bars are mantle, and dialogs and the got box are base.
pub const CATPPUCCIN: Palette = Palette {
    bg: hex(0x11111b),
    surface: hex(0x181825),
    card: hex(0x1e1e2e),
    got: hex(0x1e1e2e),
    border: hex(0x313244),
    faint: hex(0x7f849c),
    muted: hex(0xa6adc8),
    text: hex(0xcdd6f4),
    accent: hex(0x89b4fa),
    success: hex(0xa6e3a1),
    error: hex(0xf38ba8),
    warning: hex(0xf9e2af),
    info: hex(0x74c7ec),
    modified: hex(0xfab387),
    purple: hex(0xcba6f7),
    diff: hex(0x45243a),
    signature: [hex(0x89b4fa), hex(0xcba6f7), hex(0xf5c2e7)],
};

/// The original zch look: obsidian canvas, graphite surfaces, periwinkle.
pub const OBSIDIAN: Palette = Palette {
    bg: hex(0x090b0f),
    surface: hex(0x10141b),
    card: hex(0x161b24),
    got: hex(0x10161d),
    border: hex(0x2e3848),
    faint: hex(0x5d6a7e),
    muted: hex(0x7e8a9e),
    text: hex(0xe8eef7),
    accent: hex(0x8cb8ff),
    success: hex(0x8fe3a2),
    error: hex(0xff7d92),
    warning: hex(0xf0cf7a),
    info: hex(0x7fe0cd),
    modified: hex(0xffa66f),
    purple: hex(0xc8a9ff),
    diff: hex(0x3a1820),
    signature: [hex(0x6f9edd), hex(0x8cb8ff), hex(0xc8a9ff)],
};

/// The light theme: a cool off-white canvas, white dialogs and ink-dark
/// text. Every state colour is deepened until it reads on the canvas, and
/// the canvas reads on it (the console's mode badge puts one on the other).
pub const DAYLIGHT: Palette = Palette {
    bg: hex(0xf5f6f8),
    surface: hex(0xe9ecf1),
    card: hex(0xffffff),
    got: hex(0xeaf0f7),
    border: hex(0xc9d0db),
    faint: hex(0x7c8597),
    muted: hex(0x545d6e),
    text: hex(0x1b2130),
    accent: hex(0x00708f),
    success: hex(0x346c10),
    error: hex(0xc0233f),
    warning: hex(0x8f5c00),
    info: hex(0x0b6f80),
    modified: hex(0xb0480a),
    purple: hex(0x6f45c4),
    diff: hex(0xfadfe5),
    signature: [hex(0x00708f), hex(0x0b8a73), hex(0x6f45c4)],
};

#[cfg(not(test))]
static ACTIVE: AtomicU8 = AtomicU8::new(0);

// Tests run in parallel; each keeps its own theme so one switching the
// palette can never recolour another mid-assertion.
#[cfg(test)]
thread_local! {
    static ACTIVE: AtomicU8 = const { AtomicU8::new(0) };
}

fn with_active<T>(f: impl FnOnce(&AtomicU8) -> T) -> T {
    #[cfg(not(test))]
    {
        f(&ACTIVE)
    }
    #[cfg(test)]
    {
        ACTIVE.with(|active| f(active))
    }
}

/// Switch the whole UI to `theme`.
pub fn set(theme: ThemeName) {
    let index = ThemeName::ALL.iter().position(|t| *t == theme).unwrap_or(0);
    with_active(|active| active.store(index as u8, Ordering::Relaxed));
}

pub fn active() -> ThemeName {
    let index = with_active(|active| active.load(Ordering::Relaxed)) as usize;
    ThemeName::ALL[index % ThemeName::ALL.len()]
}

/// The active palette.
pub fn p() -> &'static Palette {
    active().palette()
}

pub fn bg() -> Color {
    paint(p().bg)
}
pub fn surface() -> Color {
    paint(p().surface)
}
/// The background of every dialog: the paper the content sits on.
pub fn card() -> Color {
    paint(p().card)
}
/// Inside the got box.
pub fn got_rgb() -> Rgb {
    p().got
}
pub fn got() -> Color {
    paint(got_rgb())
}
pub fn border() -> Color {
    paint(p().border)
}
pub fn text() -> Color {
    paint(p().text)
}
pub fn muted() -> Color {
    paint(p().muted)
}
pub fn faint() -> Color {
    paint(p().faint)
}
pub fn accent() -> Color {
    paint(p().accent)
}
pub fn error() -> Color {
    paint(p().error)
}
pub fn warning() -> Color {
    paint(p().warning)
}
pub fn info() -> Color {
    paint(p().info)
}
pub fn purple() -> Color {
    paint(p().purple)
}
pub fn diff() -> Color {
    paint(p().diff)
}

/// Linear blend from `from` (t = 0) to `to` (t = 1).
pub fn mix(from: Rgb, to: Rgb, t: f32) -> Rgb {
    let t = t.clamp(0.0, 1.0);
    let lerp = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t).round() as u8;
    Rgb(lerp(from.0, to.0), lerp(from.1, to.1), lerp(from.2, to.2))
}

/// [`mix`], painted.
pub fn blend(from: Rgb, to: Rgb, t: f32) -> Color {
    paint(mix(from, to, t))
}

/// A point along a multi-stop gradient, `t` in `[0, 1]`.
pub fn gradient(stops: &[Rgb], t: f32) -> Rgb {
    match stops.len() {
        0 => Rgb(0, 0, 0),
        1 => stops[0],
        n => {
            let t = t.clamp(0.0, 1.0) * (n - 1) as f32;
            let index = (t as usize).min(n - 2);
            mix(stops[index], stops[index + 1], t - index as f32)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Depth {
    TrueColor,
    Ansi256,
}

fn depth() -> Depth {
    static DEPTH: OnceLock<Depth> = OnceLock::new();
    *DEPTH.get_or_init(|| {
        let var = |name: &str| std::env::var(name).unwrap_or_default().to_ascii_lowercase();
        match var("ZCH_COLOR").as_str() {
            "truecolor" | "24bit" => return Depth::TrueColor,
            "256" => return Depth::Ansi256,
            _ => {}
        }
        let colorterm = var("COLORTERM");
        let program = var("TERM_PROGRAM");
        let term = var("TERM");
        let truecolor = colorterm.contains("truecolor")
            || colorterm.contains("24bit")
            || [
                "zed",
                "iterm.app",
                "wezterm",
                "ghostty",
                "vscode",
                "hyper",
                "warpterminal",
            ]
            .contains(&program.as_str())
            || ["kitty", "alacritty", "ghostty", "wezterm", "direct"]
                .iter()
                .any(|name| term.contains(name));
        // Tests render into a buffer, not a terminal: keep the real values.
        if truecolor || cfg!(test) {
            Depth::TrueColor
        } else {
            Depth::Ansi256
        }
    })
}

/// The terminal colour for `rgb` at this terminal's colour depth.
pub fn paint(rgb: Rgb) -> Color {
    match depth() {
        Depth::TrueColor => Color::Rgb(rgb.0, rgb.1, rgb.2),
        Depth::Ansi256 => Color::Indexed(nearest_256(rgb)),
    }
}

/// Nearest xterm-256 index: the 6×6×6 cube or the 24-step grey ramp,
/// whichever is closer. Pure black maps to 16, never to the user's own
/// palette slot 0, so the canvas stays black on every terminal theme.
fn nearest_256(Rgb(r, g, b): Rgb) -> u8 {
    const STEPS: [u8; 6] = [0, 95, 135, 175, 215, 255];
    let cube_index = |value: u8| {
        STEPS
            .iter()
            .enumerate()
            .min_by_key(|(_, step)| (i32::from(**step) - i32::from(value)).abs())
            .map(|(index, _)| index as u8)
            .unwrap_or(0)
    };
    let (ri, gi, bi) = (cube_index(r), cube_index(g), cube_index(b));
    let cube = Rgb(STEPS[ri as usize], STEPS[gi as usize], STEPS[bi as usize]);
    let average = (u16::from(r) + u16::from(g) + u16::from(b)) / 3;
    let grey_index = ((average.saturating_sub(8)) / 10).min(23) as u8;
    let grey_value = 8 + 10 * grey_index;
    let grey = Rgb(grey_value, grey_value, grey_value);

    let distance = |a: Rgb| {
        let d = |x: u8, y: u8| (i32::from(x) - i32::from(y)).pow(2);
        d(a.0, r) + d(a.1, g) + d(a.2, b)
    };
    if distance(grey) < distance(cube) {
        232 + grey_index
    } else {
        16 + 36 * ri + 6 * gi + bi
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mixing_runs_between_the_endpoints() {
        let (black, white) = (hex(0x000000), hex(0xffffff));
        assert_eq!(mix(black, white, 0.0), black);
        assert_eq!(mix(black, white, 1.0), white);
        assert_eq!(mix(black, white, 7.0), white, "clamped");
        assert_eq!(mix(black, Rgb(200, 100, 50), 0.5), Rgb(100, 50, 25));
    }

    #[test]
    fn gradients_pass_through_every_stop() {
        let stops = PRSWEET.signature;
        assert_eq!(gradient(&stops, 0.0), stops[0]);
        assert_eq!(gradient(&stops, 0.5), stops[1]);
        assert_eq!(gradient(&stops, 1.0), stops[2]);
    }

    #[test]
    fn prsweet_uses_the_syntax_colours_not_gruvbox() {
        assert_eq!(PRSWEET.bg, hex(0x000000));
        assert_eq!(PRSWEET.text, hex(0xedf2f7));
        assert_eq!(PRSWEET.error, hex(0xff4d6d), "keyword rose");
        assert_eq!(PRSWEET.success, hex(0x9ece6a), "type green");
        assert_eq!(PRSWEET.warning, hex(0xe0af68), "string gold");
        for gruvbox in [0x504945, 0x928374, 0x665c54, 0x83a598, 0x98971a, 0xfabd2f] {
            let gruvbox = hex(gruvbox);
            let palette = PRSWEET;
            for role in [
                palette.border,
                palette.muted,
                palette.faint,
                palette.accent,
                palette.success,
                palette.warning,
            ] {
                assert_ne!(role, gruvbox);
            }
        }
    }

    #[test]
    fn every_theme_keeps_its_text_legible() {
        fn luminance(Rgb(r, g, b): Rgb) -> f32 {
            let channel = |c: u8| {
                let c = f32::from(c) / 255.0;
                if c <= 0.04045 {
                    c / 12.92
                } else {
                    ((c + 0.055) / 1.055).powf(2.4)
                }
            };
            0.2126 * channel(r) + 0.7152 * channel(g) + 0.0722 * channel(b)
        }
        fn contrast(a: Rgb, b: Rgb) -> f32 {
            let (a, b) = (luminance(a), luminance(b));
            (a.max(b) + 0.05) / (a.min(b) + 0.05)
        }
        for theme in ThemeName::ALL {
            let p = theme.palette();
            for (role, colour) in [
                ("text", p.text),
                ("muted", p.muted),
                ("accent", p.accent),
                ("success", p.success),
                ("error", p.error),
                ("warning", p.warning),
                ("info", p.info),
            ] {
                for (ground, under) in [("bg", p.bg), ("got", p.got), ("card", p.card)] {
                    let ratio = contrast(colour, under);
                    assert!(
                        ratio >= 4.5,
                        "{} {role} on {ground}: {ratio:.2}",
                        theme.label()
                    );
                }
            }
            assert!(contrast(p.faint, p.bg) >= 3.0, "{} faint", theme.label());
        }
    }

    #[test]
    fn theme_names_round_trip_and_cycle() {
        for theme in ThemeName::ALL {
            let json = serde_json::to_string(&theme).unwrap();
            assert_eq!(serde_json::from_str::<ThemeName>(&json).unwrap(), theme);
            assert_eq!(theme.next().prev(), theme);
        }
        assert_eq!(
            serde_json::from_str::<ThemeName>("\"Mocha\"").unwrap(),
            ThemeName::Catppuccin
        );
    }

    #[test]
    fn the_256_colour_fallback_is_close() {
        assert_eq!(nearest_256(hex(0x000000)), 16);
        assert_eq!(nearest_256(hex(0xffffff)), 231);
        assert!(
            nearest_256(PRSWEET.accent) < 232,
            "the accent keeps its hue"
        );
    }
}
