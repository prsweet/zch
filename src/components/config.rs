use std::collections::HashMap;

use ratatui::{
    Frame,
    crossterm::event::{Event, KeyCode, KeyModifiers, MouseButton, MouseEvent, MouseEventKind},
    layout::{Position, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Paragraph, ScrollbarOrientation, ScrollbarState},
};
use ratatui_textarea::TextArea;

use crate::{
    components::{
        ScrollbarDrag, icons, scroll_to_show, scrollbar, step_wrapped,
        theme::{self, ThemeName},
        ui,
    },
    core::{Language, UserConfig, std_from_flag},
};

/// The flags list is a normal fixed-height list box: adding tags never
/// resizes the sheet, and the wheel scrolls the viewport instead of walking
/// the selection.
const FLAG_ROWS: usize = 6;

/// Where the flag list is in the `x`-deletion flow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DeleteStage {
    Off,
    Select,
    Confirm,
}

pub struct ConfigMenu {
    pub language: Language,
    pub flags: Vec<String>,
    pub flags_by_language: HashMap<Language, Vec<String>>,
    pub draft: TextArea<'static>,
    pub active_field: usize,
    pub error: Option<String>,
    pub flag_cursor: usize,
    flag_scroll: usize,
    manual_scroll: bool,
    flags_area: ratatui::layout::Rect,
    flags_scrollbar_area: ratatui::layout::Rect,
    flags_scroll_drag: Option<ScrollbarDrag>,
    delete_stage: DeleteStage,
    pub theme: ThemeName,
    /// The config the menu was opened with.
    base: UserConfig,
}

/// Fields in Tab order: language, compile flags, theme.
const FIELDS: usize = 3;
const THEME_FIELD: usize = 2;

impl ConfigMenu {
    pub fn new(config: &UserConfig) -> Self {
        let mut flags_by_language = config.flags_by_language.clone();
        let flags = flags_by_language
            .entry(config.language)
            .or_insert_with(|| config.extra_flags.clone())
            .clone();
        Self {
            language: config.language,
            flag_cursor: flags.len().saturating_sub(1),
            flags,
            flags_by_language,
            draft: TextArea::default(),
            active_field: 0,
            error: None,
            flag_scroll: 0,
            manual_scroll: false,
            flags_area: ratatui::layout::Rect::default(),
            flags_scrollbar_area: ratatui::layout::Rect::default(),
            flags_scroll_drag: None,
            delete_stage: DeleteStage::Off,
            theme: config.theme,
            base: config.clone(),
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
            // Settings the menu does not edit are kept as they were.
            ..self.base.clone()
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
        self.manual_scroll = false;
        self.draft = TextArea::default();
        self.error = None;
        self.delete_stage = DeleteStage::Off;
    }

    /// The visible rows of the flags list. Falls back to the designed height
    /// before the first draw.
    fn flag_viewport(&self) -> usize {
        if self.flags_area.height > 0 {
            self.flags_area.height as usize
        } else {
            FLAG_ROWS
        }
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
        // A normal list stops at the ends instead of wrapping.
        self.flag_cursor = if backwards {
            self.flag_cursor.saturating_sub(1)
        } else {
            (self.flag_cursor + 1).min(self.flags.len() - 1)
        };
        self.manual_scroll = false;
        self.ensure_flag_visible_with(self.flag_viewport());
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
        self.manual_scroll = false;
        self.ensure_flag_visible_with(self.flag_viewport());
    }

    /// The keys the console bar shows while the config menu owns the input.
    pub fn key_hints(&self) -> &'static [(&'static str, &'static str)] {
        match self.delete_stage {
            DeleteStage::Select => &[("Tab", "select"), ("Enter", "confirm"), ("Esc", "cancel")],
            DeleteStage::Confirm => &[("Enter", "delete"), ("Esc", "back")],
            DeleteStage::Off if self.active_field == 0 => {
                &[("←→", "language"), ("Tab", "field"), ("Esc", "close")]
            }
            DeleteStage::Off if self.active_field == THEME_FIELD => {
                &[("←→", "theme"), ("Tab", "field"), ("Esc", "close")]
            }
            DeleteStage::Off => &[
                ("Enter", "add"),
                ("x", "delete"),
                ("Tab", "field"),
                ("Esc", "close"),
            ],
        }
    }

