use std::collections::HashMap;

use ratatui::{
    Frame,
    crossterm::event::{Event, KeyCode, KeyModifiers, MouseButton, MouseEvent, MouseEventKind},
    layout::{
        Constraint::{Length, Percentage},
        HorizontalAlignment::Center,
        Layout, Position,
    },
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Clear, Paragraph, ScrollbarOrientation, ScrollbarState},
};
use ratatui_textarea::TextArea;

use crate::{
    components::{ScrollbarDrag, block, scroll_to_show, scrollbar, step_wrapped, theme},
    core::{AppTheme, Language, UserConfig, std_from_flag},
};

const MIN_VISIBLE_FLAGS: usize = 3;
const MAX_VISIBLE_FLAGS: usize = 8;

/// Where the flag list is in the `x`-deletion flow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DeleteStage {
    Off,
    Select,
    Confirm,
}

pub struct ConfigMenu {
    pub language: Language,
    pub theme: AppTheme,
    pub flags: Vec<String>,
    pub flags_by_language: HashMap<Language, Vec<String>>,
    pub draft: TextArea<'static>,
    pub active_field: usize,
    pub error: Option<String>,
    pub flag_cursor: usize,
    flag_scroll: usize,
    flags_area: ratatui::layout::Rect,
    flags_scrollbar_area: ratatui::layout::Rect,
    flags_scroll_drag: Option<ScrollbarDrag>,
    delete_stage: DeleteStage,
}

impl ConfigMenu {
    pub fn new(config: &UserConfig) -> Self {
        let mut flags_by_language = config.flags_by_language.clone();
        let flags = flags_by_language
            .entry(config.language)
            .or_insert_with(|| config.extra_flags.clone())
            .clone();
        Self {
            language: config.language,
            theme: config.theme,
            flag_cursor: flags.len().saturating_sub(1),
            flags,
            flags_by_language,
            draft: TextArea::default(),
            active_field: 0,
            error: None,
            flag_scroll: 0,
            flags_area: ratatui::layout::Rect::default(),
            flags_scrollbar_area: ratatui::layout::Rect::default(),
            flags_scroll_drag: None,
            delete_stage: DeleteStage::Off,
        }
    }

    pub fn to_config(&self) -> UserConfig {
        let mut flags_by_language = self.flags_by_language.clone();
        flags_by_language.insert(self.language, self.flags.clone());
        UserConfig {
            language: self.language,
            extra_flags: self.flags.clone(),
            flags_by_language,
            theme: self.theme,
        }
    }

    fn switch_language(&mut self, language: Language) {
        if language == self.language {
            return;
        }
        if !self.draft.is_empty() && !self.commit_draft() {
            return;
        }
        self.flags_by_language
            .insert(self.language, self.flags.clone());
        self.language = language;
        self.flags = self
            .flags_by_language
            .get(&language)
            .cloned()
            .unwrap_or_default();
        self.flag_cursor = self.flags.len().saturating_sub(1);
        self.flag_scroll = 0;
        self.draft = TextArea::default();
        self.error = None;
        self.delete_stage = DeleteStage::Off;
    }

    /// The flags panel grows with the list (up to `MAX_VISIBLE_FLAGS`) so
    /// there is always room to show several tags; beyond that it scrolls.
    fn visible_flag_rows(&self) -> usize {
        self.flags.len().clamp(MIN_VISIBLE_FLAGS, MAX_VISIBLE_FLAGS)
    }

    fn ensure_flag_visible_with(&mut self, visible: usize) {
        if self.flags.is_empty() {
            self.flag_cursor = 0;
            self.flag_scroll = 0;
            return;
        }
        self.flag_cursor = self.flag_cursor.min(self.flags.len() - 1);
        self.flag_scroll = scroll_to_show(
            self.flag_cursor,
            self.flag_scroll,
            visible.max(1),
            self.flags.len(),
        );
    }

    fn move_flag_cursor(&mut self, backwards: bool) {
        if self.flags.is_empty() {
            return;
        }
        self.flag_cursor = step_wrapped(self.flag_cursor, self.flags.len(), backwards);
        self.ensure_flag_visible_with(self.visible_flag_rows());
    }

