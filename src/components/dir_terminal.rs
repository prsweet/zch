use std::{fs, path::PathBuf};

use ratatui::{
    Frame,
    crossterm::event::{KeyCode, KeyEvent},
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Padding, Paragraph},
};
use ratatui_textarea::TextArea;
use unicode_width::UnicodeWidthStr;

use crate::components::{Message, icons, theme, ui};

const HISTORY_LIMIT: usize = 200;

/// Messages may already carry a verdict glyph. Reuse it rather than stacking a
/// second marker in front of the text.
const STATUS_GLYPHS: [char; 4] = ['✔', '✘', '⚠', '⏱'];

fn take_status_glyph(text: &str) -> Option<(char, &str)> {
    let mut chars = text.chars();
    let glyph = chars.next()?;
    if !STATUS_GLYPHS.contains(&glyph) {
        return None;
    }
    let rest = chars.as_str();
    Some((glyph, rest.strip_prefix(' ').unwrap_or(rest)))
}

fn status_glyph(color: Color) -> char {
    if color == theme::RED() {
        '✘'
    } else if color == theme::GREEN() {
        '✔'
    } else if color == theme::YELLOW() {
        '!'
    } else if color == theme::CYAN() {
        '●'
    } else {
        '·'
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MessageLane {
    Hint,
    Live,
    Result,
    Error,
}

#[derive(Default)]
struct MessageLanes {
    hint: Option<Message>,
    live: Option<Message>,
    result: Option<Message>,
    error: Option<Message>,
}

impl MessageLanes {
    fn set(&mut self, lane: MessageLane, message: Message) -> bool {
        let slot = match lane {
            MessageLane::Hint => &mut self.hint,
            MessageLane::Live => &mut self.live,
            MessageLane::Result => &mut self.result,
            MessageLane::Error => &mut self.error,
        };
        if slot
            .as_ref()
            .is_some_and(|current| current.text == message.text && current.color == message.color)
        {
            return false;
        }
        *slot = Some(message);

        match lane {
            MessageLane::Result => {
                self.hint = None;
                self.error = None;
            }
            MessageLane::Error => self.hint = None,
            MessageLane::Live => self.hint = None,
            MessageLane::Hint => {}
        }
        true
    }

    fn clear(&mut self, lane: MessageLane) -> bool {
        let slot = match lane {
            MessageLane::Hint => &mut self.hint,
            MessageLane::Live => &mut self.live,
            MessageLane::Result => &mut self.result,
            MessageLane::Error => &mut self.error,
        };
        slot.take().is_some()
    }

    fn is_empty(&self) -> bool {
        self.hint.is_none() && self.live.is_none() && self.result.is_none() && self.error.is_none()
    }

    fn active(&self) -> Option<&Message> {
        self.error
            .as_ref()
            .or(self.result.as_ref())
            .or(self.live.as_ref())
            .or(self.hint.as_ref())
    }
}

struct Completion {
    dir_part: String,
    matches: Vec<String>,
    next: usize,
}

pub struct DirTerminal {
    pub cur_dir: PathBuf,
    pub input_area: TextArea<'static>,
    pub output: Option<Message>,
    lanes: MessageLanes,
    history: Vec<String>,
    history_cursor: Option<usize>,
    completion: Option<Completion>,
}

impl DirTerminal {
    pub fn new(initial_dir: PathBuf) -> Self {
        Self {
            cur_dir: initial_dir,
            input_area: TextArea::default(),
            output: None,
            lanes: MessageLanes::default(),
            history: Vec::new(),
            history_cursor: None,
            completion: None,
        }
    }

    pub fn set_message(&mut self, color: Color, text: impl Into<String>) {
        let lane = if color == theme::MUTED() {
            MessageLane::Hint
        } else if color == theme::RED() {
            MessageLane::Error
        } else {
            MessageLane::Result
        };
        self.set_lane(lane, color, text);
    }

    pub fn set_live(&mut self, color: Color, text: impl Into<String>) {
        self.set_lane(MessageLane::Live, color, text);
    }

    pub fn set_result(&mut self, color: Color, text: impl Into<String>) {
        self.set_lane(MessageLane::Result, color, text);
    }

    pub fn set_hint(&mut self, color: Color, text: impl Into<String>) {
        self.set_lane(MessageLane::Hint, color, text);
    }

    pub fn set_error(&mut self, color: Color, text: impl Into<String>) {
        self.set_lane(MessageLane::Error, color, text);
    }

    pub fn clear_hint(&mut self) {
        if self.lanes.clear(MessageLane::Hint) {
            self.output = self.lanes.active().cloned();
        }
    }

    pub fn clear_live(&mut self) {
        if self.lanes.clear(MessageLane::Live) {
            self.output = self.lanes.active().cloned();
        }
    }

    pub fn clear_result(&mut self) {
        if self.lanes.clear(MessageLane::Result) {
            self.output = self.lanes.active().cloned();
        }
    }

    pub fn clear_error(&mut self) {
        if self.lanes.clear(MessageLane::Error) {
            self.output = self.lanes.active().cloned();
        }
    }

    fn set_lane(&mut self, lane: MessageLane, color: Color, text: impl Into<String>) {
        if self.lanes.set(lane, Message::new(color, text)) {
            self.output = self.lanes.active().cloned();
        }
    }

    /// Height of the working-directory / command bar. One compact row, so the
    /// code and test output keep the vertical space.
    pub fn input_height(&self) -> u16 {
        1
    }

    /// The console is a single fixed status line, so incoming messages never
    /// resize the rest of the layout.
    pub fn console_height(&self, _width: u16) -> u16 {
        if self.lanes.is_empty() { 0 } else { 1 }
    }

    pub fn draw_input(&mut self, frame: &mut Frame, area: Rect, is_active: bool) {
        let dir_area = Rect {
            height: area.height.min(self.input_height()),
            ..area
        };
        if dir_area.height == 0 {
            return;
        }

        if is_active {
            ui::fill(frame, dir_area, theme::SURFACE_ALT());
            let prompt_area = Rect {
                x: dir_area.x + ui::GUTTER,
                width: 2,
                height: 1,
                ..dir_area
            };
            frame.render_widget(
                Paragraph::new(Span::styled(
                    format!("{} ", icons::POINTER),
                    Style::new().fg(theme::ACCENT()).bold(),
                ))
                .style(Style::new().bg(theme::SURFACE_ALT())),
                prompt_area,
            );

            // Keep the current directory visible while the command is typed.
            let path = self.cur_dir.display().to_string();
            let path_width = (path.width() + 2).min((dir_area.width / 2) as usize).max(1) as u16;
            let path_area = Rect {
                x: dir_area.x + dir_area.width - path_width - ui::GUTTER,
                width: path_width,
                height: 1,
                ..dir_area
            };
            frame.render_widget(
                Paragraph::new(Span::styled(
                    ui::fit_path(&path, path_width as usize),
                    Style::new().fg(theme::FAINT()),
                ))
                .alignment(ratatui::layout::Alignment::Right)
                .style(Style::new().bg(theme::SURFACE_ALT())),
                path_area,
            );

            let input_x = prompt_area.x + 2;
            let input_width = path_area.x.saturating_sub(input_x + 1);
            let input_area = Rect {
                x: input_x,
                width: input_width,
                height: 1,
                ..dir_area
            };
            self.input_area.set_block(
                Block::default()
                    .padding(Padding::new(0, 0, 0, 0))
                    .style(Style::new().bg(theme::SURFACE_ALT())),
            );
            self.input_area
                .set_style(Style::new().fg(theme::TEXT()).bg(theme::SURFACE_ALT()));
            self.input_area
                .set_cursor_style(Style::new().add_modifier(ratatui::style::Modifier::REVERSED));
            if input_area.width > 0 {
                frame.render_widget(&self.input_area, input_area);
            }
        } else {
            let bar = Rect {
                x: dir_area.x + ui::GUTTER,
                width: dir_area.width.saturating_sub(ui::GUTTER * 2),
                height: 1,
                ..dir_area
            };
            let path = self.cur_dir.display().to_string();
            let available = bar.width as usize;
            let line = Line::from(vec![
                Span::styled(
                    format!("{}  ", icons::FOLDER),
                    Style::new().fg(theme::ACCENT()),
                ),
                Span::styled(
                    ui::fit_path(&path, available.saturating_sub(3)),
                    Style::new().fg(theme::MUTED()),
                ),
            ]);
            frame.render_widget(
                Paragraph::new(line).style(Style::new().bg(theme::BG())),
                bar,
            );
        }
    }

    pub fn draw_console(&self, frame: &mut Frame, area: Rect) {
        if area.height == 0 || self.lanes.is_empty() {
            return;
        }

        let Some(message) = self.lanes.active() else {
            return;
        };
        let row = Rect {
            x: area.x + ui::GUTTER,
            width: area.width.saturating_sub(ui::GUTTER * 2),
            height: 1,
            ..area
        };
        let text = message.text.lines().collect::<Vec<_>>().join(" ");
        let (glyph, rest) = match take_status_glyph(&text) {
            Some((glyph, rest)) => (glyph, rest),
            None => (status_glyph(message.color), text.as_str()),
        };
        let width = row.width.saturating_sub(2) as usize;
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(format!("{glyph} "), Style::new().fg(message.color).bold()),
                Span::styled(ui::truncate(rest, width), Style::new().fg(message.color)),
            ]))
            .style(Style::new().bg(theme::BG())),
            row,
        );
    }

    pub fn handle_event(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Enter => {
                self.execute();
                self.history_cursor = None;
                self.completion = None;
            }
            KeyCode::Tab => self.autocomplete(),
            KeyCode::Up => self.history_up(),
            KeyCode::Down => self.history_down(),
            KeyCode::Esc => {
                self.clear_hint();
                return true;
            }
            _ => {
                self.input_area.input(key);
                self.history_cursor = None;
                self.completion = None;
            }
        }
        false
    }

    fn execute(&mut self) {
        let cmd = self.input_area.lines().join(" ").trim().to_string();
        self.input_area = TextArea::default();

        if cmd.is_empty() {
            return;
        }

        if self.history.last() != Some(&cmd) {
            self.history.push(cmd.clone());
            if self.history.len() > HISTORY_LIMIT {
                self.history.remove(0);
            }
        }

        if cmd == "cd" || cmd.starts_with("cd ") {
            let target = cmd[2..].trim();
            let new_path = if target.is_empty() {
                self.cur_dir.clone()
            } else {
                self.cur_dir.join(target)
            };

            match new_path.canonicalize() {
                Ok(canonical) if canonical.is_dir() => {
                    self.cur_dir = canonical;
                    self.clear_hint();
                    self.clear_error();
                }
                Ok(_) => self.set_message(theme::RED(), format!("cd: not a directory: {target}")),
                Err(_) => {
                    self.set_message(theme::RED(), format!("cd: no such directory: {target}"))
                }
            }
        } else if cmd == "ls" {
            match fs::read_dir(&self.cur_dir) {
                Ok(entries) => {
                    let mut names: Vec<String> = entries
                        .flatten()
                        .map(|entry| {
                            let name = entry.file_name().to_string_lossy().to_string();
                            if entry.path().is_dir() {
                                format!("{} {name}/", icons::FOLDER)
                            } else {
                                format!("{} {name}", icons::FILE)
                            }
                        })
                        .collect();
                    names.sort();
                    self.set_message(theme::CYAN(), names.join("   "));
                }
                Err(e) => self.set_message(theme::RED(), format!("ls: {e}")),
            }
        } else if cmd == "pwd" {
            self.set_message(theme::CYAN(), self.cur_dir.display().to_string());
        } else {
            self.set_message(
                theme::YELLOW(),
                format!("unknown command: {cmd} (try: cd <path>, ls, pwd)"),
            );
        }
    }

    fn autocomplete(&mut self) {
        // Repeated Tab presses cycle through every match after the first press
        // completed the common prefix.
        if let Some(completion) = self.completion.take() {
            let name = completion.matches[completion.next].clone();
            let next = (completion.next + 1) % completion.matches.len();
            self.set_input(&format!("cd {}{name}/", completion.dir_part));
            self.show_matches(&completion.matches, &completion.dir_part);
            self.completion = Some(Completion { next, ..completion });
            return;
        }

        let line = self.input_area.lines().join("");

        let Some(prefix) = line.strip_prefix("cd ") else {
            return;
        };

        // Split the typed path into the directory to search and the fragment
        // being completed: "cp/qu" → ("cp/", "qu"); "quis" → ("", "quis").
        let (dir_part, fragment) = match prefix.rfind('/') {
            Some(index) => (&prefix[..=index], &prefix[index + 1..]),
            None => ("", prefix),
        };

        let search_dir = if dir_part.is_empty() {
            self.cur_dir.clone()
        } else if let Some(rest) = dir_part.strip_prefix("~/") {
            match std::env::var_os("HOME") {
                Some(home) => PathBuf::from(home).join(rest),
                None => return,
            }
        } else {
            let path = PathBuf::from(dir_part);
            if path.is_absolute() {
                path
            } else {
                self.cur_dir.join(dir_part)
            }
        };

        let Ok(entries) = fs::read_dir(&search_dir) else {
            return;
        };

        let needle = fragment.to_lowercase();
        let mut matches: Vec<String> = entries
            .flatten()
            .filter(|entry| entry.path().is_dir())
            .map(|entry| entry.file_name().to_string_lossy().to_string())
            .filter(|name| name.to_lowercase().starts_with(&needle))
            .collect();
        matches.sort();

        if matches.is_empty() {
            return;
        }

        let extension = common_extension(&matches);
        self.set_input(&format!("cd {dir_part}{extension}"));
        self.history_cursor = None;

        if matches.len() == 1 {
            self.input_area.insert_str("/");
            self.clear_hint();
        } else {
            self.show_matches(&matches, dir_part);
            self.completion = Some(Completion {
                dir_part: dir_part.to_string(),
                matches,
                next: 0,
            });
        }
    }

    fn show_matches(&mut self, matches: &[String], dir_part: &str) {
        let list = matches
            .iter()
            .map(|name| format!("{} {dir_part}{name}/", icons::FOLDER))
            .collect::<Vec<_>>()
            .join("   ");
        self.set_message(theme::CYAN(), list);
    }

    fn set_input(&mut self, text: &str) {
        self.input_area = TextArea::default();
        if !text.is_empty() {
            self.input_area.insert_str(text);
        }
    }

    fn history_up(&mut self) {
        if self.history.is_empty() {
            return;
        }
        let index = match self.history_cursor {
            Some(0) => 0,
            Some(index) => index - 1,
            None => self.history.len() - 1,
        };
        self.history_cursor = Some(index);
        self.set_input(&self.history[index].clone());
        self.completion = None;
    }

    fn history_down(&mut self) {
        let Some(index) = self.history_cursor else {
            return;
        };
        if index + 1 < self.history.len() {
            self.history_cursor = Some(index + 1);
            self.set_input(&self.history[index + 1].clone());
        } else {
            self.history_cursor = None;
            self.set_input("");
        }
        self.completion = None;
    }
}

