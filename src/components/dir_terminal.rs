//! The working-directory console (`d`): `cd`, `ls`, `pwd`, `mkdir`, `rm`.
//!
//! It lives in the header, under the app bar: the folder breadcrumb turns
//! into a command line in place, and command output (`ls`, completions,
//! errors) takes the row beneath it — never the run console, so a directory
//! listing can't be mistaken for a verdict. `mkdir` and `rm` are the
//! exception: they change the disk, so their result is a notice for the
//! console, like "config saved".

use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use ratatui::{
    Frame,
    crossterm::event::{KeyCode, KeyEvent},
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};
use ratatui_textarea::TextArea;
use unicode_width::UnicodeWidthStr;

use crate::components::{icons, motion, status::Tone, theme, ui};

const HISTORY_LIMIT: usize = 200;
/// How long the path acknowledges a directory change.
pub const PATH_GLOW: Duration = Duration::from_millis(1200);

/// What the last directory command printed.
#[derive(Debug, Clone)]
pub struct DirOutput {
    pub tone: Tone,
    /// One-line message (errors, `pwd`).
    pub text: String,
    /// Directory entries (`ls`, completion matches): `(is_dir, name)`.
    pub entries: Vec<(bool, String)>,
}

struct Completion {
    /// Everything before the path being completed: `cd `, `rm -r a.cpp `.
    head: String,
    dir_part: String,
    /// `(is_dir, name)`.
    matches: Vec<(bool, String)>,
    next: usize,
}

/// An `rm` of a folder, waiting for `Enter` before anything is deleted.
struct PendingRemoval {
    targets: Vec<RemovalTarget>,
    /// What the console reports once they are gone.
    done: String,
}

/// One path an `rm` is waiting to delete, as the confirmation shows it.
pub struct RemovalTarget {
    path: PathBuf,
    /// As typed, without a trailing `/`.
    pub name: String,
    pub is_dir: bool,
    /// Files and folders inside, up to [`COUNT_LIMIT`].
    pub inside: usize,
}

/// The commands the console runs, so the command word can be coloured as
/// it is typed.
const COMMANDS: [&str; 5] = ["cd", "ls", "pwd", "mkdir", "rm"];
const PLACEHOLDER: &str = "cd <path>   ls   pwd   mkdir <name>   rm [-r] <path>";