    /// ```text
    /// ▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀
    ///    Config
    ///
    ///    language        ‹ C++ ›
    ///
    ///    compile flags                3 flags
    ///      -std=c++20
    ///      -O2
    ///    ❯ add a flag
    ///
    ///    theme           ‹ prsweet ›  ●●●●●
    ///
    ///    submits as Codeforces language 89
    /// ```
    pub fn draw(&mut self, frame: &mut Frame, area: Rect, opened: f32) {
        let height = (4 + 9 + FLAG_ROWS as u16).min(area.height.saturating_sub(2));
        let width = (area.width * 7 / 10).clamp(46, 72);
        let body = ui::sheet(frame, area, "Config", width, height, opened);
        if body.height < 3 || body.width < 24 {
            return;
        }

        let list_height = body.height.saturating_sub(9) as usize;
        if list_height > 0 {
            self.flag_scroll = self
                .flag_scroll
                .min(self.flags.len().saturating_sub(list_height));
            if !self.manual_scroll {
                self.ensure_flag_visible_with(list_height);
            }
        }

        let rail = |frame: &mut Frame, y: u16, height: u16, color: Color| {
            for row in y..y.saturating_add(height).min(body.bottom()) {
                frame.buffer_mut()[(body.x, row)]
                    .set_symbol(icons::RAIL)
                    .set_style(Style::new().fg(color));
            }
        };
        let label = |active: bool, color: Color| {
            if active {
                Style::new().fg(color).bold()
            } else {
                Style::new().fg(theme::muted())
            }
        };
        let inner = |y: u16| Rect {
            x: body.x + 2,
            y,
            width: body.width.saturating_sub(2),
            height: 1,
        };

        // Language.
        let mut y = body.y;
        let lang_active = self.active_field == 0;
        if lang_active {
            rail(frame, y, 1, theme::accent());
        }
        let value = if lang_active {
            Line::from(vec![
                Span::styled("‹ ", Style::new().fg(theme::faint())),
                Span::styled(self.language.label(), Style::new().fg(theme::text()).bold()),
                Span::styled(" ›", Style::new().fg(theme::faint())),
            ])
        } else {
            Line::from(Span::styled(
                self.language.label(),
                Style::new().fg(theme::text()),
            ))
        };
        frame.render_widget(
            Paragraph::new(Line::from(vec![Span::styled(
                format!("{:<16}", "language"),
                label(lang_active, theme::accent()),
            )])),
            inner(y),
        );
        let value_area = Rect {
            x: body.x + 18,
            width: body.width.saturating_sub(18),
            ..inner(y)
        };
        frame.render_widget(Paragraph::new(value), value_area);
        y += 2;

        // Flags: label, list, add row.
        let flags_active = self.active_field == 1;
        let deleting = self.delete_stage != DeleteStage::Off;
        let flags_color = if deleting {
            theme::error()
        } else {
            theme::accent()
        };
        if flags_active {
            rail(frame, y, 2 + list_height as u16, flags_color);
        }
        let count = match (self.delete_stage, self.flags.len()) {
            (DeleteStage::Select, _) => "pick a flag to remove".to_string(),
            (DeleteStage::Confirm, _) => "remove it?".to_string(),
            (_, 0) => String::new(),
            (_, 1) => "1 flag".to_string(),
            (_, n) => format!("{n} flags"),
        };
        ui::row(
            frame,
            inner(y),
            Line::from(Span::styled(
                "compile flags",
                label(flags_active || deleting, flags_color),
            )),
            Line::from(Span::styled(
                count,
                Style::new().fg(if deleting {
                    theme::error()
                } else {
                    theme::faint()
                }),
            )),
        );
        y += 1;

        let list_area = Rect {
            x: body.x + 2,
            y,
            width: body.width.saturating_sub(2),
            height: list_height as u16,
        };
        self.flags_area = list_area;
        self.flags_scrollbar_area = Rect::default();
        if list_height > 0 {
            if self.flags.is_empty() {
                frame.render_widget(
                    Paragraph::new(Span::styled(
                        "  none — nothing is added behind your back",
                        Style::new().fg(theme::faint()),
                    )),
                    Rect {
                        height: 1,
                        ..list_area
                    },
                );
            } else {
                let needs_scrollbar = self.flags.len() > list_height;
                let width = list_area.width.saturating_sub(u16::from(needs_scrollbar));
                let rows = Rect { width, ..list_area };
                let end = (self.flag_scroll + list_height).min(self.flags.len());
                let mut lines: Vec<Line> = Vec::new();
                for index in self.flag_scroll..end {
                    let selected = index == self.flag_cursor;
                    let (marker, style) = match (self.delete_stage, selected) {
                        (DeleteStage::Confirm, true) => {
                            ("✗ ", Style::new().fg(theme::error()).bold())
                        }
                        (DeleteStage::Select, true) => {
                            ("› ", Style::new().fg(theme::error()).bold())
                        }
                        _ => ("  ", Style::new().fg(theme::text())),
                    };
                    // A value that belongs to the flag above reads as its argument.
                    let value = !self.flags[index].starts_with('-');
                    let indent = if value { "    " } else { "" };
                    lines.push(Line::from(vec![
                        Span::styled(marker, style),
                        Span::raw(indent),
                        Span::styled(
                            self.flags[index].clone(),
                            if value && !selected {
                                Style::new().fg(theme::muted())
                            } else {
                                style
                            },
                        ),
                    ]));
                }
                frame.render_widget(Paragraph::new(lines), rows);

                if needs_scrollbar {
                    let scrollbar_area = Rect {
                        x: list_area.right().saturating_sub(1),
                        width: 1,
                        ..list_area
                    };
                    self.flags_scrollbar_area = scrollbar_area;
                    let mut state = ScrollbarState::new(self.flags.len() - list_height)
                        .position(self.flag_scroll);
                    frame.render_stateful_widget(
                        scrollbar(ScrollbarOrientation::VerticalRight),
                        scrollbar_area,
                        &mut state,
                    );
                } else {
                    self.flags_scroll_drag = None;
                }
            }
        } else {
            self.flags_scroll_drag = None;
        }
        y = y.saturating_add(list_area.height);

        if y < body.bottom() {
            let typing = flags_active && !deleting;
            frame.render_widget(
                Paragraph::new(Span::styled(
                    format!("{} ", icons::PROMPT),
                    Style::new().fg(if typing {
                        theme::accent()
                    } else {
                        theme::faint()
                    }),
                )),
                Rect {
                    width: 2,
                    ..inner(y)
                },
            );
            let text_area = Rect {
                x: body.x + 4,
                width: body.width.saturating_sub(4),
                ..inner(y)
            };
            self.draft.set_style(Style::new().fg(theme::text()));
            self.draft.set_block(Block::default());
            self.draft.set_cursor_line_style(Style::new());
            self.draft
                .set_placeholder_text("add a flag, e.g. -O2 or -std=c++20");
            self.draft
                .set_placeholder_style(Style::new().fg(theme::faint()));
            self.draft.set_cursor_style(if typing {
                Style::new().add_modifier(Modifier::REVERSED)
            } else {
                Style::new()
            });
            frame.render_widget(&self.draft, text_area);
        }
        // A rejected draft says why right under it, where it cannot be
        // clipped off a short sheet.
        if let Some(error) = &self.error
            && y + 1 < body.bottom()
        {
            let row = Rect {
                x: body.x + 4,
                width: body.width.saturating_sub(4),
                ..inner(y + 1)
            };
            frame.render_widget(
                Paragraph::new(Line::from(vec![
                    Span::styled("✗ ", Style::new().fg(theme::error()).bold()),
                    Span::styled(
                        ui::truncate_revealing(
                            error,
                            body.width.saturating_sub(6) as usize,
                            row,
                            "Error",
                        ),
                        Style::new().fg(theme::error()),
                    ),
                ])),
                row,
            );
        }
        y += 2;

        // Theme, previewed live: the whole screen is already in it.
        if y < body.bottom() {
            let theme_active = self.active_field == THEME_FIELD;
            if theme_active {
                rail(frame, y, 1, theme::accent());
            }
            frame.render_widget(
                Paragraph::new(Span::styled(
                    format!("{:<16}", "theme"),
                    label(theme_active, theme::accent()),
                )),
                inner(y),
            );
            let mut value = if theme_active {
                vec![
                    Span::styled("‹ ", Style::new().fg(theme::faint())),
                    Span::styled(self.theme.label(), Style::new().fg(theme::text()).bold()),
                    Span::styled(" ›", Style::new().fg(theme::faint())),
                ]
            } else {
                vec![Span::styled(
                    self.theme.label(),
                    Style::new().fg(theme::text()),
                )]
            };
            // A swatch of the theme's own colours.
            value.push(Span::raw("   "));
            let palette = theme::p();
            for rgb in [
                palette.accent,
                palette.success,
                palette.error,
                palette.warning,
                palette.purple,
            ] {
                value.push(Span::styled("●", Style::new().fg(theme::paint(rgb))));
            }
            frame.render_widget(
                Paragraph::new(Line::from(value)),
                Rect {
                    x: body.x + 18,
                    width: body.width.saturating_sub(18),
                    ..inner(y)
                },
            );
        }
        y += 2;

        if y < body.bottom() {
            let hint = Line::from(vec![
                Span::styled(
                    "submits as Codeforces language ",
                    Style::new().fg(theme::faint()),
                ),
                Span::styled(
                    self.base.cf_id(&crate::core::Toolchain {
                        language: self.language,
                        flags: self.flags.clone(),
                    }),
                    Style::new().fg(theme::muted()),
                ),
            ]);
            frame.render_widget(Paragraph::new(hint), inner(y));
        }
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
                        self.manual_scroll = true;
                        self.set_flag_scroll(scroll);
                    }
                }
                MouseEventKind::Drag(MouseButton::Left) => {
                    if let Some(drag) = self.flags_scroll_drag {
                        let scroll = drag.scroll_at(position);
                        self.manual_scroll = true;
                        self.set_flag_scroll(scroll);
                    }
                }
                MouseEventKind::Up(MouseButton::Left) => self.flags_scroll_drag = None,
                MouseEventKind::ScrollDown => {
                    self.manual_scroll = true;
                    self.set_flag_scroll(self.flag_scroll.saturating_add(1));
                }
                MouseEventKind::ScrollUp => {
                    self.manual_scroll = true;
                    self.set_flag_scroll(self.flag_scroll.saturating_sub(1));
                }
                _ => {}
            }
            return true;
        }

        if self.flags_area.contains(position) {
            match mouse.kind {
                MouseEventKind::ScrollDown => {
                    self.manual_scroll = true;
                    self.set_flag_scroll(self.flag_scroll.saturating_add(1));
                }
                MouseEventKind::ScrollUp => {
                    self.manual_scroll = true;
                    self.set_flag_scroll(self.flag_scroll.saturating_sub(1));
                }
                _ => {}
            }
            return true;
        }
        false
    }

    /// Whether the flag list's scrollbar is being dragged.
    pub fn is_dragging(&self) -> bool {
        self.flags_scroll_drag.is_some()
    }

    fn set_flag_scroll(&mut self, scroll: usize) {
        let visible = self.flags_area.height as usize;
        let max_scroll = self.flags.len().saturating_sub(visible.max(1));
        self.flag_scroll = scroll.min(max_scroll);
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
                self.active_field = step_wrapped(self.active_field, FIELDS, backwards);
                self.error = None;
            }
            KeyCode::Left if self.active_field == 0 => {
                self.switch_language(self.language.prev());
            }
            KeyCode::Right if self.active_field == 0 => {
                self.switch_language(self.language.next());
            }
            KeyCode::Left if self.active_field == THEME_FIELD => {
                self.theme = self.theme.prev();
                theme::set(self.theme);
            }
            KeyCode::Right if self.active_field == THEME_FIELD => {
                self.theme = self.theme.next();
                theme::set(self.theme);
            }
            KeyCode::Char('x') if self.draft.is_empty() && !self.flags.is_empty() => {
                self.flag_cursor = self.flag_cursor.min(self.flags.len() - 1);
                self.manual_scroll = false;
                self.ensure_flag_visible_with(self.flag_viewport());
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
                if self.flags.iter().chain(&pending).any(|flag| flag == token) {
                    self.error = Some(format!("{token} is already in the list"));
                    return false;
                }
                if !previous.as_deref().is_some_and(takes_value) {
                    // g++ would read a stray value as another source file.
                    let after = previous.as_deref().unwrap_or("nothing");
                    self.error = Some(format!(
                        "{token} must come right after the flag it belongs to (not {after}) — add both at once, e.g. -include-pch <path>"
                    ));
                    return false;
                }
            } else {
                let lower = token.to_ascii_lowercase();
                if self.flags.iter().any(|flag| flag == token)
                    || pending.iter().any(|flag| flag == token)
                {
                    self.error = Some(format!("{token} is already in the list"));
                    return false;
                }
                if lower == "-std" || lower == "--std" {
                    self.error =
                        Some("write the standard as -std=c++20, without a space".to_string());
                    return false;
                }
                if (lower.starts_with("-std=") || lower.starts_with("--std="))
                    && std_from_flag(token).is_none()
                {
                    self.error = Some(format!("unknown standard — {token} (try -std=c++20)"));
                    return false;
                }
            }
            pending.push(token.to_string());
            previous = Some(token.to_string());
        }

        if !pending.is_empty() {
            self.flags.extend(pending);
            self.flag_cursor = self.flags.len() - 1;
            self.manual_scroll = false;
            self.ensure_flag_visible_with(self.flag_viewport());
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
            .draw(|frame| menu.draw(frame, frame.area(), 1.0))
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
            menu.active_field, THEME_FIELD,
            "a terminal that reports Shift+Tab as Tab must still go back"
        );
    }

    #[test]
    fn the_theme_field_cycles_and_is_saved() {
        let mut menu = menu_with(&[]);
        let opened = screen(&mut menu, 80, 30);
        assert!(opened.contains("theme"), "{opened}");
        assert!(opened.contains("prsweet"), "{opened}");

        menu.handle_key(key(KeyCode::Tab));
        menu.handle_key(key(KeyCode::Tab));
        assert_eq!(menu.active_field, THEME_FIELD);
        menu.handle_key(key(KeyCode::Right));
        assert_eq!(menu.theme, ThemeName::Catppuccin);
        assert_eq!(theme::active(), ThemeName::Catppuccin, "previewed live");
        menu.handle_key(key(KeyCode::Right));
        assert_eq!(menu.to_config().theme, ThemeName::Obsidian);
        menu.handle_key(key(KeyCode::Right));
        assert_eq!(menu.theme, ThemeName::Daylight);
        menu.handle_key(key(KeyCode::Right));
        assert_eq!(menu.theme, ThemeName::Prsweet);
        theme::set(ThemeName::Prsweet);
        menu.handle_key(key(KeyCode::Tab));
        assert_eq!(menu.active_field, 0, "tab wraps to language");
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
        menu.handle_key(key(KeyCode::BackTab));

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
        menu.handle_key(key(KeyCode::BackTab));

        menu.handle_key(key(KeyCode::Right));
        assert_eq!(menu.language, Language::Python);
        assert!(menu.flags.is_empty());

        menu.handle_key(key(KeyCode::Left));
        assert_eq!(menu.language, Language::Cpp);
        assert_eq!(menu.flags, ["-O2"]);
    }

    #[test]
    fn a_value_is_refused_after_the_wrong_flag_and_says_why() {
        let pch = "/usr/local/include/bits/stdc++.h.pch";
        let mut menu = menu_with(&["-include-pch", pch, "-O0"]);
        menu.handle_key(key(KeyCode::Tab));

        // The path is already there, after -include-pch.
        type_text(&mut menu, pch);
        menu.handle_key(key(KeyCode::Enter));
        assert_eq!(
            menu.error.as_deref(),
            Some(format!("{pch} is already in the list").as_str())
        );

        // A new value after -O0 would reach g++ as a stray source file.
        menu.draft = TextArea::default();
        type_text(&mut menu, "/tmp/other.pch");
        menu.handle_key(key(KeyCode::Enter));
        let error = menu.error.clone().unwrap_or_default();
        assert!(error.contains("not -O0"), "{error}");

        // The error shows right under the input, even on a short sheet.
        let shown = screen(&mut menu, 80, 22);
        assert!(shown.contains("must come right after"), "{shown}");
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
                .contains("must come right after the flag")
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
                .contains("unknown standard")
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
                .contains("without a space")
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

        // The list auto-scrolls to the selected (newest) flag: six rows are
        // visible at once, the rest stay reachable with the scrollbar.
        let initial = screen(&mut menu, 100, 30);
        assert!(initial.contains("-DOPT9"));
        assert!(!initial.contains("-DOPT0"));
        assert_eq!(menu.flags_area.height as usize, FLAG_ROWS);

        menu.handle_key(key(KeyCode::Char('x')));
        let deleting = screen(&mut menu, 100, 30);
        assert!(deleting.contains("-DOPT9"));
        assert!(deleting.contains('›'));

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
        assert_eq!(menu.flag_scroll, 1, "the wheel scrolls the list");
        assert_eq!(menu.flag_cursor, 0, "the wheel keeps the selection put");
        menu.handle_mouse(&mouse(MouseEventKind::ScrollUp, menu.flags_area.x + 1, row));
        assert_eq!(menu.flag_scroll, 0);
    }

    #[test]
    fn flags_panel_keeps_one_fixed_height() {
        let mut few = menu_with(&["-O2"]);
        let _ = screen(&mut few, 100, 30);
        let flags: Vec<String> = (0..12).map(|i| format!("-DOPT{i}")).collect();
        let mut many = ConfigMenu::new(&UserConfig {
            language: Language::Cpp,
            extra_flags: flags,
            ..Default::default()
        });
        let _ = screen(&mut many, 100, 30);

        assert_eq!(few.flags_area.height as usize, FLAG_ROWS);
        assert_eq!(many.flags_area.height as usize, FLAG_ROWS);
        assert_eq!(
            few.flags_area.y, many.flags_area.y,
            "adding flags must not resize or move the sheet"
        );

        // Selection stops at the last tag instead of wrapping to the top.
        many.handle_key(key(KeyCode::Char('x')));
        for _ in 0..20 {
            many.handle_key(key(KeyCode::Tab));
        }
        assert_eq!(many.flag_cursor, many.flags.len() - 1);
        for _ in 0..20 {
            many.handle_key(key(KeyCode::BackTab));
        }
        assert_eq!(many.flag_cursor, 0);
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