/// The longest prefix shared by every match (case-insensitively), rendered
/// with the casing of the first match: ["Questions", "Quiz"] → "Qu".
fn common_extension(matches: &[String]) -> String {
    let lowered: Vec<String> = matches.iter().map(|name| name.to_lowercase()).collect();
    let mut shared = lowered[0].chars().count();
    for name in &lowered[1..] {
        shared = shared.min(
            lowered[0]
                .chars()
                .zip(name.chars())
                .take_while(|(left, right)| left == right)
                .count(),
        );
    }
    matches[0].chars().take(shared).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{
        Terminal,
        backend::TestBackend,
        crossterm::event::{KeyEvent, KeyModifiers},
    };

    fn tab() -> KeyEvent {
        KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn temp_dir(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("zch_complete_{}_{name}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn typed(terminal: &DirTerminal) -> String {
        terminal.input_area.lines().join("")
    }

    #[test]
    fn message_lanes_keep_errors_visible_and_results_authoritative() {
        let mut dir = DirTerminal::new(PathBuf::from("/tmp"));

        dir.set_live(theme::CYAN(), "running case 1/2");
        dir.set_error(theme::RED(), "network failed");
        assert_eq!(dir.output.as_ref().unwrap().text, "network failed");
        assert_eq!(dir.lanes.live.as_ref().unwrap().text, "running case 1/2");
        assert_eq!(dir.lanes.error.as_ref().unwrap().text, "network failed");

        dir.set_result(theme::GREEN(), "all passed");
        assert_eq!(dir.lanes.live.as_ref().unwrap().text, "running case 1/2");
        assert!(
            dir.lanes.error.is_none(),
            "a final result supersedes an error"
        );
        assert_eq!(dir.output.as_ref().unwrap().text, "all passed");
        dir.clear_live();
        assert!(
            dir.lanes.live.is_none(),
            "the run lifecycle ends the live phase"
        );

        dir.set_error(theme::RED(), "submit failed");
        dir.set_live(theme::CYAN(), "running case 2/2");
        assert_eq!(
            dir.output.as_ref().unwrap().text,
            "submit failed",
            "a live phase must not erase an error"
        );
    }

    #[test]
    fn console_renders_the_active_message_on_one_line() {
        let mut dir = DirTerminal::new(PathBuf::from("/tmp"));
        dir.set_live(theme::CYAN(), "running case 2/3");

        let mut terminal = Terminal::new(TestBackend::new(40, 3)).unwrap();
        terminal
            .draw(|frame| dir.draw_console(frame, Rect::new(0, 0, 40, 3)))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let row = |y| (0..40).map(|x| buffer[(x, y)].symbol()).collect::<String>();
        assert!(row(0).contains("running case 2/3"));

        dir.set_result(theme::GREEN(), "2/3 passed");
        terminal
            .draw(|frame| dir.draw_console(frame, Rect::new(0, 0, 40, 3)))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let row = |y| (0..40).map(|x| buffer[(x, y)].symbol()).collect::<String>();
        assert!(row(0).contains("2/3 passed"));
        assert!(!row(0).contains("running case 2/3"));
    }

    #[test]
    fn console_does_not_stack_glyphs_on_verdict_messages() {
        let mut dir = DirTerminal::new(PathBuf::from("/tmp"));
        dir.set_live(theme::CYAN(), "plain status");

        let mut terminal = Terminal::new(TestBackend::new(40, 3)).unwrap();
        terminal
            .draw(|frame| dir.draw_console(frame, Rect::new(0, 0, 40, 3)))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let row = |y| (0..40).map(|x| buffer[(x, y)].symbol()).collect::<String>();
        assert_eq!(
            row(0).trim(),
            "● plain status",
            "a plain message still gets a status glyph"
        );

        dir.set_result(theme::GREEN(), "✔ ALL 2/2 PASSED");
        dir.clear_live();
        terminal
            .draw(|frame| dir.draw_console(frame, Rect::new(0, 0, 40, 3)))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let row = |y| (0..40).map(|x| buffer[(x, y)].symbol()).collect::<String>();
        assert_eq!(
            row(0).trim(),
            "✔ ALL 2/2 PASSED",
            "the verdict's own glyph is not duplicated"
        );
    }

    #[test]
    fn console_height_is_one_line_and_stable_for_wrapped_messages() {
        let mut dir = DirTerminal::new(PathBuf::from("/tmp"));
        assert_eq!(dir.console_height(120), 0, "empty consoles take no space");

        dir.set_result(theme::TEXT(), "short");
        assert_eq!(dir.console_height(120), 1);
        assert_eq!(dir.console_height(10), 1);

        dir.set_result(theme::TEXT(), "long ".repeat(500));
        assert_eq!(
            dir.console_height(120),
            1,
            "wrapping must not resize the layout"
        );
        assert_eq!(dir.console_height(10), 1);
    }

    #[test]
    fn completes_nested_directories_level_by_level() {
        let root = temp_dir("nested");
        fs::create_dir_all(root.join("cp/questions")).unwrap();
        let mut dir = DirTerminal::new(root.clone());

        dir.input_area.insert_str("cd c");
        dir.handle_event(tab());
        assert_eq!(typed(&dir), "cd cp/");

        dir.input_area.insert_str("q");
        dir.handle_event(tab());
        assert_eq!(typed(&dir), "cd cp/questions/");

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn completes_common_prefix_and_lists_ambiguous_matches() {
        let root = temp_dir("ambiguous");
        fs::create_dir_all(root.join("cp/questions")).unwrap();
        fs::create_dir_all(root.join("cp/quiz")).unwrap();
        let mut dir = DirTerminal::new(root.clone());

        dir.input_area.insert_str("cd cp/q");
        dir.handle_event(tab());
        assert_eq!(typed(&dir), "cd cp/qu");

        let listing = &dir.output.as_ref().unwrap().text;
        assert!(listing.contains("questions"));
        assert!(listing.contains("quiz"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn completes_from_home_prefix() {
        let root = temp_dir("home");
        fs::create_dir_all(root.join("projects")).unwrap();
        let mut dir = DirTerminal::new(root.clone());

        dir.input_area
            .insert_str(format!("cd {}/pro", root.display()));
        dir.handle_event(tab());
        assert_eq!(typed(&dir), format!("cd {}/projects/", root.display()));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn tab_cycles_through_ambiguous_matches() {
        let root = temp_dir("cycle");
        fs::create_dir_all(root.join("cp/questions")).unwrap();
        fs::create_dir_all(root.join("cp/quiz")).unwrap();
        let mut dir = DirTerminal::new(root.clone());

        dir.input_area.insert_str("cd cp/q");
        dir.handle_event(tab());
        assert_eq!(typed(&dir), "cd cp/qu");

        dir.handle_event(tab());
        assert_eq!(typed(&dir), "cd cp/questions/");
        dir.handle_event(tab());
        assert_eq!(typed(&dir), "cd cp/quiz/");
        dir.handle_event(tab());
        assert_eq!(typed(&dir), "cd cp/questions/", "cycling wraps around");

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn up_and_down_recall_command_history() {
        let root = temp_dir("history");
        let mut dir = DirTerminal::new(root.clone());

        dir.input_area.insert_str("ls");
        dir.handle_event(key(KeyCode::Enter));
        dir.input_area.insert_str("pwd");
        dir.handle_event(key(KeyCode::Enter));

        dir.handle_event(key(KeyCode::Up));
        assert_eq!(typed(&dir), "pwd");
        dir.handle_event(key(KeyCode::Up));
        assert_eq!(typed(&dir), "ls");
        dir.handle_event(key(KeyCode::Up));
        assert_eq!(typed(&dir), "ls", "oldest entry stays put");

        dir.handle_event(key(KeyCode::Down));
        assert_eq!(typed(&dir), "pwd");
        dir.handle_event(key(KeyCode::Down));
        assert_eq!(typed(&dir), "", "past the newest entry clears the box");

        // Typing after recalling leaves history traversal.
        dir.handle_event(key(KeyCode::Up));
        dir.handle_event(key(KeyCode::Char('x')));
        assert_eq!(typed(&dir), "pwdx");

        let _ = fs::remove_dir_all(&root);
    }
}