    /// A flag that takes a value owns the bare tag right after it; deleting
    /// either one removes the pair, never a dangling flag.
    fn flag_delete_range(&self, index: usize) -> (usize, usize) {
        if !self.flags[index].starts_with('-') {
            if index > 0 && takes_value(&self.flags[index - 1]) {
                return (index - 1, index + 1);
            }
            return (index, index + 1);
        }
        if takes_value(&self.flags[index])
            && index + 1 < self.flags.len()
            && !self.flags[index + 1].starts_with('-')
        {
            return (index, index + 2);
        }
        (index, index + 1)
    }

    fn remove_selected_flag(&mut self) {
        if self.flags.is_empty() {
            self.delete_stage = DeleteStage::Off;
            return;
        }
        let index = self.flag_cursor.min(self.flags.len() - 1);
        let (start, end) = self.flag_delete_range(index);
        self.flags.drain(start..end);
        self.flag_cursor = start.min(self.flags.len().saturating_sub(1));
        self.delete_stage = DeleteStage::Off;
        self.error = None;
        self.ensure_flag_visible_with(self.visible_flag_rows());
    }

    pub fn draw(&mut self, frame: &mut Frame, area: ratatui::layout::Rect) {
        let max_popup = area.height.saturating_sub(2).max(11);
        let desired_flags = self.visible_flag_rows() as u16 + 2;
        let popup_height = (12 + desired_flags).min(max_popup);
        let flags_height = popup_height.saturating_sub(12).max(2);
        let popup_area = area.centered(Percentage(64), Length(popup_height));
        frame.render_widget(Clear, popup_area);

        let popup_block = block(Some(" Config "))
            .border_style(theme::ACCENT())
            .title_style(Style::new().fg(theme::ACCENT()).bold())
            .title_alignment(Center);
        let popup_inner = popup_block.inner(popup_area);
        frame.render_widget(popup_block, popup_area);

        let divisions = Layout::vertical([
            Length(3),
            Length(flags_height),
            Length(3),
            Length(3),
            Length(1),
        ])
        .split(popup_inner);

        let lang_active = self.active_field == 0;
        let lang_color = if lang_active {
            theme::GREEN()
        } else {
            theme::BORDER()
        };
        let lang_panel = Paragraph::new(Line::from(vec![
            Span::styled(
                if lang_active { " ◀ " } else { "   " },
                Style::new().fg(lang_color),
            ),
            Span::styled(
                self.language.label(),
                Style::new().bold().fg(if lang_active {
                    theme::GREEN()
                } else {
                    Color::Reset
                }),
            ),
            Span::styled(
                if lang_active { " ▶ " } else { "" },
                Style::new().fg(lang_color),
            ),
        ]))
        .alignment(Center)
        .block(
            block(Some(" Language "))
                .border_style(lang_color)
                .title_alignment(Center),
        );
        frame.render_widget(lang_panel, divisions[0]);

        let flags_active = self.active_field == 1;
        let deleting = self.delete_stage != DeleteStage::Off;
        let flags_color = if deleting {
            theme::RED()
        } else if flags_active {
            theme::GREEN()
        } else {
            theme::BORDER()
        };
        let list_height = flags_height.saturating_sub(2) as usize;
        self.ensure_flag_visible_with(list_height);

        let flags_block = block(Some(" Extra compile flags "))
            .border_style(flags_color)
            .title_alignment(Center);
        let flags_inner = flags_block.inner(divisions[1]);
        self.flags_area = flags_inner;
        self.flags_scrollbar_area = ratatui::layout::Rect::default();
        if self.flags.is_empty() {
            self.flags_scroll_drag = None;
        }
        frame.render_widget(flags_block, divisions[1]);

        if self.flags.is_empty() {
            let empty = Paragraph::new(Line::from(Span::styled(
                "no flags — type one below and press Enter",
                Style::new().fg(theme::MUTED()).italic(),
            )));
            frame.render_widget(empty, flags_inner);
        } else {
            let columns = Layout::horizontal([Percentage(100), Length(1)]).split(flags_inner);
            let needs_scrollbar = self.flags.len() > list_height;
            let (list_area, scrollbar_area) = if needs_scrollbar {
                (columns[0], columns[1])
            } else {
                (flags_inner, ratatui::layout::Rect::default())
            };
            self.flags_scrollbar_area = scrollbar_area;
            if !needs_scrollbar {
                self.flags_scroll_drag = None;
            }

            let end = (self.flag_scroll + list_height).min(self.flags.len());
            let mut lines: Vec<Line> = Vec::new();
            for index in self.flag_scroll..end {
                let selected = index == self.flag_cursor;
                let style = match (self.delete_stage, selected) {
                    (DeleteStage::Confirm, true) => {
                        Style::new().fg(Color::Black).bg(theme::RED()).bold()
                    }
                    (DeleteStage::Select, true) => Style::new().fg(theme::RED()).bold(),
                    (_, true) if flags_active => Style::new().fg(theme::GREEN()).bold(),
                    (_, true) => Style::new().fg(theme::TEXT()).bold(),
                    _ => Style::new().fg(theme::TEXT()),
                };
                let marker = if selected && deleting { "▸ " } else { "  " };
                lines.push(Line::from(vec![
                    Span::styled(marker, style),
                    Span::styled(self.flags[index].clone(), style),
                ]));
            }
            frame.render_widget(Paragraph::new(lines), list_area);

            if needs_scrollbar {
                let mut state =
                    ScrollbarState::new(self.flags.len() - list_height).position(self.flag_scroll);
                frame.render_stateful_widget(
                    scrollbar(ScrollbarOrientation::VerticalRight),
                    scrollbar_area,
                    &mut state,
                );
            }
        }

        let draft_color = if flags_active && !deleting {
            theme::GREEN()
        } else {
            theme::BORDER()
        };
        self.draft.set_style(Style::new().fg(theme::TEXT()));
        self.draft.set_block(
            block(Some(" Add flag "))
                .border_style(draft_color)
                .title_alignment(Center),
        );
        self.draft.set_cursor_style(if flags_active && !deleting {
            Style::new().add_modifier(Modifier::REVERSED)
        } else {
            Style::new()
        });
        frame.render_widget(&self.draft, divisions[2]);

        let theme_active = self.active_field == 2;
        let theme_color = if theme_active {
            theme::GREEN()
        } else {
            theme::BORDER()
        };
        let theme_panel = Paragraph::new(Line::from(vec![
            Span::styled(
                if theme_active { " ◀ " } else { "   " },
                Style::new().fg(theme_color),
            ),
            Span::styled(
                self.theme.label(),
                Style::new().bold().fg(if theme_active {
                    theme::GREEN()
                } else {
                    Color::Reset
                }),
            ),
            Span::styled(
                if theme_active { " ▶ " } else { "" },
                Style::new().fg(theme_color),
            ),
        ]))
        .alignment(Center)
        .block(
            block(Some(" Theme "))
                .border_style(theme_color)
                .title_alignment(Center),
        );
        frame.render_widget(theme_panel, divisions[3]);

        let hint = match &self.error {
            Some(error) => Line::from(Span::styled(
                format!(" ✘ {error}"),
                Style::new().fg(theme::RED()),
            )),
            None => match self.delete_stage {
                DeleteStage::Select => Line::from(Span::styled(
                    " Tab selects · Enter confirms · Esc cancels",
                    Style::new().fg(theme::RED()),
                )),
                DeleteStage::Confirm => {
                    let flag = self
                        .flags
                        .get(self.flag_cursor)
                        .map(String::as_str)
                        .unwrap_or("");
                    Line::from(Span::styled(
                        format!(" Enter deletes {flag} · Backspace backs out"),
                        Style::new().fg(theme::RED()).bold(),
                    ))
                }
                DeleteStage::Off => Line::from(""),
            },
        };
        frame.render_widget(Paragraph::new(hint), divisions[4]);
    }