pub struct DirTerminal {
    pub cur_dir: PathBuf,
    pub input_area: TextArea<'static>,
    pub output: Option<DirOutput>,
    /// When the directory last changed, so the header can acknowledge it.
    pub changed_at: Option<Instant>,
    /// A result for the console, collected with [`DirTerminal::take_notice`].
    notice: Option<(Tone, String)>,
    removal: Option<PendingRemoval>,
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
            changed_at: None,
            notice: None,
            removal: None,
            history: Vec::new(),
            history_cursor: None,
            completion: None,
        }
    }

    fn say(&mut self, tone: Tone, text: impl Into<String>) {
        self.output = Some(DirOutput {
            tone,
            text: text.into(),
            entries: Vec::new(),
        });
    }

    fn list(&mut self, entries: Vec<(bool, String)>) {
        self.output = Some(DirOutput {
            tone: Tone::Info,
            text: String::new(),
            entries,
        });
    }

    fn notify(&mut self, tone: Tone, text: impl Into<String>) {
        self.notice = Some((tone, text.into()));
    }

    /// The last command's console notice, if it left one.
    pub fn take_notice(&mut self) -> Option<(Tone, String)> {
        self.notice.take()
    }

    pub fn clear_output(&mut self) {
        self.output = None;
    }

    /// Rows the path bar takes: the path, plus one while a command has output.
    pub fn input_height(&self) -> u16 {
        1 + u16::from(self.output.is_some())
    }

    /// The path row of the header. Idle, it is a breadcrumb of the folder
    /// new problems are saved to (`~ › cp › round912`, the current folder
    /// brightest); with `d` it becomes a command line in place, the
    /// breadcrumb pinned to the right. Output takes the row beneath.
    pub fn draw_input(&mut self, frame: &mut Frame, area: Rect, is_active: bool, now: Instant) {
        if area.height == 0 {
            return;
        }
        let row = Rect {
            x: area.x + ui::GUTTER,
            width: area.width.saturating_sub(ui::GUTTER * 2),
            height: 1,
            ..area
        };
        // A directory change is acknowledged by the folder glowing and settling.
        let glow = self
            .changed_at
            .map_or(1.0, |at| motion::ease_out(at, PATH_GLOW, now));

        if is_active {
            let crumbs = breadcrumb(&self.cur_dir, (row.width / 3) as usize, glow);
            let crumbs_width = crumbs.width() as u16;
            frame.render_widget(
                Paragraph::new(crumbs).alignment(ratatui::layout::Alignment::Right),
                Rect {
                    x: row.right().saturating_sub(crumbs_width),
                    width: crumbs_width,
                    ..row
                },
            );
            frame.render_widget(
                Paragraph::new(Span::styled(
                    format!("{} ", icons::PROMPT),
                    Style::new().fg(theme::accent()).bold(),
                )),
                Rect { width: 2, ..row },
            );
            let input = Rect {
                x: row.x + 2,
                width: row.width.saturating_sub(crumbs_width + 4),
                ..row
            };
            if input.width > 0 {
                frame.render_widget(
                    Paragraph::new(self.command_line(input.width as usize)),
                    input,
                );
            }
        } else {
            frame.render_widget(
                Paragraph::new(breadcrumb(&self.cur_dir, row.width as usize, glow)),
                row,
            );
        }

        if area.height > 1
            && let Some(output) = &self.output
        {
            let output_row = Rect {
                y: area.y + 1,
                ..row
            };
            frame.render_widget(
                Paragraph::new(output_line(output, output_row, now)),
                output_row,
            );
        }
    }

    /// The typed command, coloured like fish: the command word in the
    /// theme's command colour when the console knows it and its error red
    /// when it does not, the rest in the reading
    /// colour, the cursor a reversed cell. A line wider than the box scrolls
    /// to keep the cursor in view.
    fn command_line(&self, width: usize) -> Line<'static> {
        let cursor_style = Style::new().add_modifier(Modifier::REVERSED);
        let text: Vec<char> = self
            .input_area
            .lines()
            .first()
            .map(|line| line.chars().collect())
            .unwrap_or_default();
        if text.is_empty() {
            let placeholder = ui::truncate(PLACEHOLDER, width);
            let mut chars = placeholder.chars();
            let first = chars.next().map(String::from).unwrap_or_default();
            return Line::from(vec![
                Span::styled(first, cursor_style.fg(theme::faint())),
                Span::styled(chars.collect::<String>(), Style::new().fg(theme::faint())),
            ]);
        }

        let word_end = text
            .iter()
            .position(|c| c.is_whitespace())
            .unwrap_or(text.len());
        let word: String = text[..word_end].iter().collect();
        let word_style = Style::new().fg(if COMMANDS.contains(&word.as_str()) {
            theme::command()
        } else {
            theme::error()
        });
        let rest_style = Style::new().fg(theme::text());

        let cursor = self.input_area.cursor().1;
        let start = (cursor + 1).saturating_sub(width);
        let spans: Vec<Span<'static>> = (start..text.len().max(cursor + 1))
            .take(width)
            .map(|index| {
                let style = if index < word_end {
                    word_style
                } else {
                    rest_style
                };
                let style = if index == cursor {
                    style.add_modifier(Modifier::REVERSED)
                } else {
                    style
                };
                Span::styled(text.get(index).copied().unwrap_or(' ').to_string(), style)
            })
            .collect();
        Line::from(spans)
    }

    /// What an `rm` waiting for confirmation would delete.
    pub fn removal(&self) -> Option<&[RemovalTarget]> {
        self.removal
            .as_ref()
            .map(|removal| removal.targets.as_slice())
    }

    pub fn handle_event(&mut self, key: KeyEvent) -> bool {
        // A folder removal waits on its sheet: Enter deletes, Esc keeps.
        // Esc only cancels, so it does not also close the console.
        if let Some(removal) = self.removal.take() {
            match key.code {
                KeyCode::Enter => {
                    let paths: Vec<PathBuf> =
                        removal.targets.iter().map(|t| t.path.clone()).collect();
                    self.remove(&paths, &removal.done);
                }
                KeyCode::Esc | KeyCode::Backspace | KeyCode::Char('n') => {
                    self.notify(Tone::Info, "nothing removed");
                }
                _ => self.removal = Some(removal),
            }
            return false;
        }
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
                self.clear_output();
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

        self.clear_output();

        if self.history.last() != Some(&cmd) {
            self.history.push(cmd.clone());
            if self.history.len() > HISTORY_LIMIT {
                self.history.remove(0);
            }
        }

        if cmd == "cd" || cmd.starts_with("cd ") {
            let target = cmd[2..].trim();
            let new_path = if target.is_empty() {
                std::env::var_os("HOME")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| self.cur_dir.clone())
            } else {
                self.resolve(target)
            };

            match new_path.canonicalize() {
                Ok(canonical) if canonical.is_dir() => {
                    if canonical != self.cur_dir {
                        self.changed_at = Some(Instant::now());
                    }
                    self.cur_dir = canonical;
                }
                Ok(_) => self.say(Tone::Error, format!("not a directory — {target}")),
                Err(_) => self.say(Tone::Error, format!("no such directory — {target}")),
            }
        } else if cmd == "ls" {
            match fs::read_dir(&self.cur_dir) {
                Ok(entries) => {
                    let mut names: Vec<(bool, String)> = entries
                        .flatten()
                        .map(|entry| {
                            (
                                entry.path().is_dir(),
                                entry.file_name().to_string_lossy().to_string(),
                            )
                        })
                        .filter(|(_, name)| !name.starts_with('.'))
                        .collect();
                    // Folders first, then files, each alphabetically.
                    names.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
                    if names.is_empty() {
                        self.say(Tone::Info, "empty directory");
                    } else {
                        self.list(names);
                    }
                }
                Err(e) => self.say(Tone::Error, format!("cannot list this directory — {e}")),
            }
        } else if cmd == "pwd" {
            self.say(Tone::Info, self.cur_dir.display().to_string());
        } else if cmd == "mkdir" || cmd.starts_with("mkdir ") {
            self.make_dir(cmd[5..].trim());
        } else if cmd == "rm" || cmd.starts_with("rm ") {
            self.remove_command(&cmd[2..]);
        } else {
            self.say(
                Tone::Warning,
                format!("unknown command — {cmd} (try cd, ls, pwd, mkdir or rm)"),
            );
        }
    }

    /// `rm [-r] [-f] <path>…`: files go at once; a folder needs `-r` and an
    /// `Enter` to confirm, since nothing here goes to the Trash. A folder
    /// that holds the current one, and the home folder, are refused. If any
    /// path is wrong, nothing is removed.
    fn remove_command(&mut self, args: &str) {
        let (mut recursive, mut force) = (false, false);
        let mut targets = Vec::new();
        for arg in args.split_whitespace() {
            match arg.strip_prefix('-').filter(|flags| !flags.is_empty()) {
                Some(flags) => {
                    for flag in flags.chars() {
                        match flag {
                            'r' | 'R' => recursive = true,
                            'f' => force = true,
                            other => {
                                self.notify(
                                    Tone::Warning,
                                    format!("unknown option -{other} — rm takes -r and -f"),
                                );
                                return;
                            }
                        }
                    }
                }
                None => targets.push(arg),
            }
        }
        if targets.is_empty() {
            self.notify(Tone::Warning, "rm needs a path — rm [-r] <path>");
            return;
        }

        let home = std::env::var_os("HOME").and_then(|home| fs::canonicalize(home).ok());
        let current = fs::canonicalize(&self.cur_dir).unwrap_or_else(|_| self.cur_dir.clone());
        let mut paths = Vec::new();
        let mut names = Vec::new();
        let mut folders = Vec::new();
        for target in targets {
            let path = self.resolve(target);
            let Ok(meta) = fs::symlink_metadata(&path) else {
                if force {
                    continue;
                }
                self.notify(Tone::Error, format!("no such file or folder — {target}"));
                return;
            };
            // A link is removed itself, never what it points to.
            if meta.is_dir() {
                if !recursive {
                    self.notify(Tone::Error, format!("{target} is a folder — use rm -r"));
                    return;
                }
                let canonical = fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
                if current.starts_with(&canonical) {
                    self.notify(
                        Tone::Error,
                        format!("refusing to remove {target} — it holds the current folder"),
                    );
                    return;
                }
                if home.as_ref() == Some(&canonical) {
                    self.notify(Tone::Error, "refusing to remove your home folder");
                    return;
                }
                folders.push((target, count_entries(&path)));
            }
            paths.push(path);
            names.push(target);
        }

        let done = match paths.len() {
            0 => {
                self.notify(Tone::Info, "nothing to remove");
                return;
            }
            1 if folders.is_empty() => format!("removed {}", args_name(&paths[0])),
            1 => format!("removed {}/", folders[0].0.trim_end_matches('/')),
            count => format!("removed {count} items"),
        };
        if folders.is_empty() {
            self.remove(&paths, &done);
            return;
        }
        let targets = paths
            .into_iter()
            .zip(names)
            .map(|(path, name)| {
                let folder = folders.iter().find(|(folder, _)| *folder == name);
                RemovalTarget {
                    path,
                    name: name.trim_end_matches('/').to_string(),
                    is_dir: folder.is_some(),
                    inside: folder.map_or(0, |(_, count)| *count),
                }
            })
            .collect();
        self.removal = Some(PendingRemoval { targets, done });
    }

    fn remove(&mut self, paths: &[PathBuf], done: &str) {
        for path in paths {
            let is_dir = fs::symlink_metadata(path).is_ok_and(|meta| meta.is_dir());
            let removed = if is_dir {
                fs::remove_dir_all(path)
            } else {
                fs::remove_file(path)
            };
            if let Err(e) = removed {
                self.notify(
                    Tone::Error,
                    format!("cannot remove {} — {e}", args_name(path)),
                );
                return;
            }
        }
        self.notify(Tone::Success, done);
    }

    /// `mkdir <path>`: creates the folder and any missing parents, like
    /// `mkdir -p`, but refuses one that already exists so a typo can't pass
    /// silently. It stays in the current folder; `cd` moves.
    fn make_dir(&mut self, target: &str) {
        if target.is_empty() {
            self.notify(Tone::Warning, "mkdir needs a name — mkdir <path>");
            return;
        }
        let path = self.resolve(target);
        if path.exists() {
            let what = if path.is_dir() { "folder" } else { "file" };
            self.notify(Tone::Error, format!("a {what} already exists — {target}"));
            return;
        }
        match fs::create_dir_all(&path) {
            Ok(()) => self.notify(Tone::Success, format!("created {target}")),
            Err(e) => self.notify(Tone::Error, format!("cannot create {target} — {e}")),
        }
    }

    /// A typed path, against the current folder unless it is absolute or
    /// starts at `~/`.
    fn resolve(&self, target: &str) -> PathBuf {
        if let Some(rest) = target.strip_prefix("~/")
            && let Some(home) = std::env::var_os("HOME")
        {
            return PathBuf::from(home).join(rest);
        }
        self.cur_dir.join(target)
    }

    fn autocomplete(&mut self) {
        // Repeated Tab presses cycle through every match after the first press
        // completed the common prefix.
        if let Some(completion) = self.completion.take() {
            let (is_dir, name) = &completion.matches[completion.next];
            let next = (completion.next + 1) % completion.matches.len();
            let slash = if *is_dir { "/" } else { "" };
            self.set_input(&format!(
                "{}{}{name}{slash}",
                completion.head, completion.dir_part
            ));
            self.show_matches(&completion.matches, &completion.dir_part);
            self.completion = Some(Completion { next, ..completion });
            return;
        }

        let line = self.input_area.lines().join("");

        // `mkdir` completes the existing folders of the path it extends; `rm`
        // completes files too, in whichever of its arguments is last.
        let (head, prefix, with_files) = if line.starts_with("rm ") {
            let split = line.rfind(' ').map_or(0, |index| index + 1);
            (&line[..split], &line[split..], true)
        } else if let Some(command) = ["cd ", "mkdir "]
            .into_iter()
            .find(|command| line.starts_with(command))
        {
            (command, &line[command.len()..], false)
        } else {
            return;
        };
        if prefix.starts_with('-') {
            return;
        }

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
        let mut matches: Vec<(bool, String)> = entries
            .flatten()
            .map(|entry| {
                (
                    entry.path().is_dir(),
                    entry.file_name().to_string_lossy().to_string(),
                )
            })
            .filter(|(is_dir, name)| {
                (*is_dir || with_files)
                    && name.to_lowercase().starts_with(&needle)
                    && (!name.starts_with('.') || fragment.starts_with('.'))
            })
            .collect();
        matches.sort();

        if matches.is_empty() {
            return;
        }

        let names: Vec<String> = matches.iter().map(|(_, name)| name.clone()).collect();
        let extension = common_extension(&names);
        self.set_input(&format!("{head}{dir_part}{extension}"));
        self.history_cursor = None;

        if let [(is_dir, _)] = matches.as_slice() {
            if *is_dir {
                self.input_area.insert_str("/");
            }
            self.clear_output();
        } else {
            self.show_matches(&matches, dir_part);
            self.completion = Some(Completion {
                head: head.to_string(),
                dir_part: dir_part.to_string(),
                matches,
                next: 0,
            });
        }
    }

    fn show_matches(&mut self, matches: &[(bool, String)], dir_part: &str) {
        self.list(
            matches
                .iter()
                .map(|(is_dir, name)| (*is_dir, format!("{dir_part}{name}")))
                .collect(),
        );
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

/// `  ~ › cp › round912`: a folder icon, then the path as segments with the
/// current folder in the reading colour and its parents muted. When the
/// path is too long, leading segments give way to `…`. `glow` (0 → 1)
/// brings the current folder in from the accent after a `cd`.
fn breadcrumb(path: &Path, width: usize, glow: f32) -> Line<'static> {
    let shown = path.display().to_string();
    let shown = match std::env::var("HOME") {
        Ok(home) if !home.is_empty() && shown.starts_with(&home) => {
            format!("~{}", &shown[home.len()..])
        }
        _ => shown,
    };
    let mut segments: Vec<String> = shown
        .split('/')
        .filter(|segment| !segment.is_empty())
        .map(str::to_string)
        .collect();
    if shown.starts_with('/') {
        segments.insert(0, "/".to_string());
    }

    const SEPARATOR: &str = " › ";
    let icon = format!("{}  ", icons::FOLDER_OPEN);
    let budget = width.saturating_sub(icon.width());
    let mut dropped = false;
    let total = |segments: &[String], dropped: bool| {
        segments.iter().map(|s| s.width()).sum::<usize>()
            + SEPARATOR.width() * segments.len().saturating_sub(1)
            + if dropped { 1 + SEPARATOR.width() } else { 0 }
    };
    while segments.len() > 1 && total(&segments, dropped) > budget {
        segments.remove(0);
        dropped = true;
    }

    let p = theme::p();
    let mut spans = vec![Span::styled(icon, Style::new().fg(theme::accent()))];
    if dropped {
        spans.push(Span::styled("…", Style::new().fg(theme::faint())));
        spans.push(Span::styled(SEPARATOR, Style::new().fg(theme::faint())));
    }
    let last = segments.len().saturating_sub(1);
    for (index, segment) in segments.into_iter().enumerate() {
        if index > 0 {
            spans.push(Span::styled(SEPARATOR, Style::new().fg(theme::faint())));
        }
        let style = if index == last {
            Style::new().fg(theme::blend(p.accent, p.text, glow)).bold()
        } else {
            Style::new().fg(theme::muted())
        };
        let budget = budget.max(1);
        spans.push(Span::styled(ui::truncate(&segment, budget), style));
    }
    Line::from(spans)
}

