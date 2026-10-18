use ratatui::{
    crossterm::event::MouseEvent,
    layout::Rect,
    style::{Color, Style},
    widgets::{Block, BorderType, Borders, Padding},
};

pub mod config;
pub mod dir_terminal;
pub mod splash;
pub mod tc_list;
pub mod testcase;

/// Catppuccin Mocha — fixed truecolor values (no ANSI palette lookups), so the
/// UI renders identically on every machine regardless of terminal theme.
pub mod theme {
    use ratatui::style::Color;

    pub const BORDER: Color = Color::Rgb(0x45, 0x47, 0x5a);
    pub const MUTED: Color = Color::Rgb(0x7f, 0x84, 0x9c);
    pub const TEXT: Color = Color::Rgb(0xcd, 0xd6, 0xf4);
    pub const ACCENT: Color = Color::Rgb(0x89, 0xb4, 0xfa);
    pub const GREEN: Color = Color::Rgb(0xa6, 0xe3, 0xa1);
    pub const RED: Color = Color::Rgb(0xf3, 0x8b, 0xa8);
    pub const YELLOW: Color = Color::Rgb(0xf9, 0xe2, 0xaf);
    pub const ORANGE: Color = Color::Rgb(0xfa, 0xb3, 0x87);
    pub const PURPLE: Color = Color::Rgb(0xcb, 0xa6, 0xf7);
    pub const CYAN: Color = Color::Rgb(0x94, 0xe2, 0xd5);
}

/// Nerd Font glyphs — private-use codepoints, so every machine with a patched
/// font shows the exact same icons (no OS-dependent emoji artwork).
pub mod icons {
    pub const FOLDER: &str = "\u{f07b}";
    pub const FILE: &str = "\u{f15b}";
}

pub fn block(title: Option<&str>) -> Block<'static> {
    let if_title = title.unwrap_or("");
    Block::default()
        .title(if_title.to_string())
        .borders(Borders::ALL)
        .title_style(Style::new().fg(theme::MUTED).bold())
        .border_style(theme::BORDER)
        .border_type(BorderType::Rounded)
        .padding(Padding::symmetric(1, 0))
}

#[derive(Debug, Clone)]
pub struct Message {
    pub color: Color,
    pub text: String,
}

impl Message {
    pub fn new(color: Color, text: impl Into<String>) -> Self {
        Self { color, text: text.into() }
    }
}

pub struct VisualState {
    pub rect: Rect,
}

impl VisualState {
    pub fn new() -> Self {
        Self { rect: Rect::default() }
    }

    pub fn check_hover(&self, mouse: &MouseEvent) -> bool {
        self.rect.width > 0
            && self.rect.height > 0
            && mouse.column >= self.rect.x
            && mouse.column < self.rect.x + self.rect.width
            && mouse.row >= self.rect.y
            && mouse.row < self.rect.y + self.rect.height
    }
}
