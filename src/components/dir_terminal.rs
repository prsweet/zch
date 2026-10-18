use std::{fs, path::PathBuf};

use ratatui::{
    Frame,
    crossterm::event::{KeyCode, KeyEvent},
    layout::{HorizontalAlignment::Right, Rect},
    style::{Color, Style},
    widgets::{Paragraph, Wrap},
};
use ratatui_textarea::TextArea;

use crate::components::{Message, block, icons, theme};

const MSG_MAX_HEIGHT: u16 = 10;

pub struct DirTerminal {
    pub cur_dir: PathBuf,
    pub input_area: TextArea<'static>,
    pub output: Option<Message>,
}

impl DirTerminal {
    pub fn new(initial_dir: PathBuf) -> Self {
        Self {
            cur_dir: initial_dir,
            input_area: TextArea::default(),
            output: None,
        }
    }

    pub fn set_message(&mut self, color: Color, text: impl Into<String>) {
        self.output = Some(Message::new(color, text));
    }

    /// Height of the working-directory / command bar.
    pub fn input_height(&self) -> u16 {
        3
    }

    /// Height of the console panel rendered at the bottom of the screen.
    pub fn console_height(&self, width: u16) -> u16 {
        let Some(msg) = &self.output else { return 0 };

        let inner_width = width.saturating_sub(4).max(1) as usize;
        let mut rows: usize = 0;
        for line in msg.text.lines() {
            rows += line.chars().count().max(1).div_ceil(inner_width);
        }
        rows = rows.clamp(1, MSG_MAX_HEIGHT as usize);
        rows as u16 + 2
    }

    pub fn draw_input(&mut self, frame: &mut Frame, area: Rect, is_active: bool) {
        let dir_area = Rect { height: area.height.min(self.input_height()), ..area };
        let dir_block = block(None)
            .border_style(if is_active { theme::ACCENT } else { theme::BORDER })
            .title_style(Style::new().fg(theme::CYAN).bold())
            .title_alignment(Right);

        if is_active {
            let prompt = format!(" {} > ", self.cur_dir.display());
            self.input_area.set_block(dir_block.title(prompt));
            frame.render_widget(&self.input_area, dir_area);
        } else {
            let dir_panel = Paragraph::new(format!("  {} ", self.cur_dir.display()))
                .style(Style::new().fg(theme::TEXT))
                .block(dir_block.title(" Working Directory "))
                .wrap(Wrap { trim: true });
            frame.render_widget(dir_panel, dir_area);
        }
    }

    pub fn draw_console(&self, frame: &mut Frame, area: Rect) {
        if area.height < 2 {
            return;
        }
        let Some(msg) = &self.output else { return };

        let msg_panel = Paragraph::new(msg.text.clone())
            .style(msg.color)
            .block(
                block(Some(" Console "))
                    .border_style(theme::BORDER)
                    .title_style(Style::new().fg(theme::PURPLE).bold()),
            )
            .wrap(Wrap { trim: false });
        frame.render_widget(msg_panel, area);
    }

    pub fn handle_event(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Enter => self.execute(),
            KeyCode::Tab => self.autocomplete(),
            KeyCode::Esc => {
                self.output = None;
                return true;
            }
            _ => {
                self.input_area.input(key);
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

        if cmd == "cd" || cmd.starts_with("cd ") {
            let target = cmd[2..].trim();
            let new_path = if target.is_empty() { self.cur_dir.clone() } else { self.cur_dir.join(target) };

            match new_path.canonicalize() {
                Ok(canonical) if canonical.is_dir() => {
                    self.cur_dir = canonical;
                    self.output = None;
                }
                Ok(_) => self.set_message(theme::RED, format!("cd: not a directory: {target}")),
                Err(_) => self.set_message(theme::RED, format!("cd: no such directory: {target}")),
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
                    self.set_message(theme::CYAN, names.join("   "));
                }
                Err(e) => self.set_message(theme::RED, format!("ls: {e}")),
            }
        } else if cmd == "pwd" {
            self.set_message(theme::CYAN, self.cur_dir.display().to_string());
        } else {
            self.set_message(theme::YELLOW, format!("unknown command: {cmd} (try: cd <path>, ls, pwd)"));
        }
    }

    fn autocomplete(&mut self) {
        let line = self.input_area.lines().join("");

        let Some(prefix) = line.strip_prefix("cd ") else { return };

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
            if path.is_absolute() { path } else { self.cur_dir.join(dir_part) }
        };

        let Ok(entries) = fs::read_dir(&search_dir) else { return };

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
        self.input_area = TextArea::default();
        self.input_area.insert_str(format!("cd {dir_part}{extension}"));

        if matches.len() == 1 {
            self.input_area.insert_str("/");
            self.output = None;
        } else {
            let list = matches
                .iter()
                .map(|name| format!("{} {name}/", icons::FOLDER))
                .collect::<Vec<_>>()
                .join("   ");
            self.set_message(theme::CYAN, list);
        }
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
    use ratatui::crossterm::event::{KeyEvent, KeyModifiers};

    fn tab() -> KeyEvent {
        KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)
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

        dir.input_area.insert_str(format!("cd {}/pro", root.display()));
        dir.handle_event(tab());
        assert_eq!(typed(&dir), format!("cd {}/projects/", root.display()));

        let _ = fs::remove_dir_all(&root);
    }
}