/// A file's icon and colour, by what kind of file it is.
pub fn file_icon(is_dir: bool, name: &str) -> (&'static str, ratatui::style::Color) {
    if is_dir {
        return (icons::FOLDER, theme::accent());
    }
    let extension = name
        .rsplit_once('.')
        .map(|(_, ext)| ext.to_ascii_lowercase());
    match extension.as_deref() {
        Some("cpp" | "cc" | "cxx" | "c" | "h" | "hpp") => (icons::CPP, theme::info()),
        Some("py") => (icons::PYTHON, theme::warning()),
        Some("java") => (icons::JAVA, theme::error()),
        Some("rs") => (icons::RUST, theme::warning()),
        Some("json") => (icons::JSON, theme::warning()),
        Some("md" | "txt") => (icons::TEXT, theme::muted()),
        Some("in" | "out" | "ans") => (icons::DATA, theme::purple()),
        _ => (icons::FILE, theme::faint()),
    }
}

/// The output row: a message with its tone's glyph, or a listing as icon +
/// name pairs, cut off with a count of what did not fit.
fn output_line(output: &DirOutput, row: Rect, now: Instant) -> Line<'static> {
    let width = row.width as usize;
    if output.entries.is_empty() {
        return Line::from(vec![
            Span::styled(
                format!("{} ", output.tone.glyph(now, now)),
                Style::new().fg(output.tone.color()).bold(),
            ),
            Span::styled(
                ui::truncate_revealing(&output.text, width.saturating_sub(2), row, "Output"),
                Style::new().fg(theme::text()),
            ),
        ]);
    }
    let mut spans = Vec::new();
    let mut used = 0usize;
    for (index, (is_dir, name)) in output.entries.iter().enumerate() {
        let (icon, color) = file_icon(*is_dir, name);
        let name = if *is_dir {
            format!("{name}/")
        } else {
            name.clone()
        };
        let cell = format!("{icon} {name}    ");
        let rest = output.entries.len() - index;
        let note = format!("+{rest} more");
        if rest > 1 && used + cell.width() + note.width() > width {
            spans.push(Span::styled(note, Style::new().fg(theme::faint())));
            let listing: Vec<String> = output
                .entries
                .iter()
                .map(|(is_dir, name)| {
                    if *is_dir {
                        format!("{name}/")
                    } else {
                        name.clone()
                    }
                })
                .collect();
            ui::reveal(row, "Listing", &listing.join("\n"));
            break;
        }
        used += cell.width();
        spans.push(Span::styled(format!("{icon} "), Style::new().fg(color)));
        spans.push(Span::styled(
            format!("{name}    "),
            Style::new().fg(if *is_dir {
                theme::text()
            } else {
                theme::muted()
            }),
        ));
    }
    Line::from(spans)
}

