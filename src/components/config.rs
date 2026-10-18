use ratatui::{
    Frame,
    crossterm::event::{Event, KeyCode},
    layout::{Constraint::{Length, Percentage}, HorizontalAlignment::Center, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Clear, Paragraph},
};
use ratatui_textarea::TextArea;

use crate::{
    components::{block, theme},
    core::{Language, UserConfig},
};

pub struct ConfigMenu {
    pub language: Language,
    pub extra_flags: TextArea<'static>,
    pub active_field: usize,
}

impl ConfigMenu {
    pub fn new(config: &UserConfig) -> Self {
        let mut extra_flags = TextArea::default();
        extra_flags.insert_str(config.extra_flags.join(" "));

        Self {
            language: config.language,
            extra_flags,
            active_field: 0,
        }
    }

    pub fn to_config(&self) -> UserConfig {
        UserConfig {
            language: self.language,
            extra_flags: self
                .extra_flags
                .lines()
                .join(" ")
                .split_whitespace()
                .map(str::to_string)
                .collect(),
        }
    }

    pub fn draw(&mut self, frame: &mut Frame, area: Rect) {
        let popup_area = area.centered(Percentage(60), Length(8));
        frame.render_widget(Clear, popup_area);

        let popup_block = block(Some(" Config "))
            .border_style(theme::ACCENT)
            .title_style(Style::new().fg(theme::ACCENT).bold())
            .title_alignment(Center);
        let popup_inner = popup_block.inner(popup_area);
        frame.render_widget(popup_block, popup_area);

        let divisions = Layout::vertical([Length(3), Length(3)]).split(popup_inner);

        let lang_active = self.active_field == 0;
        let lang_color = if lang_active { theme::GREEN } else { theme::BORDER };
        let lang_panel = Paragraph::new(Line::from(vec![
            Span::styled(if lang_active { " ◀ " } else { "   " }, Style::new().fg(lang_color)),
            Span::styled(self.language.label(), Style::new().bold().fg(if lang_active { theme::GREEN } else { Color::Reset })),
            Span::styled(if lang_active { " ▶ " } else { "" }, Style::new().fg(lang_color)),
        ]))
        .alignment(Center)
        .block(block(Some(" Language ")).border_style(lang_color).title_alignment(Center));
        frame.render_widget(lang_panel, divisions[0]);

        let flags_active = self.active_field == 1;
        let (flags_color, cursor_style) = if flags_active {
            (theme::GREEN, Style::new().add_modifier(Modifier::REVERSED))
        } else {
            (theme::BORDER, Style::new())
        };
        self.extra_flags
            .set_block(block(Some(" Extra compile flags ")).border_style(flags_color).title_alignment(Center));
        self.extra_flags.set_cursor_style(cursor_style);
        frame.render_widget(&self.extra_flags, divisions[1]);
    }

    pub fn handle_key(&mut self, event: Event) -> bool {
        if let Event::Key(key) = event {
            match key.code {
                KeyCode::Tab => {
                    self.active_field = (self.active_field + 1) % 2;
                }
                KeyCode::Left if self.active_field == 0 => {
                    self.language = self.language.prev();
                }
                KeyCode::Right if self.active_field == 0 => {
                    self.language = self.language.next();
                }
                KeyCode::Esc => return true,
                _ => {
                    if self.active_field == 1 {
                        self.extra_flags.input(key);
                    }
                }
            }
        }
        false
    }
}
