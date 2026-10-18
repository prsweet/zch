use std::path::{Path, PathBuf};

use ratatui::{
    Frame,
    crossterm::event::{Event, KeyCode, MouseEventKind},
    layout::{Alignment, Position, Rect, Constraint::{Length, Min, Percentage}, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Clear, Paragraph, Scrollbar, ScrollbarState},
};

use crate::{
    components::{
        block,
        config::ConfigMenu,
        dir_terminal::DirTerminal,
        splash::render,
        tc_list::TestCaseList,
        testcase::{CaseVerdict, TestCase},
        theme,
    },
    core::{
        ActiveProblem, HelperCommand,
        PassingCommand::{self, ToHelper, ToRunner, ToSubmitter},
        RunnerCommand, RunAction, SubmitConfig, SubmitterCommand, Verdict,
        fs_ops::FsOps,
        submitter::{self, SubmissionInfo},
    },
};
use throbber_widgets_tui::{Throbber, ThrobberState};

pub enum InputMode {
    Normal,
    Editing,
    Directory,
    Config,
    ConfirmSubmit,
}

pub struct Submission {
    pub id: u64,
    pub label: String,
    pub message: String,
    pub color: Color,
    pub done: bool,
}

const MAX_SUBMISSIONS: usize = 20;
const SUBMISSION_VIEWPORT: usize = 4;

pub struct PendingSubmit {
    pub config: SubmitConfig,
    pub info: SubmissionInfo,
    pub test_count: usize,
    pub time_limit: u64,
}

pub struct Helper {
    pub dir_terminal: DirTerminal,
    pub active_problem: Option<ActiveProblem>,
    pub input_mode: InputMode,
    pub tc_list: TestCaseList,
    pub config_menu: ConfigMenu,
    pub fs_ops: FsOps,
    pub submissions: Vec<Submission>,
    pub running: bool,
    pub dirty: bool,
    pending_submit: Option<PendingSubmit>,
    run_id: u64,
    next_submission_id: u64,
    throbber: ThrobberState,
    submission_scroll: usize,
    submissions_area: Rect,
}

impl Helper {
    pub fn new(initial_dir: PathBuf, fs_ops: &FsOps) -> Self {
        let (config, _) = fs_ops.load_config();
        let mut dir_terminal = DirTerminal::new(initial_dir);
        dir_terminal.set_message(
            theme::CYAN,
            format!(
                "Ready · listening on 127.0.0.1:{} — send a problem with Competitive Companion",
                crate::core::receiver::listen_port()
            ),
        );

        Self {
            dir_terminal,
            active_problem: None,
            input_mode: InputMode::Normal,
            tc_list: TestCaseList::new(),
            config_menu: ConfigMenu::new(&config),
            fs_ops: fs_ops.clone(),
            submissions: Vec::new(),
            running: false,
            dirty: true,
            pending_submit: None,
            run_id: 0,
            next_submission_id: 1,
            throbber: ThrobberState::default(),
            submission_scroll: 0,
            submissions_area: Rect::default(),
        }
    }

    /// Any background work that deserves an animated loader.
    pub fn busy(&self) -> bool {
        self.running || self.submissions.iter().any(|s| !s.done)
    }

    /// Advances every animated loader by one frame.
    pub fn tick(&mut self) {
        self.throbber.calc_next();
    }