/// How many folder confirmations count up to before saying "N+".
pub const COUNT_LIMIT: usize = 10_000;

/// Files and folders under `path`, stopping at [`COUNT_LIMIT`].
fn count_entries(path: &Path) -> usize {
    let mut count = 0;
    let mut stack = vec![path.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            count += 1;
            if count >= COUNT_LIMIT {
                return count;
            }
            if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                stack.push(entry.path());
            }
        }
    }
    count
}

/// A path as the console names it: its last component.
fn args_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().to_string(),
    )
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

    fn run(dir: &mut DirTerminal, command: &str) {
        dir.input_area.insert_str(command);
        dir.handle_event(key(KeyCode::Enter));
    }

    #[test]
    fn listing_puts_folders_first_and_hides_dotfiles() {
        let root = temp_dir("ls");
        fs::create_dir_all(root.join("round912")).unwrap();
        fs::write(root.join("A.cpp"), "").unwrap();
        fs::write(root.join(".hidden"), "").unwrap();
        let mut dir = DirTerminal::new(root.clone());

        run(&mut dir, "ls");
        let output = dir.output.as_ref().unwrap();
        assert_eq!(output.tone, Tone::Info);
        assert_eq!(
            output.entries,
            [(true, "round912".to_string()), (false, "A.cpp".to_string())]
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn cd_moves_and_errors_say_what_went_wrong() {
        let root = temp_dir("cd");
        fs::create_dir_all(root.join("round912")).unwrap();
        let mut dir = DirTerminal::new(root.clone());

        run(&mut dir, "cd round912");
        assert!(dir.cur_dir.ends_with("round912"));
        assert!(dir.changed_at.is_some());
        assert!(dir.output.is_none(), "a successful cd prints nothing");

        run(&mut dir, "cd nowhere");
        let output = dir.output.as_ref().unwrap();
        assert_eq!(output.tone, Tone::Error);
        assert_eq!(output.text, "no such directory — nowhere");

        run(&mut dir, "frobnicate");
        assert_eq!(dir.output.as_ref().unwrap().tone, Tone::Warning);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn mkdir_creates_nested_folders_and_refuses_existing_ones() {
        let root = temp_dir("mkdir");
        fs::write(root.join("A.cpp"), "").unwrap();
        let mut dir = DirTerminal::new(root.clone());

        run(&mut dir, "mkdir round912/div2");
        assert!(root.join("round912/div2").is_dir());
        assert_eq!(
            dir.take_notice(),
            Some((Tone::Success, "created round912/div2".to_string()))
        );
        assert!(dir.output.is_none(), "the result goes to the console");
        assert_eq!(dir.cur_dir, root, "mkdir does not move");

        run(&mut dir, "mkdir round912");
        assert_eq!(
            dir.take_notice(),
            Some((
                Tone::Error,
                "a folder already exists — round912".to_string()
            ))
        );

        run(&mut dir, "mkdir A.cpp");
        assert_eq!(
            dir.take_notice(),
            Some((Tone::Error, "a file already exists — A.cpp".to_string()))
        );

        run(&mut dir, "mkdir");
        assert_eq!(dir.take_notice().map(|(tone, _)| tone), Some(Tone::Warning));

        dir.input_area.insert_str("mkdir rou");
        dir.handle_event(tab());
        assert_eq!(typed(&dir), "mkdir round912/", "mkdir completes parents");

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn rm_removes_files_at_once_and_folders_after_enter() {
        let root = temp_dir("rm");
        fs::write(root.join("A.cpp"), "").unwrap();
        fs::write(root.join("B.cpp"), "").unwrap();
        fs::create_dir_all(root.join("round912/div2")).unwrap();
        fs::write(root.join("round912/C.cpp"), "").unwrap();
        let mut dir = DirTerminal::new(root.clone());

        run(&mut dir, "rm A.cpp");
        assert!(!root.join("A.cpp").exists());
        assert_eq!(
            dir.take_notice(),
            Some((Tone::Success, "removed A.cpp".to_string()))
        );

        run(&mut dir, "rm round912");
        assert_eq!(
            dir.take_notice(),
            Some((Tone::Error, "round912 is a folder — use rm -r".to_string()))
        );

        run(&mut dir, "rm -rf round912/");
        assert!(root.join("round912").exists(), "a folder waits for Enter");
        let targets = dir.removal().expect("the sheet is waiting");
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].name, "round912");
        assert!(targets[0].is_dir);
        assert_eq!(targets[0].inside, 2);
        dir.handle_event(key(KeyCode::Char('x')));
        assert!(dir.removal().is_some(), "other keys leave the sheet open");
        assert!(
            !dir.handle_event(key(KeyCode::Esc)),
            "Esc keeps the console open"
        );
        assert!(dir.removal().is_none());
        assert!(root.join("round912").exists());
        assert_eq!(
            dir.take_notice(),
            Some((Tone::Info, "nothing removed".to_string()))
        );

        run(&mut dir, "rm -rf round912");
        dir.handle_event(key(KeyCode::Enter));
        assert!(!root.join("round912").exists());
        assert_eq!(
            dir.take_notice(),
            Some((Tone::Success, "removed round912/".to_string()))
        );

        run(&mut dir, "rm B.cpp nowhere.cpp");
        assert!(
            root.join("B.cpp").exists(),
            "one wrong path removes nothing"
        );
        assert_eq!(dir.take_notice().map(|(tone, _)| tone), Some(Tone::Error));
        run(&mut dir, "rm -f B.cpp nowhere.cpp");
        assert!(!root.join("B.cpp").exists(), "-f skips missing paths");

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn command_word_takes_the_theme_colour_when_known_and_red_when_not() {
        let mut dir = DirTerminal::new(std::env::temp_dir());
        let color = |dir: &DirTerminal| dir.command_line(40).spans[0].style.fg;

        dir.input_area.insert_str("mkdir round912");
        assert_eq!(color(&dir), Some(theme::command()));
        dir.set_input("mkdi");
        assert_eq!(color(&dir), Some(theme::error()), "half a command is red");
        dir.set_input("rmdir x");
        assert_eq!(color(&dir), Some(theme::error()));

        let shown: String = dir
            .command_line(40)
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect();
        assert_eq!(shown, "rmdir x ", "the cursor cell follows the text");
    }

    #[test]
    fn rm_refuses_the_folders_holding_the_current_one() {
        let root = temp_dir("rm_guard");
        fs::create_dir_all(root.join("cp")).unwrap();
        let mut dir = DirTerminal::new(root.join("cp").canonicalize().unwrap());

        for command in ["rm -rf .", "rm -rf ..", "rm -rf /"] {
            run(&mut dir, command);
            let (tone, text) = dir.take_notice().unwrap();
            assert_eq!(tone, Tone::Error, "{command}");
            assert!(
                text.contains("holds the current folder"),
                "{command}: {text}"
            );
        }
        assert!(root.join("cp").exists());

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn rm_completes_files_as_well_as_folders() {
        let root = temp_dir("rm_complete");
        fs::write(root.join("Alpha.cpp"), "").unwrap();
        fs::create_dir_all(root.join("round912")).unwrap();
        let mut dir = DirTerminal::new(root.clone());

        dir.input_area.insert_str("rm -r round912 Al");
        dir.handle_event(tab());
        assert_eq!(typed(&dir), "rm -r round912 Alpha.cpp");

        let _ = fs::remove_dir_all(&root);
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

        let names: Vec<_> = dir
            .output
            .as_ref()
            .unwrap()
            .entries
            .iter()
            .map(|(_, name)| name.clone())
            .collect();
        assert_eq!(names, ["cp/questions", "cp/quiz"]);

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

        run(&mut dir, "ls");
        run(&mut dir, "pwd");

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