    /// Mouse wheel and scrollbar dragging. Returns true when the event was
    /// consumed so the caller does not pass it to the test-case list.
    pub fn handle_mouse(&mut self, mouse: &MouseEvent) -> bool {
        let position = Position::new(mouse.column, mouse.row);
        if matches!(mouse.kind, MouseEventKind::Up(MouseButton::Left)) {
            self.flags_scroll_drag = None;
        }
        if matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left))
            && !self.flags_scrollbar_area.contains(position)
        {
            self.flags_scroll_drag = None;
        }
        if self.flags_scrollbar_area.contains(position) {
            match mouse.kind {
                MouseEventKind::Down(MouseButton::Left) => {
                    let max_scroll = self
                        .flags
                        .len()
                        .saturating_sub(self.flags_area.height as usize);
                    if let Some((drag, scroll)) = ScrollbarDrag::begin(
                        self.flags_scrollbar_area,
                        true,
                        max_scroll,
                        self.flag_scroll,
                        position,
                    ) {
                        self.flags_scroll_drag = Some(drag);
                        self.set_flag_scroll(scroll);
                    }
                }
                MouseEventKind::Drag(MouseButton::Left) => {
                    if let Some(drag) = self.flags_scroll_drag {
                        let scroll = drag.scroll_at(position);
                        self.set_flag_scroll(scroll);
                    }
                }
                MouseEventKind::Up(MouseButton::Left) => self.flags_scroll_drag = None,
                MouseEventKind::ScrollDown => self.move_flag_cursor(false),
                MouseEventKind::ScrollUp => self.move_flag_cursor(true),
                _ => {}
            }
            return true;
        }

        if self.flags_area.contains(position) {
            match mouse.kind {
                MouseEventKind::ScrollDown => self.move_flag_cursor(false),
                MouseEventKind::ScrollUp => self.move_flag_cursor(true),
                _ => {}
            }
            return true;
        }
        false
    }

    fn set_flag_scroll(&mut self, scroll: usize) {
        let visible = self.flags_area.height as usize;
        let max_scroll = self.flags.len().saturating_sub(visible);
        self.flag_scroll = scroll.min(max_scroll);
        if !self.flags.is_empty() {
            self.flag_cursor = self.flag_scroll.min(self.flags.len() - 1);
        }
    }

    pub fn handle_key(&mut self, event: Event) -> bool {
        let Event::Key(key) = event else { return false };

        // The `x` deletion flow owns every key while it is open, exactly like
        // the test-case delete mode: select, confirm, and only then delete.
        match self.delete_stage {
            DeleteStage::Select => {
                match key.code {
                    KeyCode::BackTab => self.move_flag_cursor(true),
                    KeyCode::Tab => {
                        let backwards = key.modifiers.contains(KeyModifiers::SHIFT);
                        self.move_flag_cursor(backwards);
                    }
                    KeyCode::Enter => self.delete_stage = DeleteStage::Confirm,
                    KeyCode::Esc => {
                        self.delete_stage = DeleteStage::Off;
                        self.error = None;
                    }
                    _ => {}
                }
                return false;
            }
            DeleteStage::Confirm => {
                match key.code {
                    KeyCode::Enter => self.remove_selected_flag(),
                    KeyCode::Esc | KeyCode::Backspace => self.delete_stage = DeleteStage::Select,
                    _ => {}
                }
                return false;
            }
            DeleteStage::Off => {}
        }

        match key.code {
            KeyCode::Tab | KeyCode::BackTab => {
                let backwards =
                    key.code == KeyCode::BackTab || key.modifiers.contains(KeyModifiers::SHIFT);
                self.active_field = step_wrapped(self.active_field, 3, backwards);
                self.error = None;
            }
            KeyCode::Left if self.active_field == 0 => {
                self.switch_language(self.language.prev());
            }
            KeyCode::Right if self.active_field == 0 => {
                self.switch_language(self.language.next());
            }
            KeyCode::Left if self.active_field == 2 => {
                self.theme = self.theme.prev();
                theme::apply(self.theme);
            }
            KeyCode::Right if self.active_field == 2 => {
                self.theme = self.theme.next();
                theme::apply(self.theme);
            }
            KeyCode::Char('x') if self.draft.is_empty() && !self.flags.is_empty() => {
                self.flag_cursor = self.flag_cursor.min(self.flags.len() - 1);
                self.ensure_flag_visible_with(self.visible_flag_rows());
                self.delete_stage = DeleteStage::Select;
                self.error = None;
            }
            KeyCode::Enter if self.active_field == 1 => {
                self.commit_draft();
            }
            KeyCode::Esc => {
                if self.active_field == 1 && !self.draft.is_empty() && !self.commit_draft() {
                    return false;
                }
                return true;
            }
            _ => {
                if self.active_field == 1 {
                    self.draft.input(key);
                    self.error = None;
                }
            }
        }
        false
    }

    /// Turn the draft into tags, one per whitespace-separated flag. Rejects
    /// anything that is not flag-shaped instead of silently passing it to the
    /// compiler later. A bare value is only accepted directly after a flag
    /// that takes one (e.g. `-include-pch /path/to/stdc++.h.pch`).
    fn commit_draft(&mut self) -> bool {
        let raw = self.draft.lines().join(" ");
        let mut pending: Vec<String> = Vec::new();
        let mut previous = self.flags.last().cloned();

        for token in raw.split_whitespace() {
            let is_value = !token.starts_with('-');
            if is_value {
                if !previous.as_deref().is_some_and(takes_value) {
                    self.error = Some(format!(
                        "'{token}' is not a flag (it must start with -, or follow a flag that takes a value)"
                    ));
                    return false;
                }
            } else {
                let lower = token.to_ascii_lowercase();
                if self.flags.iter().any(|flag| flag == token)
                    || pending.iter().any(|flag| flag == token)
                {
                    self.error = Some(format!("'{token}' is already in the list"));
                    return false;
                }
                if lower == "-std" || lower == "--std" {
                    self.error = Some("write -std=c++20 (no space)".to_string());
                    return false;
                }
                if (lower.starts_with("-std=") || lower.starts_with("--std="))
                    && std_from_flag(token).is_none()
                {
                    self.error = Some(format!("invalid standard '{token}' (try c++20)"));
                    return false;
                }
            }
            pending.push(token.to_string());
            previous = Some(token.to_string());
        }

        if !pending.is_empty() {
            self.flags.extend(pending);
            self.flag_cursor = self.flags.len() - 1;
            self.ensure_flag_visible_with(self.visible_flag_rows());
            self.draft = TextArea::default();
            self.error = None;
        }
        true
    }
}