    fn spinner(&self) -> &'static str {
        let frames = throbber_widgets_tui::BRAILLE_EIGHT.symbols;
        frames[(self.throbber.index() as usize) % frames.len()]
    }

    // ------------------------------------------------------------------
    // Drawing
    // ------------------------------------------------------------------

    pub fn draw(&mut self, frame: &mut Frame) {
        let area = frame.area();

        let master_border = block(Some(" ZCH "))
            .border_style(theme::ACCENT)
            .title_style(Style::new().fg(theme::ACCENT).bold());
        let main_area = master_border.inner(area);
        frame.render_widget(master_border, area);

        let console_height = self.dir_terminal.console_height(main_area.width);
        let submit_height = self.submissions_height();
        let chunks = Layout::vertical([
            Length(3),              // working directory / command bar
            Min(0),                 // test cases
            Length(submit_height),  // submissions (bottom)
            Length(console_height), // console (bottom)
            Length(1),              // footer
        ])
        .split(main_area);

        self.dir_terminal
            .draw_input(frame, chunks[0], matches!(self.input_mode, InputMode::Directory));

        if self.tc_list.testcases.is_empty() {
            render(
                frame,
                chunks[1],
                self.spinner(),
                self.throbber.index() as i64,
                crate::core::receiver::listen_port(),
            );
        } else {
            self.tc_list
                .draw(frame, chunks[1], matches!(self.input_mode, InputMode::Editing));
        }

        self.draw_submissions(frame, chunks[2]);
        self.dir_terminal.draw_console(frame, chunks[3]);
        self.draw_footer(frame, chunks[4]);

        match self.input_mode {
            InputMode::Config => {
                let dim_bg = Block::default().style(Style::new().add_modifier(Modifier::DIM));
                frame.render_widget(dim_bg, area);
                self.config_menu.draw(frame, area);
            }
            InputMode::ConfirmSubmit => {
                let dim_bg = Block::default().style(Style::new().add_modifier(Modifier::DIM));
                frame.render_widget(dim_bg, area);
                self.draw_submit_modal(frame, area);
            }
            _ => {}
        }
    }

    fn submissions_height(&self) -> u16 {
        if self.submissions.is_empty() {
            0
        } else {
            self.submissions.len().min(SUBMISSION_VIEWPORT) as u16 + 2
        }
    }

    /// Wheel scrolling inside the submissions panel (hovered area only).
    fn scroll_submissions(&mut self, down: bool) {
        let viewport = if self.submissions_area.height >= 3 {
            self.submissions_area.height.saturating_sub(2) as usize
        } else {
            SUBMISSION_VIEWPORT
        };
        let max_scroll = self.submissions.len().saturating_sub(viewport);
        if down {
            self.submission_scroll = (self.submission_scroll + 1).min(max_scroll);
        } else {
            self.submission_scroll = self.submission_scroll.saturating_sub(1);
        }
    }

    fn draw_submissions(&mut self, frame: &mut Frame, area: Rect) {
        self.submissions_area = area;
        if area.height < 3 {
            return;
        }

        let total = self.submissions.len();
        let viewport = area.height.saturating_sub(2) as usize;
        let max_scroll = total.saturating_sub(viewport);
        self.submission_scroll = self.submission_scroll.min(max_scroll);

        let needs_scrollbar = total > viewport;
        let (list_area, scrollbar_area) = if needs_scrollbar {
            let division = Layout::horizontal([Percentage(100), Length(2)]).split(area);
            (division[0], division[1])
        } else {
            (area, Rect::default())
        };

        let spinner = self.spinner();
        let lines: Vec<Line> = self
            .submissions
            .iter()
            .rev()
            .skip(self.submission_scroll)
            .take(viewport)
            .map(|submission| {
                let glyph = if !submission.done {
                    spinner
                } else if submission.color == theme::GREEN {
                    "✔"
                } else if submission.color == theme::RED {
                    "✘"
                } else {
                    "⚠"
                };
                Line::from(vec![
                    Span::styled(format!(" {glyph} "), Style::new().fg(submission.color).bold()),
                    Span::styled(format!("{} ", submission.label), Style::new().bold()),
                    Span::styled(
                        format!("· {}", submission.message),
                        Style::new().fg(submission.color),
                    ),
                ])
            })
            .collect();

        frame.render_widget(Paragraph::new(lines).block(block(Some(" Submissions "))), list_area);

        if needs_scrollbar {
            let scrollbar = Scrollbar::new(ratatui::widgets::ScrollbarOrientation::VerticalRight)
                .style(theme::MUTED)
                .track_style(theme::BORDER)
                .begin_symbol(Some("-"))
                .end_symbol(Some("-"));
            let mut scroll_state = ScrollbarState::new(max_scroll).position(self.submission_scroll);
            frame.render_stateful_widget(scrollbar, scrollbar_area, &mut scroll_state);
        }
    }

    fn draw_footer(&mut self, frame: &mut Frame, area: Rect) {
        let mut spans: Vec<Span> = Vec::new();
        let key = |k: &str| Span::styled(format!("[{k}]"), Style::new().fg(theme::ACCENT).bold());
        let hint = |h: &str| Span::styled(h.to_string(), Style::new().fg(theme::MUTED));
        let badge = |text: &str, color: Color| {
            Span::styled(
                format!(" {text} "),
                Style::new().fg(Color::Black).bg(color).bold(),
            )
        };

        match self.input_mode {
            InputMode::Normal => {
                for (i, (k, label)) in [
                    ("a", "add case"),
                    ("r", "run"),
                    ("e", "edit"),
                    ("s", "submit"),
                    ("d", "dir"),
                    ("c", "config"),
                    ("q", "quit"),
                ]
                .iter()
                .enumerate()
                {
                    if i > 0 {
                        spans.push(hint("  "));
                    }
                    spans.push(key(k));
                    spans.push(hint(&format!(" {label}")));
                }
            }
            InputMode::Editing => {
                spans.push(badge(
                    &format!(
                        "EDITING {} / {}",
                        self.tc_list.active_area + 1,
                        self.tc_list.testcases.len()
                    ),
                    theme::GREEN,
                ));
                spans.push(hint("   "));
                spans.push(key("Tab"));
                spans.push(hint(" switch input/expected   "));
                spans.push(key("Esc"));
                spans.push(hint(" save & back"));
            }
            InputMode::Directory => {
                spans.push(badge("CONSOLE", theme::CYAN));
                spans.push(hint("   cd <path>   ls   pwd   "));
                spans.push(key("Tab"));
                spans.push(hint(" complete   "));
                spans.push(key("Esc"));
                spans.push(hint(" back"));
            }
            InputMode::Config => {
                spans.push(badge("CONFIG", theme::YELLOW));
                spans.push(hint("   "));
                spans.push(key("Tab"));
                spans.push(hint(" next field   "));
                spans.push(key("←/→"));
                spans.push(hint(" language   "));
                spans.push(key("Esc"));
                spans.push(hint(" save & close"));
            }
            InputMode::ConfirmSubmit => {
                spans.push(badge("CONFIRM SUBMISSION", theme::PURPLE));
                spans.push(hint("   "));
                spans.push(key("Enter"));
                spans.push(hint(" submit   "));
                spans.push(key("Backspace"));
                spans.push(hint(" cancel"));
            }
        }

        let footer_area = if self.busy() {
            let in_flight = self.submissions.iter().filter(|s| !s.done).count();
            let label = match (self.running, in_flight) {
                (true, 0) => " running tests…".to_string(),
                (false, n) => format!(" {n} submission{} in flight…", if n == 1 { "" } else { "s" }),
                (true, n) => format!(
                    " running tests · {n} submission{} in flight…",
                    if n == 1 { "" } else { "s" }
                ),
            };
            let width = label.chars().count() as u16 + 2;
            let chunks = Layout::horizontal([Min(0), Length(width)]).split(area);
            let busy_area = chunks[1];

            let throbber = Throbber::default()
                .throbber_set(throbber_widgets_tui::BRAILLE_EIGHT)
                .throbber_style(Style::new().fg(theme::CYAN))
                .use_type(throbber_widgets_tui::WhichUse::Spin);
            frame.render_stateful_widget(
                throbber,
                Rect { width: 1, ..busy_area },
                &mut self.throbber,
            );
            frame.render_widget(
                Paragraph::new(Span::styled(label, Style::new().fg(theme::CYAN).italic())),
                Rect {
                    x: busy_area.x + 1,
                    width: busy_area.width.saturating_sub(1),
                    ..busy_area
                },
            );
            chunks[0]
        } else {
            area
        };

        let footer = Paragraph::new(Line::from(spans)).alignment(Alignment::Center);
        frame.render_widget(footer, footer_area);
    }

    fn draw_submit_modal(&self, frame: &mut Frame, area: Rect) {
        let Some(pending) = &self.pending_submit else { return };
        let info = &pending.info;

        let width = 76.min(area.width.saturating_sub(4));
        let height = 12.min(area.height);
        let popup = Rect {
            x: area.x + area.width.saturating_sub(width) / 2,
            y: area.y + area.height.saturating_sub(height) / 2,
            width,
            height,
        };

        frame.render_widget(Clear, popup);
        let border = block(Some(&format!(" Submit to {} ", info.platform)))
            .border_style(theme::ACCENT)
            .title_style(Style::new().fg(theme::ACCENT).bold())
            .title_alignment(Alignment::Center);
        let inner = border.inner(popup);
        frame.render_widget(border, popup);

        let row = |label: &str, value: String, color: Color| -> Line<'static> {
            Line::from(vec![
                Span::styled(format!("  {label:<12}"), Style::new().fg(theme::MUTED)),
                Span::styled(value, Style::new().fg(color)),
            ])
        };

        let problem_name = self
            .active_problem
            .as_ref()
            .map(|p| p.name.clone())
            .unwrap_or_default();
        let browser = if info.safari_available {
            info.browser.to_string()
        } else {
            format!("{} — not installed!", info.browser)
        };
        let rows: Vec<Line> = vec![
            row("Problem", problem_name, Color::Reset),
            row("File", info.file_name.clone(), theme::CYAN),
            row("Platform", info.platform.to_string(), theme::YELLOW),
            row("Problem ID", info.problem_code.clone(), theme::ORANGE),
            row("Submit URL", info.submit_url.clone(), theme::ACCENT),
            row(
                "Language",
                format!("{}  (id {})", info.language, info.language_id),
                theme::GREEN,
            ),
            row(
                "Browser",
                browser,
                if info.safari_available { theme::PURPLE } else { theme::RED },
            ),
            row(
                "Local tests",
                format!("{} cases · TL {}ms", pending.test_count, pending.time_limit),
                theme::MUTED,
            ),
            Line::from(""),
            Line::from(vec![
                Span::styled("  [Enter]", Style::new().fg(theme::GREEN).bold()),
                Span::styled(" submit      ", Style::new().fg(theme::MUTED)),
                Span::styled("[Backspace]", Style::new().fg(theme::RED).bold()),
                Span::styled(" cancel", Style::new().fg(theme::MUTED)),
            ]),
        ];

        let body = Paragraph::new(rows).block(Block::default().padding(ratatui::widgets::Padding::symmetric(1, 0)));
        frame.render_widget(body, inner);
    }

    // ------------------------------------------------------------------
    // Commands
    // ------------------------------------------------------------------

    pub fn cmd_run(&mut self) -> Option<PassingCommand> {
        if self.running {
            self.dir_terminal.set_message(theme::YELLOW, "already running — wait for the verdicts");
            return None;
        }
        let Some(problem) = &self.active_problem else {
            self.dir_terminal
                .set_message(theme::RED, "no active problem — receive one or open a file via a Zed task");
            return None;
        };
        if problem.test_cases.is_empty() {
            self.dir_terminal
                .set_message(theme::YELLOW, "no test cases for this problem — press [a] to add one");
            return None;
        }

        for tc in self.tc_list.testcases.iter_mut() {
            tc.reset_result();
        }
        self.running = true;
        self.run_id = self.run_id.wrapping_add(1);
        self.dir_terminal.set_message(
            theme::CYAN,
            format!(
                "compiling & running {} case{} · TL {}ms",
                problem.test_cases.len(),
                if problem.test_cases.len() == 1 { "" } else { "s" },
                problem.effective_time_limit()
            ),
        );
        Some(ToRunner(RunnerCommand::RunCode {
            problem: problem.clone(),
            run_id: self.run_id,
        }))
    }

    fn cancel_run(&mut self) {
        self.running = false;
        self.run_id = self.run_id.wrapping_add(1);
    }

    pub fn cmd_add(&mut self) {
        if self.active_problem.is_none() {
            self.dir_terminal
                .set_message(theme::RED, "no active problem — receive one to add custom cases");
            return;
        }
        // Editing mutates cases the runner may still be writing to — retire the run.
        self.cancel_run();
        self.input_mode = InputMode::Editing;
        self.tc_list.add_tc();
        self.expand_all_cases();
        self.sync_problem_tests();
        self.dir_terminal.set_message(
            theme::GREEN,
            format!(
                "editing case {} · [Tab] switch field · [Esc] save",
                self.tc_list.testcases.len()
            ),
        );
    }

    pub fn cmd_edit(&mut self) {
        if self.active_problem.is_none() || self.tc_list.testcases.is_empty() {
            self.dir_terminal.set_message(theme::RED, "nothing to edit");
            return;
        }
        self.cancel_run();
        self.input_mode = InputMode::Editing;
        self.tc_list.focus_first();
        self.expand_all_cases();
        self.dir_terminal
            .set_message(theme::GREEN, "editing · [Tab] switch field · [Esc] save");
    }

    /// Nothing may stay hidden behind a collapsed card while editing — the
    /// user has to be able to see every case they are editing.
    fn expand_all_cases(&mut self) {
        for tc in self.tc_list.testcases.iter_mut() {
            tc.collapsed = false;
        }
        self.tc_list.update_scroll();
    }

    pub fn cmd_submit(&mut self) -> Option<PassingCommand> {
        let Some(problem) = &self.active_problem else {
            self.dir_terminal.set_message(theme::RED, "no active problem to submit");
            return None;
        };

        let (config, _) = self.fs_ops.load_config();
        let payload = SubmitConfig {
            language: config.language,
            url: problem.url.clone(),
            file_path: problem.code_file.clone(),
        };

        match submitter::describe(&payload) {
            Ok(info) => {
                self.pending_submit = Some(PendingSubmit {
                    config: payload,
                    info,
                    test_count: problem.test_cases.len(),
                    time_limit: problem.effective_time_limit(),
                });
                self.input_mode = InputMode::ConfirmSubmit;
                self.dir_terminal.set_message(
                    theme::PURPLE,
                    "confirm submission · [Enter] submit · [Backspace] cancel",
                );
            }
            Err(e) => {
                self.dir_terminal.set_message(theme::RED, format!("cannot submit: {e}"));
            }
        }
        None
    }

    pub fn external_command(&mut self, file: Option<PathBuf>, action: RunAction) -> Option<PassingCommand> {
        if let Some(file) = file
            && !self.set_active_from_file(&file) {
                return None;
            }
        match action {
            RunAction::Run => self.cmd_run(),
            RunAction::Add => {
                self.cmd_add();
                None
            }
            RunAction::Edit => {
                self.cmd_edit();
                None
            }
            RunAction::Submit => self.cmd_submit(),
        }
    }

    // ------------------------------------------------------------------
    // Problem lifecycle
    // ------------------------------------------------------------------

    /// Wire in a freshly received Competitive Companion problem. Returns the
    /// created source file path so the caller can open it in Zed.
    pub fn new_problem(&mut self, mut problem: ActiveProblem) -> Option<PathBuf> {
        problem.name = FsOps::sanitize_name(&problem.name);

        if let Err(e) = self.fs_ops.save_tc_file(&problem) {
            self.dir_terminal.set_message(theme::RED, format!("failed to save test cases: {e}"));
        }

        let code_path = self.fs_ops.write_code_file(&problem, &self.dir_terminal.cur_dir.clone());        problem.code_file = code_path.clone();

        self.tc_list.clear();
        for tc in &problem.test_cases {
            self.tc_list
                .testcases
                .push(TestCase::new(&tc.input, &tc.expected_output));
        }

        self.input_mode = InputMode::Normal;
        self.dir_terminal.set_message(
            theme::GREEN,
            format!(
                "received {} · {} test{} · TL {}ms · saved to {}",
                problem.name,
                problem.test_cases.len(),
                if problem.test_cases.len() == 1 { "" } else { "s" },
                problem.effective_time_limit(),
                code_path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()
            ),
        );

        self.active_problem = Some(problem);
        // Invalidate any run that was still in flight for the previous problem.
        self.cancel_run();
        self.dirty = true;
        Some(code_path)
    }

    fn set_active_from_file(&mut self, file: &Path) -> bool {
        if !file.exists() {
            self.dir_terminal
                .set_message(theme::RED, format!("file not found: {}", file.display()));
            return false;
        }

        let stem = file
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "problem".to_string());

        let problem = match self.fs_ops.load_problem_by_stem(&stem) {
            Ok(mut problem) => {
                problem.code_file = file.to_path_buf();
                problem
            }
            Err(_) => ActiveProblem {
                name: stem.clone(),
                url: url_from_code(file).unwrap_or_default(),
                time_limit: 0,
                test_cases: Vec::new(),
                code_file: file.to_path_buf(),
            },
        };

        self.tc_list.clear();
        for tc in &problem.test_cases {
            self.tc_list
                .testcases
                .push(TestCase::new(&tc.input, &tc.expected_output));
        }
        self.active_problem = Some(problem);
        self.cancel_run();
        self.dirty = true;
        true
    }

    /// A case finished in the runner — show it immediately.
    pub fn apply_case_result(&mut self, run_id: u64, index: usize, result: Verdict) {
        if run_id != self.run_id {
            return;
        }
        let total = self.tc_list.testcases.len();
        let Some(tc) = self.tc_list.testcases.get_mut(index) else { return };
        let expected = tc.exp_content.lines().join("\n");

        let (verdict, got) = match result {
            Verdict::Success { output, time } => {
                let got = String::from_utf8_lossy(&output.stdout).trim_end().to_string();
                let stderr = String::from_utf8_lossy(&output.stderr).trim_end().to_string();
                let is_ok = normalize(&got) == normalize(&expected);

                let mut display = got;
                if !stderr.is_empty() {
                    display.push_str(&format!("\n── stderr ──\n{stderr}"));
                }

                if is_ok {
                    (CaseVerdict::AC { ms: time }, display)
                } else {
                    (CaseVerdict::WA { ms: time }, display)
                }
            }
            Verdict::TimeLimitExceeded => (
                CaseVerdict::Tle,
                format!(
                    "time limit exceeded (>{:.2}s)",
                    self.active_problem.as_ref().map(|p| p.effective_time_limit()).unwrap_or(4000) as f64 / 1000.0
                ),
            ),
            Verdict::RuntimeError { error } => (CaseVerdict::RE, error),
            Verdict::CompilationError { error } => (CaseVerdict::CE, error),
        };

        tc.set_result(verdict, got);

        let label = match verdict {
            CaseVerdict::AC { ms } => format!("✔ case {}/{} · AC · {}ms", index + 1, total, ms),
            CaseVerdict::WA { ms } => format!("✘ case {}/{} · WA · {}ms", index + 1, total, ms),
            CaseVerdict::Tle => format!("⏱ case {}/{} · TLE", index + 1, total),
            CaseVerdict::RE => format!("✘ case {}/{} · RE", index + 1, total),
            CaseVerdict::CE => format!("✘ case {}/{} · compilation failed", index + 1, total),
            CaseVerdict::Pending => String::new(),
        };
        self.dir_terminal.set_message(verdict.color(), label);
    }

    /// All cases of a run are in: summarize and focus the first failure.
    pub fn run_finished(&mut self, run_id: u64) {
        if run_id != self.run_id {
            return;
        }
        self.running = false;

        let total = self.tc_list.testcases.len();
        if total == 0 {
            self.dir_terminal.set_message(theme::YELLOW, "no test cases to run");
            return;
        }

        let passed = self.tc_list.testcases.iter().filter(|tc| tc.verdict.is_ac()).count();
        let failed = self.tc_list.testcases.iter().filter(|tc| tc.verdict.is_failed()).count();
        let compile_error = self
            .tc_list
            .testcases
            .iter()
            .any(|tc| matches!(tc.verdict, CaseVerdict::CE));
        let total_time: u128 = self
            .tc_list
            .testcases
            .iter()
            .filter_map(|tc| match tc.verdict {
                CaseVerdict::AC { ms } | CaseVerdict::WA { ms } => Some(ms),
                _ => None,
            })
            .sum();

        let message = if compile_error {
            "✘ compilation failed — fix the errors, then press [r] again".to_string()
        } else if failed == 0 {
            format!("✔ ALL {passed}/{total} PASSED · {total_time}ms")
        } else {
            format!("✘ {failed}/{total} failed · {passed} passed · {total_time}ms")
        };

        self.dir_terminal
            .set_message(if failed == 0 { theme::GREEN } else { theme::RED }, message);

        if failed > 0
            && let Some(idx) = self.tc_list.testcases.iter().position(|tc| tc.verdict.is_failed())
        {
            self.tc_list.active_area = idx;
            self.tc_list.update_scroll();
        }
    }

    pub fn submit_status(&mut self, id: u64, message: String) {
        if let Some(submission) = self.submissions.iter_mut().find(|s| s.id == id) {
            submission.message = message;
            submission.color = theme::PURPLE;
        }
    }

    pub fn submit_finished(&mut self, id: u64, result: String) {
        let color = match submitter::classify(&result) {
            submitter::Outcome::Accepted => theme::GREEN,
            submitter::Outcome::Rejected | submitter::Outcome::Failed => theme::RED,
        };
        if let Some(submission) = self.submissions.iter_mut().find(|s| s.id == id) {
            submission.message = result;
            submission.color = color;
            submission.done = true;
        }
    }

    pub fn set_error(&mut self, message: String) {
        self.dir_terminal.set_message(theme::RED, message);
    }

    // ------------------------------------------------------------------
    // Editing
    // ------------------------------------------------------------------

    fn finish_editing(&mut self) {
        self.input_mode = InputMode::Normal;

        // Drop a trailing case that was added but left completely empty.
        if self.tc_list.testcases.len() > 1
            && let Some(last) = self.tc_list.testcases.last()
                && last.inp_content.lines().join("").trim().is_empty()
                    && last.exp_content.lines().join("").trim().is_empty()
                {
                    self.tc_list.remove_last();
                }

        if self.sync_problem_tests() {
            self.dir_terminal.set_message(theme::GREEN, "test cases saved");
        }
    }

    fn sync_problem_tests(&mut self) -> bool {
        let Some(problem) = &mut self.active_problem else { return false };
        problem.test_cases = self
            .tc_list
            .testcases
            .iter()
            .map(|tc| crate::core::ReceivingTestCase {
                input: tc.inp_content.lines().join("\n"),
                expected_output: tc.exp_content.lines().join("\n"),
            })
            .collect();
        let snapshot = problem.clone();
        if let Err(e) = self.fs_ops.save_tc_file(&snapshot) {
            self.dir_terminal.set_message(theme::RED, format!("failed to save: {e}"));
            return false;
        }
        true
    }

    // ------------------------------------------------------------------
    // Event handling
    // ------------------------------------------------------------------

    pub fn handle_event(&mut self, event: Event) -> Option<PassingCommand> {
        self.dirty = true;
        match event {
            Event::Key(key) => match self.input_mode {
                InputMode::Editing => {
                    if key.code == KeyCode::Esc {
                        self.finish_editing();
                        return None;
                    }
                    self.tc_list.handle_key(Event::Key(key));
                    self.tc_list.update_scroll();
                }
                InputMode::Normal => match key.code {
                    KeyCode::Char('q') => return Some(ToHelper(HelperCommand::Quit)),
                    KeyCode::Char('a') => self.cmd_add(),
                    KeyCode::Char('r') => return self.cmd_run(),
                    KeyCode::Char('e') => self.cmd_edit(),
                    KeyCode::Char('s') => return self.cmd_submit(),
                    KeyCode::Char('d') => {
                        self.input_mode = InputMode::Directory;
                    }
                    KeyCode::Char('c') => {
                        let (config, _) = self.fs_ops.load_config();
                        self.config_menu = ConfigMenu::new(&config);
                        self.input_mode = InputMode::Config;
                    }
                    _ => {}
                },
                InputMode::Directory => {
                    if self.dir_terminal.handle_event(key) {
                        self.input_mode = InputMode::Normal;
                    }
                }
                InputMode::Config => {
                    if self.config_menu.handle_key(Event::Key(key)) {
                        let config = self.config_menu.to_config();
                        let saved = self.fs_ops.save_config(&config);
                        self.input_mode = InputMode::Normal;
                        self.dir_terminal.set_message(
                            if saved { theme::GREEN } else { theme::RED },
                            if saved {
                                format!("config saved · language {}", config.language.label())
                            } else {
                                "failed to save config".to_string()
                            },
                        );
                    }
                }
                InputMode::ConfirmSubmit => match key.code {
                    KeyCode::Enter => {
                        if let Some(pending) = self.pending_submit.take() {
                            self.input_mode = InputMode::Normal;
                            let id = self.next_submission_id;
                            self.next_submission_id += 1;
                            let label = self
                                .active_problem
                                .as_ref()
                                .map(|p| p.name.clone())
                                .unwrap_or_else(|| pending.info.file_name.clone());
                            self.submissions.push(Submission {
                                id,
                                label: label.clone(),
                                message: format!(
                                    "submitting to {} {} · {}",
                                    pending.info.platform, pending.info.problem_code, pending.info.language
                                ),
                                color: theme::MUTED,
                                done: false,
                            });
                            self.submission_scroll = 0;
                            if self.submissions.len() > MAX_SUBMISSIONS {
                                self.submissions.remove(0);
                            }
                            return Some(ToSubmitter(SubmitterCommand::SubmitCode {
                                id,
                                config: pending.config,
                            }));
                        }
                    }
                    KeyCode::Backspace | KeyCode::Esc => {
                        self.pending_submit = None;
                        self.input_mode = InputMode::Normal;
                        self.dir_terminal
                            .set_message(theme::MUTED, "submission cancelled");
                    }
                    _ => {}
                },
            },
            Event::Mouse(mouse) => {
                let over_submissions = self
                    .submissions_area
                    .contains(Position::new(mouse.column, mouse.row));
                match mouse.kind {
                    MouseEventKind::ScrollDown if over_submissions => self.scroll_submissions(true),
                    MouseEventKind::ScrollUp if over_submissions => self.scroll_submissions(false),
                    _ => self.tc_list.handle_mouse(&mouse),
                }
            }
            _ => {}
        }
        None
    }
}

