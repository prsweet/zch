use std::collections::HashMap;

use ratatui::{
    Frame,
    crossterm::event::{Event, KeyCode, KeyModifiers, MouseButton, MouseEvent, MouseEventKind},
    layout::{
        Constraint::{Length, Percentage},
        Layout, Position, Rect,
    },
    style::Style,
    text::{Line, Span},
    widgets::{Clear, Paragraph, ScrollbarOrientation, ScrollbarState, Wrap},
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
    language_choices: Vec<(Rect, usize)>,
    theme_choices: Vec<(Rect, usize)>,
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
            language_choices: Vec::new(),
            theme_choices: Vec::new(),
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

    pub fn draw(&mut self, frame: &mut Frame, area: Rect) {
        // A fixed comfortable width uses the available space in editor splits.
        // Short terminals reveal the focused section instead of clipping it away.
        self.flags_area = Rect::default();
        self.flags_scrollbar_area = Rect::default();
        self.language_choices.clear();
        self.theme_choices.clear();
        let margin = if area.width >= 42 { 4 } else { 0 };
        let width = 74.min(area.width.saturating_sub(margin));
        let max_height = area.height.saturating_sub(u16::from(area.height >= 18) * 2);
        let height = (17 + self.visible_flag_rows() as u16).min(max_height);
        let popup = area.centered(Length(width), Length(height));
        frame.render_widget(Clear, popup);
        let panel = block(Some(" Settings "))
            .style(Style::new().fg(theme::TEXT()).bg(theme::SURFACE()))
            .border_style(theme::BORDER())
            .title_style(Style::new().fg(theme::TEXT()).bold());
        let inner = panel.inner(popup);
        frame.render_widget(panel, popup);
        if inner.is_empty() {
            return;
        }

        let deleting = self.delete_stage != DeleteStage::Off;
        let active = if deleting { 1 } else { self.active_field };
        let mut language = Rect::default();
        let mut flags = Rect::default();
        let mut draft = Rect::default();
        let mut appearance = Rect::default();
        let hint;

        if inner.height >= 16 {
            let rows = Layout::vertical([
                Length(1),
                Length(1),
                Length(3),
                Length(inner.height - 13),
                Length(3),
                Length(3),
                Length(2),
            ])
            .split(inner);
            frame.render_widget(
                Paragraph::new("Build tools & appearance").style(Style::new().fg(theme::MUTED())),
                rows[0],
            );
            language = rows[2];
            flags = rows[3];
            draft = rows[4];
            appearance = rows[5];
            hint = rows[6];
        } else if inner.height >= 9 {
            let rows = Layout::vertical([
                Length(1),
                Length(inner.height - 7),
                Length(3),
                Length(1),
                Length(2),
            ])
            .split(inner);
            language = rows[0];
            flags = rows[1];
            draft = rows[2];
            appearance = rows[3];
            hint = rows[4];
        } else {
            let hint_height = inner.height.saturating_sub(1).min(2);
            let rows = Layout::vertical([Length(inner.height - hint_height), Length(hint_height)])
                .split(inner);
            hint = rows[1];
            match active {
                0 => language = rows[0],
                2 => appearance = rows[0],
                _ if deleting => flags = rows[0],
                _ => {
                    let rows =
                        Layout::vertical([Length(rows[0].height.saturating_sub(3)), Length(3)])
                            .split(rows[0]);
                    flags = rows[0];
                    draft = rows[1];
                }
            }
        }

        self.language_choices = Self::draw_choice(
            frame,
            language,
            "Language",
            self.language.label(),
            &Language::ALL
                .iter()
                .map(Language::label)
                .collect::<Vec<_>>(),
            active == 0,
        );
        self.draw_flags(frame, flags);
        self.draw_draft(frame, draft, active == 1 && !deleting);
        self.theme_choices = Self::draw_choice(
            frame,
            appearance,
            "Theme",
            self.theme.label(),
            &[AppTheme::Mocha.label(), AppTheme::Prsweet.label()],
            active == 2,
        );
        self.draw_hint(frame, hint);
    }

    fn draw_choice(
        frame: &mut Frame,
        area: Rect,
        label: &str,
        value: &str,
        options: &[&str],
        active: bool,
    ) -> Vec<(Rect, usize)> {
        if area.is_empty() {
            return Vec::new();
        }
        let color = if active {
            theme::ACCENT()
        } else {
            theme::BORDER()
        };
        let choices_width: usize = options
            .iter()
            .map(|option| Line::from(*option).width() + 4)
            .sum();
        let mut spans = Vec::new();
        let compact = area.height < 3;
        let content = if compact {
            area
        } else {
            block(None).inner(area)
        };
        let content_width = content.width as usize;
        let mut targets = Vec::new();
        let mut x = content.x;
        if compact {
            spans.push(Span::styled(
                format!("{label:<10}"),
                Style::new().fg(theme::MUTED()),
            ));
            x = x.saturating_add(10);
        }
        if !compact && choices_width <= content_width {
            for (index, option) in options.iter().enumerate() {
                let selected = *option == value;
                let style = if selected && active {
                    Style::new().fg(theme::BASE()).bg(theme::ACCENT()).bold()
                } else if selected {
                    Style::new().fg(theme::TEXT()).bg(theme::ELEVATED()).bold()
                } else {
                    Style::new().fg(theme::MUTED())
                };
                spans.push(Span::styled(format!(" {option} "), style));
                spans.push(Span::raw("  "));
                let width = Line::from(*option).width() as u16 + 2;
                targets.push((
                    Rect::new(x, content.y, width, 1).intersection(content),
                    index,
                ));
                x = x.saturating_add(width + 2);
            }
        } else {
            let selected = options
                .iter()
                .position(|option| *option == value)
                .unwrap_or(0);
            if active && !options.is_empty() {
                targets.push((
                    Rect::new(x, content.y, 1, 1).intersection(content),
                    step_wrapped(selected, options.len(), true),
                ));
            }
            x = x.saturating_add(2);
            let value_width = Line::from(value).width() as u16;
            targets.push((
                Rect::new(x, content.y, value_width, 1).intersection(content),
                selected,
            ));
            spans.push(Span::styled(
                if active { "‹ " } else { "  " },
                Style::new().fg(color),
            ));
            spans.push(Span::styled(
                value.to_string(),
                Style::new()
                    .fg(if active {
                        theme::ACCENT()
                    } else {
                        theme::TEXT()
                    })
                    .bold(),
            ));
            if active {
                spans.push(Span::styled(" ›", Style::new().fg(color)));
                if !options.is_empty() {
                    targets.push((
                        Rect::new(x.saturating_add(value_width + 1), content.y, 1, 1)
                            .intersection(content),
                        step_wrapped(selected, options.len(), false),
                    ));
                }
            }
        }
        let mut choice = Paragraph::new(Line::from(spans));
        if !compact {
            choice = choice.block(
                block(Some(&format!(" {label} ")))
                    .border_style(color)
                    .title_style(Style::new().fg(if active {
                        theme::ACCENT()
                    } else {
                        theme::MUTED()
                    })),
            );
        }
        frame.render_widget(choice, area);
        targets.retain(|(rect, _)| !rect.is_empty());
        targets
    }

    fn draw_flags(&mut self, frame: &mut Frame, area: Rect) {
        if area.is_empty() {
            return;
        }
        let deleting = self.delete_stage != DeleteStage::Off;
        let compact = area.height < 3;
        let inner = if compact {
            area
        } else {
            let title = format!(" Compile flags · {} ", self.flags.len());
            let panel = block(Some(&title))
                .border_style(if deleting {
                    theme::RED()
                } else {
                    theme::BORDER()
                })
                .title_style(Style::new().fg(if deleting {
                    theme::RED()
                } else {
                    theme::MUTED()
                }));
            let inner = panel.inner(area);
            frame.render_widget(panel, area);
            inner
        };
        self.flags_area = inner;
        let visible = inner.height as usize;
        if visible == 0 {
            return;
        }
        self.ensure_flag_visible_with(visible);
        if self.flags.is_empty() {
            self.flags_scroll_drag = None;
            frame.render_widget(
                Paragraph::new("No extra flags").style(Style::new().fg(theme::MUTED())),
                inner,
            );
            return;
        }
        let needs_scrollbar = self.flags.len() > visible && inner.width >= 2;
        let (list_area, scrollbar_area) = if needs_scrollbar {
            let columns = Layout::horizontal([Percentage(100), Length(1)]).split(inner);
            (columns[0], columns[1])
        } else {
            self.flags_scroll_drag = None;
            (inner, Rect::default())
        };
        self.flags_scrollbar_area = scrollbar_area;
        let lines: Vec<Line> = self
            .flags
            .iter()
            .enumerate()
            .skip(self.flag_scroll)
            .take(visible)
            .map(|(index, flag)| {
                let selected = index == self.flag_cursor && deleting;
                let style = if selected && self.delete_stage == DeleteStage::Confirm {
                    Style::new().fg(theme::BASE()).bg(theme::RED()).bold()
                } else if selected {
                    Style::new().fg(theme::RED()).bg(theme::ELEVATED()).bold()
                } else {
                    Style::new().fg(theme::TEXT())
                };
                let marker = if selected { "▸ " } else { "  " };
                Line::styled(
                    fit_label(&format!("{marker}{flag}"), list_area.width as usize),
                    style,
                )
            })
            .collect();
        frame.render_widget(Paragraph::new(lines), list_area);
        if needs_scrollbar {
            let mut state =
                ScrollbarState::new(self.flags.len() - visible).position(self.flag_scroll);
            frame.render_stateful_widget(
                scrollbar(ScrollbarOrientation::VerticalRight),
                scrollbar_area,
                &mut state,
            );
        }
    }

    fn draw_draft(&mut self, frame: &mut Frame, area: Rect, active: bool) {
        if area.is_empty() {
            return;
        }
        let label = if self.flags.last().is_some_and(|flag| takes_value(flag)) {
            " Add value "
        } else {
            " Add flag "
        };
        let panel = block(Some(label))
            .border_style(if active {
                theme::ACCENT()
            } else {
                theme::BORDER()
            })
            .title_style(Style::new().fg(if active {
                theme::ACCENT()
            } else {
                theme::MUTED()
            }));
        if active {
            self.draft
                .set_style(Style::new().fg(theme::TEXT()).bg(theme::ELEVATED()));
            self.draft.set_cursor_line_style(Style::new());
            self.draft
                .set_cursor_style(Style::new().fg(theme::BASE()).bg(theme::ACCENT()));
            self.draft.set_placeholder_text("Type a flag, then Enter");
            self.draft
                .set_placeholder_style(Style::new().fg(theme::MUTED()));
            if area.height >= 3 {
                self.draft.set_block(panel);
            } else {
                self.draft.remove_block();
            }
            frame.render_widget(&self.draft, area);
        } else {
            let content = if self.draft.is_empty() {
                "Type a flag, then Enter".to_string()
            } else {
                self.draft.lines().join(" ")
            };
            let mut text = Paragraph::new(content).style(Style::new().fg(theme::MUTED()));
            if area.height >= 3 {
                text = text.block(panel);
            }
            frame.render_widget(text, area);
        }
    }

    fn draw_hint(&self, frame: &mut Frame, area: Rect) {
        if area.is_empty() {
            return;
        }
        let (text, color) = if let Some(error) = &self.error {
            (format!("! {error}"), theme::RED())
        } else {
            let text = match self.delete_stage {
                DeleteStage::Select => "Tab select · Enter review · Esc cancel".to_string(),
                DeleteStage::Confirm => "Enter delete · Backspace back".to_string(),
                DeleteStage::Off => {
                    let action = if self.active_field == 1 {
                        "Enter add"
                    } else {
                        "←/→ change"
                    };
                    if area.width >= 60 {
                        format!("Tab next field · {action} · x remove flag · Esc save & close")
                    } else if area.width >= 32 {
                        format!("Tab next · {action}\nEsc save & close · x remove flag")
                    } else if area.width >= 20 {
                        let action = if self.active_field == 1 {
                            "Enter add"
                        } else {
                            "←/→"
                        };
                        format!("Tab next · {action}\nEsc save · x remove")
                    } else {
                        "Tab next\nEsc save".to_string()
                    }
                }
            };
            (
                text,
                if self.delete_stage == DeleteStage::Off {
                    theme::MUTED()
                } else {
                    theme::RED()
                },
            )
        };
        frame.render_widget(
            Paragraph::new(text)
                .style(Style::new().fg(color))
                .wrap(Wrap { trim: true }),
            area,
        );
    }

    /// Mouse wheel and scrollbar dragging. Returns true when the event was
    /// consumed so the caller does not pass it to the test-case list.
    pub fn handle_mouse(&mut self, mouse: &MouseEvent) -> bool {
        let position = Position::new(mouse.column, mouse.row);
        if matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left))
            && self.delete_stage == DeleteStage::Off
        {
            if let Some((_, index)) = self
                .language_choices
                .iter()
                .find(|(rect, _)| rect.contains(position))
            {
                let language = Language::ALL[*index];
                self.switch_language(language);
                // A rejected draft keeps the user in the field that needs fixing.
                self.active_field = if self.language == language { 0 } else { 1 };
                return true;
            }
            if let Some((_, index)) = self
                .theme_choices
                .iter()
                .find(|(rect, _)| rect.contains(position))
            {
                self.theme = [AppTheme::Mocha, AppTheme::Prsweet][*index];
                theme::apply(self.theme);
                self.active_field = 2;
                return true;
            }
        }
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
                        "Flags must start with - (or follow a flag that takes a value): '{token}'"
                    ));
                    return false;
                }
            } else {
                let lower = token.to_ascii_lowercase();
                if self.flags.iter().any(|flag| flag == token)
                    || pending.iter().any(|flag| flag == token)
                {
                    self.error = Some(format!("Flag is already in the list: '{token}'"));
                    return false;
                }
                if lower == "-std" || lower == "--std" {
                    self.error = Some("write -std=c++20 (no space)".to_string());
                    return false;
                }
                if (lower.starts_with("-std=") || lower.starts_with("--std="))
                    && std_from_flag(token).is_none()
                {
                    self.error = Some(format!("invalid standard (try c++20): '{token}'"));
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

/// Ellipsize by terminal cells so long flag values cannot run into a scrollbar.
fn fit_label(text: &str, width: usize) -> String {
    if Line::from(text).width() <= width {
        return text.to_string();
    }
    if width == 0 {
        return String::new();
    }
    let mut result = String::new();
    for ch in text.chars() {
        if Line::from(format!("{result}{ch}…")).width() > width {
            break;
        }
        result.push(ch);
    }
    result.push('…');
    result
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
    fn focused_fields_remain_usable_in_short_editor_splits() {
        for width in [28, 40, 80, 120] {
            for height in [6, 8, 12, 18, 24] {
                let mut menu = menu_with(&[]);
                for (field, content) in [(0, "C++"), (1, "-DDEBUG"), (2, "Mocha")] {
                    menu.active_field = field;
                    if field == 1 {
                        menu.draft.insert_str("-DDEBUG");
                    }
                    let rendered = screen(&mut menu, width, height);
                    assert!(
                        rendered.contains(content),
                        "focused field {field} disappeared at {width}x{height}:\n{rendered}"
                    );
                    assert!(
                        rendered.contains("Esc save"),
                        "save action disappeared at {width}x{height}:\n{rendered}"
                    );
                }
            }
        }
    }

    #[test]
    fn compact_errors_explain_the_fix_and_preserve_the_draft() {
        let mut menu = menu_with(&[]);
        menu.handle_key(key(KeyCode::Tab));
        let invalid = "unprefixed".repeat(12);
        type_text(&mut menu, &invalid);
        menu.handle_key(key(KeyCode::Enter));
        for (width, height) in [(40, 8), (80, 24)] {
            let rendered = screen(&mut menu, width, height);
            assert!(rendered.contains("must start with -"), "{rendered}");
            assert!(menu.flags.is_empty());
            assert_eq!(menu.draft.lines().join(""), invalid);
        }
        assert!(
            !menu.handle_key(key(KeyCode::Esc)),
            "invalid drafts cannot silently save"
        );
    }

    #[test]
    fn compact_delete_keeps_the_selected_flag_and_confirmation_visible() {
        let mut menu = menu_with(&["-O2", "-Wall", "-std=c++20"]);
        menu.handle_key(key(KeyCode::Char('x')));
        let selected = screen(&mut menu, 40, 8);
        assert!(selected.contains("-std=c++20"), "{selected}");
        assert!(selected.contains("Enter review"), "{selected}");
        menu.handle_key(key(KeyCode::Enter));
        let confirm = screen(&mut menu, 40, 8);
        assert!(confirm.contains("-std=c++20"), "{confirm}");
        assert!(confirm.contains("Enter delete"), "{confirm}");
        assert_eq!(menu.flags.len(), 3);
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

    fn click_text(menu: &mut ConfigMenu, rendered: &str, text: &str) -> bool {
        let (row, line) = rendered
            .lines()
            .enumerate()
            .find(|(_, line)| line.contains(text))
            .unwrap();
        let column = Line::from(&line[..line.find(text).unwrap()]).width();
        menu.handle_mouse(&mouse(
            MouseEventKind::Down(MouseButton::Left),
            column as u16,
            row as u16,
        ))
    }

    #[test]
    fn rendered_choice_clicks_validate_drafts_select_and_focus() {
        let mut menu = menu_with(&[]);
        menu.handle_key(key(KeyCode::Tab));
        type_text(&mut menu, "invalid");
        let rendered = screen(&mut menu, 80, 28);
        assert!(click_text(&mut menu, &rendered, "Python 3"));
        assert_eq!(
            menu.language,
            Language::Cpp,
            "a click must not discard an invalid draft"
        );
        assert_eq!(menu.active_field, 1);
        assert!(menu.error.is_some());
        assert_eq!(menu.draft.lines().join(""), "invalid");

        menu.draft = TextArea::default();
        type_text(&mut menu, "-O2");
        let rendered = screen(&mut menu, 80, 28);
        assert!(click_text(&mut menu, &rendered, "Python 3"));
        assert_eq!(menu.language, Language::Python);
        assert_eq!(menu.active_field, 0);
        assert_eq!(menu.flags_by_language[&Language::Cpp], ["-O2"]);
        assert!(menu.draft.is_empty());

        let rendered = screen(&mut menu, 80, 28);
        let original_accent = theme::ACCENT();
        assert!(click_text(&mut menu, &rendered, "prsweet's favourite"));
        assert_eq!(menu.theme, AppTheme::Prsweet);
        assert_eq!(menu.active_field, 2);
        assert_ne!(
            theme::ACCENT(),
            original_accent,
            "the preview updates immediately"
        );
        theme::apply(AppTheme::Mocha);
    }

    #[test]
    fn compact_selector_arrows_work_and_hidden_or_blank_choices_do_nothing() {
        let mut menu = menu_with(&[]);
        let rendered = screen(&mut menu, 40, 12);
        assert!(click_text(&mut menu, &rendered, "›"));
        assert_eq!(menu.language, Language::Python);
        let rendered = screen(&mut menu, 40, 12);
        assert!(click_text(&mut menu, &rendered, "Mocha"));
        assert_eq!(menu.active_field, 2);
        let rendered = screen(&mut menu, 40, 12);
        assert!(click_text(&mut menu, &rendered, "›"));
        assert_eq!(menu.theme, AppTheme::Prsweet);

        // Shrinking to a focused-only view removes the old language targets.
        let old_language = menu.language_choices[0].0;
        let _ = screen(&mut menu, 40, 8);
        assert!(menu.language_choices.is_empty());
        menu.handle_mouse(&mouse(
            MouseEventKind::Down(MouseButton::Left),
            old_language.x,
            old_language.y,
        ));
        assert_eq!(menu.language, Language::Python);
        assert_eq!(menu.active_field, 2);

        let _ = screen(&mut menu, 80, 28);
        let (first, _) = menu.language_choices[0];
        assert!(
            !menu.handle_mouse(&mouse(
                MouseEventKind::Down(MouseButton::Left),
                first.right(),
                first.y
            )),
            "the gap after a chip is not a choice"
        );
        assert_eq!(menu.active_field, 2);
        theme::apply(AppTheme::Mocha);
    }

    #[test]
    fn choice_clicks_cannot_bypass_flag_delete_confirmation() {
        let mut menu = menu_with(&["-O2"]);
        menu.handle_key(key(KeyCode::Char('x')));
        menu.handle_key(key(KeyCode::Enter));
        let rendered = screen(&mut menu, 80, 28);
        assert!(!click_text(&mut menu, &rendered, "Python 3"));
        assert!(!click_text(&mut menu, &rendered, "prsweet's favourite"));
        assert_eq!(menu.language, Language::Cpp);
        assert_eq!(menu.theme, AppTheme::Mocha);
        assert_eq!(menu.delete_stage, DeleteStage::Confirm);
        assert_eq!(menu.flags, ["-O2"]);
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