/// Flags whose value is a separate argument rather than joined with `=`.
fn takes_value(flag: &str) -> bool {
    matches!(
        flag,
        "-include-pch" | "-include" | "-isystem" | "-I" | "-D" | "-U" | "-L" | "-x" | "-MF" | "-MT"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{
        Terminal,
        backend::TestBackend,
        crossterm::event::{KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind},
    };

    fn key(code: KeyCode) -> Event {
        Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    fn screen(menu: &mut ConfigMenu, width: u16, height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| menu.draw(frame, frame.area()))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let screen = (0..height)
            .map(|y| {
                (0..width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        if std::env::var("ZCH_DUMP").is_ok() {
            println!("{screen}");
        }
        screen
    }

    fn type_text(menu: &mut ConfigMenu, text: &str) {
        for c in text.chars() {
            menu.handle_key(key(KeyCode::Char(c)));
        }
    }

    fn menu_with(flags: &[&str]) -> ConfigMenu {
        ConfigMenu::new(&UserConfig {
            language: Language::Cpp,
            extra_flags: flags.iter().map(|flag| flag.to_string()).collect(),
            ..Default::default()
        })
    }

    #[test]
    fn shift_tab_cycles_fields_backwards() {
        let mut menu = menu_with(&[]);
        menu.handle_key(key(KeyCode::Tab));
        assert_eq!(menu.active_field, 1);
        menu.handle_key(Event::Key(KeyEvent::new(
            KeyCode::BackTab,
            KeyModifiers::SHIFT,
        )));
        assert_eq!(menu.active_field, 0);
        menu.handle_key(Event::Key(KeyEvent::new(KeyCode::Tab, KeyModifiers::SHIFT)));
        assert_eq!(
            menu.active_field, 2,
            "a terminal that reports Shift+Tab as Tab must still go back"
        );
    }

    #[test]
    fn theme_section_sits_under_the_flag_draft() {
        let mut menu = menu_with(&[]);
        let opened = screen(&mut menu, 80, 28);
        assert!(opened.contains("Theme"), "{opened}");
        assert!(opened.contains("Mocha"), "{opened}");
        menu.handle_key(key(KeyCode::Tab));
        menu.handle_key(key(KeyCode::Tab));
        assert_eq!(menu.active_field, 2);
        menu.handle_key(key(KeyCode::Right));
        assert_eq!(menu.theme, AppTheme::Prsweet);
        assert_eq!(menu.to_config().theme, AppTheme::Prsweet);
        let chosen = screen(&mut menu, 80, 28);
        assert!(chosen.contains("prsweet's favourite"), "{chosen}");
        menu.handle_key(key(KeyCode::Left));
        assert_eq!(menu.theme, AppTheme::Mocha);
    }

    #[test]
    fn adds_flag_tags_and_derives_the_cf_language_id() {
        let mut menu = menu_with(&[]);
        menu.handle_key(key(KeyCode::Tab));
        assert_eq!(menu.active_field, 1);

        type_text(&mut menu, "-O2");
        menu.handle_key(key(KeyCode::Enter));
        type_text(&mut menu, "-std=c++20");
        menu.handle_key(key(KeyCode::Enter));

        assert_eq!(menu.flags, vec!["-O2", "-std=c++20"]);
        assert_eq!(menu.language.cf_id(&menu.flags), "89");
        assert_eq!(menu.to_config().extra_flags, menu.flags);
        assert!(menu.draft.is_empty());
    }

    #[test]
    fn flags_follow_the_selected_language() {
        let mut menu = menu_with(&["-O2"]);

        menu.handle_key(key(KeyCode::Right));
        assert_eq!(menu.language, Language::Python);
        assert!(menu.flags.is_empty());

        menu.handle_key(key(KeyCode::Tab));
        type_text(&mut menu, "-u");
        menu.handle_key(key(KeyCode::Enter));
        menu.handle_key(key(KeyCode::Tab));
        menu.handle_key(key(KeyCode::Tab));

        menu.handle_key(key(KeyCode::Left));
        assert_eq!(menu.language, Language::Cpp);
        assert_eq!(menu.flags, ["-O2"]);

        menu.handle_key(key(KeyCode::Right));
        assert_eq!(menu.language, Language::Python);
        assert_eq!(menu.flags, ["-u"]);

        let config = menu.to_config();
        assert_eq!(config.extra_flags, ["-u"]);
        assert_eq!(config.flags_by_language[&Language::Cpp], ["-O2"]);
        assert_eq!(config.flags_by_language[&Language::Python], ["-u"]);
    }

    #[test]
    fn switching_language_commits_a_pending_draft_first() {
        let mut menu = menu_with(&[]);
        menu.handle_key(key(KeyCode::Tab));
        type_text(&mut menu, "-O2");
        menu.handle_key(key(KeyCode::Tab));
        menu.handle_key(key(KeyCode::Tab));

        menu.handle_key(key(KeyCode::Right));
        assert_eq!(menu.language, Language::Python);
        assert!(menu.flags.is_empty());

        menu.handle_key(key(KeyCode::Left));
        assert_eq!(menu.language, Language::Cpp);
        assert_eq!(menu.flags, ["-O2"]);
    }

    #[test]
    fn rejects_non_flags_and_duplicates() {
        let mut menu = menu_with(&["-O2"]);
        menu.handle_key(key(KeyCode::Tab));

        type_text(&mut menu, "O2");
        menu.handle_key(key(KeyCode::Enter));
        assert!(
            menu.error
                .as_deref()
                .unwrap_or_default()
                .contains("must start with -")
        );
        assert_eq!(menu.flags, vec!["-O2"]);

        menu.draft = TextArea::default();
        type_text(&mut menu, "-O2");
        menu.handle_key(key(KeyCode::Enter));
        assert!(
            menu.error
                .as_deref()
                .unwrap_or_default()
                .contains("already")
        );
        assert_eq!(menu.flags, vec!["-O2"]);
    }

    #[test]
    fn rejects_unknown_std_versions() {
        let mut menu = menu_with(&[]);
        menu.handle_key(key(KeyCode::Tab));

        type_text(&mut menu, "-std=c++1000");
        menu.handle_key(key(KeyCode::Enter));
        assert!(
            menu.error
                .as_deref()
                .unwrap_or_default()
                .contains("invalid standard")
        );
        assert!(menu.flags.is_empty());
        assert_eq!(menu.draft.lines().join(""), "-std=c++1000");

        menu.draft = TextArea::default();
        type_text(&mut menu, "-std");
        menu.handle_key(key(KeyCode::Enter));
        assert!(
            menu.error
                .as_deref()
                .unwrap_or_default()
                .contains("no space")
        );
        assert!(menu.flags.is_empty());
    }

    #[test]
    fn backspace_only_edits_the_draft() {
        let mut menu = menu_with(&["-O2", "-Wall"]);
        menu.handle_key(key(KeyCode::Tab));

        // Deleting tags goes through `x`; Backspace never removes one.
        menu.handle_key(key(KeyCode::Backspace));
        assert_eq!(menu.flags, vec!["-O2", "-Wall"]);

        type_text(&mut menu, "-DDEBUG");
        menu.handle_key(key(KeyCode::Backspace));
        assert_eq!(menu.flags, vec!["-O2", "-Wall"]);
        assert_eq!(menu.draft.lines().join(""), "-DDEBU");
    }

    #[test]
    fn x_selects_and_deletes_a_flag_with_confirmation() {
        let mut menu = menu_with(&["-O2", "-Wall", "-std=c++20"]);
        assert_eq!(menu.flag_cursor, 2, "the newest flag starts selected");

        menu.handle_key(key(KeyCode::Up));
        assert_eq!(
            menu.flag_cursor, 2,
            "arrows do not move the flag focus before x"
        );

        menu.handle_key(key(KeyCode::Char('x')));
        assert!(matches!(menu.delete_stage, DeleteStage::Select));

        menu.handle_key(key(KeyCode::BackTab));
        assert_eq!(menu.flag_cursor, 1);
        menu.handle_key(key(KeyCode::Enter));
        assert!(matches!(menu.delete_stage, DeleteStage::Confirm));
        assert_eq!(menu.flags.len(), 3, "confirmation must not delete yet");

        // Only Enter deletes; x is just the way into the flow.
        menu.handle_key(key(KeyCode::Char('x')));
        assert!(matches!(menu.delete_stage, DeleteStage::Confirm));
        assert_eq!(menu.flags.len(), 3);

        // Esc steps back to selection, Esc again cancels entirely.
        menu.handle_key(key(KeyCode::Esc));
        assert!(matches!(menu.delete_stage, DeleteStage::Select));
        menu.handle_key(key(KeyCode::Esc));
        assert!(matches!(menu.delete_stage, DeleteStage::Off));
        assert_eq!(menu.flags.len(), 3, "cancelling deletes nothing");

        menu.flag_cursor = 0;
        menu.handle_key(key(KeyCode::Char('x')));
        menu.handle_key(key(KeyCode::Enter));
        menu.handle_key(key(KeyCode::Enter));
        assert_eq!(menu.flags, ["-Wall", "-std=c++20"]);
        assert!(matches!(menu.delete_stage, DeleteStage::Off));
    }

    #[test]
    fn deleting_a_value_flag_removes_its_value_too() {
        let mut menu = menu_with(&["-O2", "-include-pch", "/tmp/stdc++.h.pch"]);
        assert_eq!(menu.flag_cursor, 2);

        // Selecting the bare value deletes the flag that owns it.
        menu.handle_key(key(KeyCode::Char('x')));
        assert_eq!(menu.flag_cursor, 2);
        menu.handle_key(key(KeyCode::Enter));
        menu.handle_key(key(KeyCode::Enter));
        assert_eq!(menu.flags, ["-O2"]);

        // Selecting the flag deletes its value along with it.
        let mut menu = menu_with(&["-include-pch", "/tmp/stdc++.h.pch", "-O2"]);
        menu.flag_cursor = 0;
        menu.handle_key(key(KeyCode::Char('x')));
        menu.handle_key(key(KeyCode::Enter));
        menu.handle_key(key(KeyCode::Enter));
        assert_eq!(menu.flags, ["-O2"]);
    }

    #[test]
    fn x_is_a_character_while_typing_a_tag() {
        let mut menu = menu_with(&["-O2"]);
        menu.handle_key(key(KeyCode::Tab));
        type_text(&mut menu, "-Px");

        assert!(matches!(menu.delete_stage, DeleteStage::Off));
        assert_eq!(menu.draft.lines().join(""), "-Px");
        assert_eq!(menu.flags, ["-O2"]);
    }

    #[test]
    fn every_flag_is_reachable_in_the_scrolling_list() {
        let flags: Vec<String> = (0..10).map(|i| format!("-DOPT{i}")).collect();
        let mut menu = ConfigMenu::new(&UserConfig {
            language: Language::Cpp,
            extra_flags: flags,
            ..Default::default()
        });

        // The list auto-scrolls to the selected (newest) flag: eight rows are
        // visible at once, the rest stay reachable with the scrollbar.
        let initial = screen(&mut menu, 100, 30);
        assert!(initial.contains("-DOPT9"));
        assert!(!initial.contains("-DOPT0"));

        menu.handle_key(key(KeyCode::Char('x')));
        let deleting = screen(&mut menu, 100, 30);
        assert!(deleting.contains("-DOPT9"));
        assert!(deleting.contains('▸'));

        // Walking back nine times reaches the first flag and scrolls it in.
        for _ in 0..9 {
            menu.handle_key(key(KeyCode::BackTab));
        }
        assert_eq!(menu.flag_cursor, 0);
        let top = screen(&mut menu, 100, 30);
        assert!(top.contains("-DOPT0"));
        assert!(!top.contains("-DOPT9"));
    }

    fn mouse(kind: MouseEventKind, column: u16, row: u16) -> MouseEvent {
        MouseEvent {
            kind,
            column,
            row,
            modifiers: KeyModifiers::NONE,
        }
    }

    #[test]
    fn scrollbar_drag_and_wheel_move_the_flag_list() {
        let flags: Vec<String> = (0..20).map(|i| format!("-DOPT{i}")).collect();
        let mut menu = ConfigMenu::new(&UserConfig {
            language: Language::Cpp,
            extra_flags: flags,
            ..Default::default()
        });
        menu.flag_scroll = 0;
        menu.flag_cursor = 0;
        let _ = screen(&mut menu, 100, 30);

        let area = menu.flags_scrollbar_area;
        assert!(area.height > 2, "the flag list must expose a scrollbar");
        let bottom = area.y + area.height - 2;

        menu.handle_mouse(&mouse(
            MouseEventKind::Down(MouseButton::Left),
            area.x,
            bottom,
        ));
        let clicked = menu.flag_scroll;
        assert!(clicked > 0, "clicking the track must seek down");

        menu.handle_mouse(&mouse(
            MouseEventKind::Drag(MouseButton::Left),
            area.x,
            bottom - 2,
        ));
        let dragged = menu.flag_scroll;
        assert!(dragged < clicked, "dragging up must scroll back");
        assert!(dragged > 0, "a short drag must not jump to the top");

        menu.handle_mouse(&mouse(
            MouseEventKind::Up(MouseButton::Left),
            area.x,
            bottom - 2,
        ));
        assert!(menu.flags_scroll_drag.is_none());

        menu.flag_cursor = 0;
        menu.flag_scroll = 0;
        let _ = screen(&mut menu, 100, 30);
        let row = menu.flags_area.y;
        menu.handle_mouse(&mouse(
            MouseEventKind::ScrollDown,
            menu.flags_area.x + 1,
            row,
        ));
        assert_eq!(menu.flag_cursor, 1, "the wheel moves the selected flag");
    }

    #[test]
    fn accepts_a_value_after_a_flag_that_takes_one() {
        let mut menu = menu_with(&[]);
        menu.handle_key(key(KeyCode::Tab));

        type_text(&mut menu, "-include-pch");
        menu.handle_key(key(KeyCode::Enter));
        type_text(&mut menu, "/tmp/stdc++.h.pch");
        menu.handle_key(key(KeyCode::Enter));
        assert_eq!(menu.flags, ["-include-pch", "/tmp/stdc++.h.pch"]);

        // A bare path after a flag that takes no value is rejected.
        type_text(&mut menu, "/tmp/other.pch");
        menu.handle_key(key(KeyCode::Enter));
        assert!(menu.error.is_some());
        assert_eq!(menu.flags, ["-include-pch", "/tmp/stdc++.h.pch"]);
    }

    #[test]
    fn esc_commits_a_pending_draft_before_closing() {
        let mut menu = menu_with(&[]);
        menu.handle_key(key(KeyCode::Tab));
        type_text(&mut menu, "-O2");

        assert!(menu.handle_key(key(KeyCode::Esc)));
        assert_eq!(menu.flags, vec!["-O2"]);
    }
}