fn normalize(text: &str) -> String {
    text.trim()
        .lines()
        .map(|line| line.trim_end())
        .collect::<Vec<_>>()
        .join("\n")
}

fn url_from_code(path: &Path) -> Option<String> {
    let content = std::fs::read_to_string(path).ok()?;
    content.lines().take(5).find_map(|line| {
        line.strip_prefix("// URL: ")
            .or_else(|| line.strip_prefix("# URL: "))
            .map(|s| s.trim().to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{Language, ReceivingTestCase};
    use ratatui::{
        Terminal,
        backend::TestBackend,
        crossterm::event::{KeyEvent, KeyModifiers, MouseEvent, MouseEventKind},
    };
    use std::process::{Command, Output};

    fn temp_root(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("zcph_test_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn sample_problem() -> ActiveProblem {
        ActiveProblem {
            name: "E Prime Destruction".into(),
            url: "https://codeforces.com/contest/2266/problem/E".into(),
            time_limit: 2000,
            test_cases: vec![
                ReceivingTestCase { input: "1 2".into(), expected_output: "3".into() },
                ReceivingTestCase { input: "5".into(), expected_output: "10".into() },
            ],
            code_file: PathBuf::new(),
        }
    }

    fn ok_output(stdout: &str) -> Output {
        Output {
            status: Command::new("true").status().unwrap(),
            stdout: stdout.as_bytes().to_vec(),
            stderr: Vec::new(),
        }
    }

    fn key(code: KeyCode) -> Event {
        Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    fn type_text(helper: &mut Helper, text: &str) {
        for c in text.chars() {
            helper.handle_event(key(KeyCode::Char(c)));
        }
    }

    fn stream(helper: &mut Helper, results: Vec<Verdict>) {
        for (index, verdict) in results.into_iter().enumerate() {
            helper.apply_case_result(helper.run_id, index, verdict);
        }
        helper.run_finished(helper.run_id);
    }

    #[test]
    fn renders_problem_and_results() {
        let root = temp_root("render");
        let fs_ops = FsOps::with_root(root.clone());
        let work = root.join("problems");
        std::fs::create_dir_all(&work).unwrap();

        let mut helper = Helper::new(work.clone(), &fs_ops);
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
        terminal.draw(|f| helper.draw(f)).unwrap();

        let code_path = helper.new_problem(sample_problem()).unwrap();
        assert!(code_path.exists());
        let content = std::fs::read_to_string(&code_path).unwrap();
        assert!(content.starts_with("// URL: https://codeforces.com/contest/2266/problem/E"));

        let saved = fs_ops.load_problem_by_stem("E_Prime_Destruction").unwrap();
        assert_eq!(saved.test_cases.len(), 2);
        assert_eq!(helper.tc_list.testcases.len(), 2);

        let results = [
            Verdict::Success { output: ok_output("3\n"), time: 4 },
            Verdict::Success { output: ok_output("0\n"), time: 9 },
        ];
        // Results are applied one at a time, exactly as they arrive from the runner.
        helper.apply_case_result(helper.run_id, 0, results[0].clone());
        assert!(matches!(helper.tc_list.testcases[0].verdict, CaseVerdict::AC { ms: 4 }));
        assert!(matches!(helper.tc_list.testcases[1].verdict, CaseVerdict::Pending));

        helper.apply_case_result(helper.run_id, 1, results[1].clone());
        helper.run_finished(helper.run_id);
        terminal.draw(|f| helper.draw(f)).unwrap();

        assert!(matches!(helper.tc_list.testcases[1].verdict, CaseVerdict::WA { ms: 9 }));
        assert!(helper.tc_list.testcases[0].collapsed);
        assert!(!helper.running);

        let failures = vec![
            Verdict::TimeLimitExceeded,
            Verdict::CompilationError { error: "boom".into() },
        ];
        stream(&mut helper, failures);
        terminal.draw(|f| helper.draw(f)).unwrap();
        assert!(matches!(helper.tc_list.testcases[0].verdict, CaseVerdict::Tle));
        assert!(matches!(helper.tc_list.testcases[1].verdict, CaseVerdict::CE));

        dump_screen(&terminal);

        let _ = std::fs::remove_dir_all(&root);
    }

    /// `ZCH_DUMP=1 cargo test renders_problem -- --nocapture` to eyeball the UI.
    fn dump_screen(terminal: &Terminal<TestBackend>) {
        if std::env::var("ZCH_DUMP").is_err() {
            return;
        }
        let buffer = terminal.backend().buffer();
        let area = buffer.area;
        for y in 0..area.height {
            let mut line = String::new();
            for x in 0..area.width {
                line.push_str(buffer[(x, y)].symbol());
            }
            println!("|{line}|");
        }
    }

    #[test]
    fn add_edit_and_persist_custom_case() {
        let root = temp_root("edit");
        let fs_ops = FsOps::with_root(root.clone());
        let work = root.join("problems");
        std::fs::create_dir_all(&work).unwrap();

        let mut helper = Helper::new(work, &fs_ops);
        helper.new_problem(sample_problem());

        helper.handle_event(key(KeyCode::Char('a')));
        assert!(matches!(helper.input_mode, InputMode::Editing));
        assert_eq!(helper.tc_list.testcases.len(), 3);

        type_text(&mut helper, "7 8");
        helper.handle_event(key(KeyCode::Tab));
        type_text(&mut helper, "15");
        helper.handle_event(key(KeyCode::Esc));
        assert!(matches!(helper.input_mode, InputMode::Normal));

        let saved = fs_ops.load_problem_by_stem("E_Prime_Destruction").unwrap();
        assert_eq!(saved.test_cases.len(), 3);
        assert_eq!(saved.test_cases[2].input, "7 8");
        assert_eq!(saved.test_cases[2].expected_output, "15");

        // An added case left completely empty is dropped on save.
        helper.handle_event(key(KeyCode::Char('a')));
        helper.handle_event(key(KeyCode::Esc));
        assert_eq!(helper.tc_list.testcases.len(), 3);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn config_menu_cycles_language_and_saves() {
        let root = temp_root("config");
        let fs_ops = FsOps::with_root(root.clone());
        let work = root.join("problems");
        std::fs::create_dir_all(&work).unwrap();

        let mut helper = Helper::new(work, &fs_ops);
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();

        helper.handle_event(key(KeyCode::Char('c')));
        assert!(matches!(helper.input_mode, InputMode::Config));
        terminal.draw(|f| helper.draw(f)).unwrap();

        helper.handle_event(key(KeyCode::Right));
        helper.handle_event(key(KeyCode::Esc));
        assert!(matches!(helper.input_mode, InputMode::Normal));

        let (config, loaded) = fs_ops.load_config();
        assert!(loaded);
        assert_eq!(config.language, Language::Cpp20);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn submit_waits_for_confirmation() {
        let root = temp_root("submit");
        let fs_ops = FsOps::with_root(root.clone());
        let work = root.join("problems");
        std::fs::create_dir_all(&work).unwrap();

        let mut helper = Helper::new(work, &fs_ops);
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
        helper.new_problem(sample_problem());

        assert!(helper.handle_event(key(KeyCode::Char('s'))).is_none());
        assert!(matches!(helper.input_mode, InputMode::ConfirmSubmit));
        let pending = helper.pending_submit.as_ref().expect("pending submission");
        assert_eq!(pending.info.platform, "Codeforces");
        assert_eq!(pending.info.problem_code, "E");
        assert_eq!(pending.info.submit_url, "https://codeforces.com/contest/2266/submit");
        assert_eq!(pending.info.language_id, Language::Cpp23.cf_id());
        assert_eq!(pending.test_count, 2);
        terminal.draw(|f| helper.draw(f)).unwrap();
        dump_screen(&terminal);

        helper.handle_event(key(KeyCode::Char('x')));
        assert!(helper.pending_submit.is_some(), "other keys must not dismiss the modal");

        helper.handle_event(key(KeyCode::Backspace));
        assert!(matches!(helper.input_mode, InputMode::Normal));
        assert!(helper.pending_submit.is_none());

        helper.handle_event(key(KeyCode::Char('s')));
        let command = helper.handle_event(key(KeyCode::Enter));
        assert!(matches!(
            command,
            Some(PassingCommand::ToSubmitter(SubmitterCommand::SubmitCode { id: 1, .. }))
        ));
        assert_eq!(helper.submissions.len(), 1);
        assert!(!helper.submissions[0].done);
        assert!(matches!(helper.input_mode, InputMode::Normal));
        assert!(helper.pending_submit.is_none());

        helper.submit_status(1, "judging on Codeforces…".into());
        assert_eq!(helper.submissions[0].message, "judging on Codeforces…");
        assert_eq!(helper.submissions[0].color, theme::PURPLE);
        helper.submit_finished(1, "RESULT: ACCEPTED".into());
        assert!(helper.submissions[0].done);
        assert_eq!(helper.submissions[0].color, theme::GREEN);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn submissions_track_their_own_status_in_parallel() {
        let root = temp_root("submit_queue");
        let fs_ops = FsOps::with_root(root.clone());
        let work = root.join("problems");
        std::fs::create_dir_all(&work).unwrap();

        let mut helper = Helper::new(work, &fs_ops);
        helper.new_problem(sample_problem());

        for _ in 0..2 {
            helper.handle_event(key(KeyCode::Char('s')));
            let command = helper.handle_event(key(KeyCode::Enter));
            assert!(matches!(
                command,
                Some(PassingCommand::ToSubmitter(SubmitterCommand::SubmitCode { .. }))
            ));
        }
        assert_eq!(helper.submissions.len(), 2);
        assert_eq!(helper.submissions[0].id, 1);
        assert_eq!(helper.submissions[1].id, 2);

        helper.submit_status(2, "judging…".into());
        assert_eq!(helper.submissions[1].message, "judging…");
        assert_eq!(helper.submissions[0].color, theme::MUTED);

        helper.submit_finished(1, "RESULT: ACCEPTED".into());
        assert!(helper.submissions[0].done && !helper.submissions[1].done);
        assert_eq!(helper.submissions[1].message, "judging…");

        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
        terminal.draw(|f| helper.draw(f)).unwrap();
        dump_screen(&terminal);

        for _ in 0..MAX_SUBMISSIONS {
            helper.handle_event(key(KeyCode::Char('s')));
            helper.handle_event(key(KeyCode::Enter));
        }
        assert_eq!(helper.submissions.len(), MAX_SUBMISSIONS);
        assert_eq!(helper.submissions.first().unwrap().id, 3);
        assert_eq!(helper.submissions.last().unwrap().id, 2 + MAX_SUBMISSIONS as u64);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn submissions_panel_scrolls_internally() {
        let root = temp_root("submit_scroll");
        let fs_ops = FsOps::with_root(root.clone());
        let work = root.join("problems");
        std::fs::create_dir_all(&work).unwrap();

        let mut helper = Helper::new(work, &fs_ops);
        helper.new_problem(sample_problem());
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();

        for _ in 0..(SUBMISSION_VIEWPORT + 3) {
            helper.handle_event(key(KeyCode::Char('s')));
            helper.handle_event(key(KeyCode::Enter));
        }
        assert_eq!(helper.submissions.len(), SUBMISSION_VIEWPORT + 3);
        assert_eq!(helper.submissions_height(), SUBMISSION_VIEWPORT as u16 + 2);

        terminal.draw(|f| helper.draw(f)).unwrap();
        let area = helper.submissions_area;
        let wheel = |kind| {
            Event::Mouse(MouseEvent {
                kind,
                column: area.x + 1,
                row: area.y + 1,
                modifiers: KeyModifiers::NONE,
            })
        };

        helper.handle_event(wheel(MouseEventKind::ScrollDown));
        assert_eq!(helper.submission_scroll, 1);
        helper.handle_event(wheel(MouseEventKind::ScrollDown));
        assert_eq!(helper.submission_scroll, 2);
        helper.handle_event(wheel(MouseEventKind::ScrollUp));
        assert_eq!(helper.submission_scroll, 1);

        for _ in 0..20 {
            helper.handle_event(wheel(MouseEventKind::ScrollDown));
        }
        assert_eq!(helper.submission_scroll, 3, "scroll clamps to overflow");

        helper.handle_event(key(KeyCode::Char('s')));
        helper.handle_event(key(KeyCode::Enter));
        assert_eq!(helper.submission_scroll, 0, "new submission snaps back to newest");

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn empty_expected_output_is_not_auto_accepted() {
        let root = temp_root("strict_empty");
        let fs_ops = FsOps::with_root(root.clone());
        let work = root.join("problems");
        std::fs::create_dir_all(&work).unwrap();

        let mut helper = Helper::new(work, &fs_ops);
        let mut problem = sample_problem();
        problem.test_cases = vec![ReceivingTestCase { input: "1".into(), expected_output: String::new() }];
        helper.new_problem(problem);

        // Output against an empty expected output is a mismatch, not a pass.
        stream(&mut helper, vec![Verdict::Success { output: ok_output("lkej\n"), time: 1 }]);
        assert!(matches!(helper.tc_list.testcases[0].verdict, CaseVerdict::WA { .. }));

        // Staying silent still passes.
        stream(&mut helper, vec![Verdict::Success { output: ok_output(""), time: 1 }]);
        assert!(matches!(helper.tc_list.testcases[0].verdict, CaseVerdict::AC { .. }));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn entering_edit_mode_expands_collapsed_cases() {
        let root = temp_root("edit_expand");
        let fs_ops = FsOps::with_root(root.clone());
        let work = root.join("problems");
        std::fs::create_dir_all(&work).unwrap();

        let mut helper = Helper::new(work, &fs_ops);
        helper.new_problem(sample_problem());
        stream(
            &mut helper,
            vec![
                Verdict::Success { output: ok_output("3\n"), time: 1 },
                Verdict::Success { output: ok_output("10\n"), time: 1 },
            ],
        );
        assert!(helper.tc_list.testcases.iter().all(|tc| tc.collapsed), "AC cases auto-collapse");

        helper.handle_event(key(KeyCode::Char('e')));
        assert!(matches!(helper.input_mode, InputMode::Editing));
        assert!(
            helper.tc_list.testcases.iter().all(|tc| !tc.collapsed),
            "every case must be visible while editing"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn clicking_a_pane_in_edit_mode_moves_the_focus() {
        let root = temp_root("click_focus");
        let fs_ops = FsOps::with_root(root.clone());
        let work = root.join("problems");
        std::fs::create_dir_all(&work).unwrap();

        let mut helper = Helper::new(work, &fs_ops);
        helper.new_problem(sample_problem());
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();

        helper.handle_event(key(KeyCode::Char('e')));
        terminal.draw(|f| helper.draw(f)).unwrap();

        let click = |rect: ratatui::layout::Rect| {
            Event::Mouse(MouseEvent {
                kind: MouseEventKind::Down(ratatui::crossterm::event::MouseButton::Left),
                column: rect.x + 1,
                row: rect.y + 1,
                modifiers: KeyModifiers::NONE,
            })
        };

        let expected_rect = helper.tc_list.testcases[0].expected_box.rect;
        helper.handle_event(click(expected_rect));
        assert!(matches!(
            helper.tc_list.active_box,
            crate::components::testcase::ActiveBox::Expected
        ));
        assert_eq!(helper.tc_list.active_area, 0);

        let input_rect = helper.tc_list.testcases[0].input_box.rect;
        helper.handle_event(click(input_rect));
        assert!(matches!(
            helper.tc_list.active_box,
            crate::components::testcase::ActiveBox::Input
        ));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn directory_console_changes_dir() {
        let root = temp_root("dir");
        let fs_ops = FsOps::with_root(root.clone());
        let work = root.join("problems");
        std::fs::create_dir_all(work.join("cses")).unwrap();

        let mut helper = Helper::new(work.clone(), &fs_ops);
        helper.handle_event(key(KeyCode::Char('d')));
        assert!(matches!(helper.input_mode, InputMode::Directory));

        type_text(&mut helper, "cd cses");
        helper.handle_event(key(KeyCode::Enter));

        assert_eq!(helper.dir_terminal.cur_dir, work.join("cses").canonicalize().unwrap());

        helper.handle_event(key(KeyCode::Esc));
        assert!(matches!(helper.input_mode, InputMode::Normal));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn external_command_loads_file_by_stem() {
        let root = temp_root("external");
        let fs_ops = FsOps::with_root(root.clone());
        let work = root.join("problems");
        std::fs::create_dir_all(&work).unwrap();

        let mut helper = Helper::new(work.clone(), &fs_ops);
        let code_path = helper.new_problem(sample_problem()).unwrap();

        let mut fresh = Helper::new(work, &fs_ops);
        fresh.external_command(Some(code_path.clone()), RunAction::Add);
        assert!(matches!(fresh.input_mode, InputMode::Editing));
        assert_eq!(fresh.tc_list.testcases.len(), 3);
        assert!(fresh.active_problem.is_some());

        let _ = std::fs::remove_dir_all(&root);
    }
}
