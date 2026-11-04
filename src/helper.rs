use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use ratatui::{
    Frame,
    crossterm::event::{Event, KeyCode, KeyModifiers, MouseButton, MouseEventKind},
    layout::{Alignment, Position, Rect},
    style::{Color, Style},
    text::{Line, Span, Text},
    widgets::{Paragraph, ScrollbarOrientation, ScrollbarState},
};

use crate::{
    components::{
        ScrollbarDrag,
        config::ConfigMenu,
        dir_terminal::DirTerminal,
        scrollbar,
        splash::render,
        step_wrapped,
        tc_list::TestCaseList,
        testcase::{
            ActiveBox, CaseFocus, CaseVerdict, GOT_PAGE_LINES, PAGED_OUTPUT_NOTE, PaneKind,
            STDERR_MARKER, TestCase,
        },
        theme, ui,
    },
    core::{
        ActiveProblem, HelperCommand,
        PassingCommand::{self, ToHelper, ToRunner, ToSubmitter},
        ReceivingTestCase, RunAction, RunnerCommand, SubmitConfig, SubmitterCommand, Verdict,
        fs_ops::FsOps,
        submitter::{self, SubmissionInfo},
    },
};
use throbber_widgets_tui::ThrobberState;

pub enum InputMode {
    Normal,
    Editing,
    DeleteSelect,
    ConfirmDelete,
    Directory,
    Config,
    ConfirmSubmit,
}

enum DialogContent {
    Lines(Vec<Line<'static>>),
    Paged {
        case_index: usize,
        version: u64,
        line_count: usize,
        page_start: usize,
        page: Vec<Line<'static>>,
        warning: bool,
    },
}

impl DialogContent {
    fn total_lines(&self) -> usize {
        match self {
            Self::Lines(lines) => lines.len(),
            Self::Paged {
                line_count,
                warning,
                ..
            } => line_count + usize::from(*warning),
        }
    }

    fn max_width(&self) -> usize {
        match self {
            Self::Lines(lines) => lines.iter().map(Line::width).max().unwrap_or(0),
            Self::Paged {
                page,
                warning,
                page_start,
                ..
            } => {
                let warning_width = if *warning && *page_start == 0 {
                    Line::from(PAGED_OUTPUT_NOTE).width()
                } else {
                    0
                };
                page.iter()
                    .map(Line::width)
                    .chain(std::iter::once(warning_width))
                    .max()
                    .unwrap_or(0)
            }
        }
    }
}

struct PaneDialog {
    title: String,
    case_index: usize,
    pane: PaneKind,
    content: DialogContent,
    scroll: usize,
    h_scroll: usize,
    viewport: u16,
    viewport_width: u16,
    v_scroll_area: Rect,
    v_scroll_drag: Option<ScrollbarDrag>,
    h_scroll_area: Rect,
    h_scroll_drag: Option<ScrollbarDrag>,
    color: Color,
}

#[cfg(test)]
impl PaneDialog {
    fn visible_lines(&self) -> &[Line<'static>] {
        match &self.content {
            DialogContent::Lines(lines) => lines,
            DialogContent::Paged { page, .. } => page,
        }
    }

    fn total_lines(&self) -> usize {
        self.content.total_lines()
    }
}

fn paged_warning_line(text: &'static str) -> Line<'static> {
    Line::from(Span::styled(text, Style::new().fg(theme::YELLOW()).bold()))
}

fn sync_paged_page(content: &mut DialogContent, tc_list: &TestCaseList, scroll: usize) {
    let DialogContent::Paged {
        case_index,
        page_start,
        page,
        warning,
        ..
    } = content
    else {
        return;
    };
    let warning_rows = usize::from(*warning);
    let file_scroll = scroll.saturating_sub(warning_rows);
    let wanted = (file_scroll / GOT_PAGE_LINES) * GOT_PAGE_LINES;
    if *page_start != wanted {
        *page = tc_list
            .testcases
            .get(*case_index)
            .map(|tc| tc.got_page_lines(wanted, GOT_PAGE_LINES))
            .unwrap_or_default();
        *page_start = wanted;
    }
}

#[derive(Clone, Copy)]
enum RunRefusal {
    AlreadyRunning,
    NoProblem,
}

impl RunRefusal {
    fn message(self) -> &'static str {
        match self {
            Self::AlreadyRunning => "already running — wait for the verdicts",
            Self::NoProblem => "no active problem — receive one or open a file via a Zed task",
        }
    }

    fn color(self) -> Color {
        match self {
            Self::AlreadyRunning => theme::YELLOW(),
            Self::NoProblem => theme::RED(),
        }
    }
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
    submissions_scrollbar_area: Rect,
    submissions_scrollbar_drag: Option<ScrollbarDrag>,
    last_compile: Option<String>,
    run_cancel: Arc<AtomicBool>,
    run_started: Option<Instant>,
    pane_dialog: Option<PaneDialog>,
    last_pane_click: Option<(Instant, usize, PaneKind)>,
    show_help: bool,
    adding_case: bool,
}

impl Helper {
    pub fn new(initial_dir: PathBuf, fs_ops: &FsOps) -> Self {
        let (config, _) = fs_ops.load_config();
        let dir_terminal = DirTerminal::new(initial_dir);

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
            submissions_scrollbar_area: Rect::default(),
            submissions_scrollbar_drag: None,
            last_compile: None,
            run_cancel: Arc::new(AtomicBool::new(false)),
            run_started: None,
            pane_dialog: None,
            last_pane_click: None,
            show_help: false,
            adding_case: false,
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
        ui::canvas(frame, area);

        if area.width < 48 || area.height < 14 {
            let message = Paragraph::new(vec![
                Line::from(Span::styled(
                    "Terminal too small",
                    Style::new().fg(theme::TEXT()).bold(),
                )),
                Line::from(Span::styled(
                    format!(
                        "resize to at least 48 × 14  ({0} × {1})",
                        area.width, area.height
                    ),
                    Style::new().fg(theme::MUTED()),
                )),
            ])
            .alignment(Alignment::Center)
            .style(Style::new().bg(theme::BG()));
            let height = 2.min(area.height);
            frame.render_widget(
                message,
                Rect {
                    y: area.y + area.height.saturating_sub(height) / 2,
                    height,
                    ..area
                },
            );
            return;
        }

        // Chrome: identity bar, hairline, the path bar, a hairline, then one
        // quiet row before the case list.
        let app_bar = Rect {
            x: area.x,
            y: area.y,
            width: area.width,
            height: 1,
        };
        ui::fill(frame, app_bar, theme::SURFACE());
        self.draw_app_bar(frame, app_bar);
        ui::hairline(
            frame,
            Rect {
                x: area.x,
                y: area.y + 1,
                width: area.width,
                height: 1,
            },
        );

        let path_bar = Rect {
            x: area.x,
            y: area.y + 2,
            width: area.width,
            height: self.dir_terminal.input_height(),
        };
        self.dir_terminal.draw_input(
            frame,
            path_bar,
            matches!(self.input_mode, InputMode::Directory),
        );

        let top_rule_y = path_bar.bottom();
        ui::hairline(
            frame,
            Rect {
                x: area.x,
                y: top_rule_y,
                width: area.width,
                height: 1,
            },
        );
        let list_top = top_rule_y + 2;

        // Bottom chrome is computed from the bottom up. The last row is the
        // console bar: result or mode keys on the left, mode on the right.
        let console = Rect {
            x: area.x,
            y: area.bottom() - 1,
            width: area.width,
            height: 1,
        };
        let bottom_group_top = console.y;

        let submit_height = self.submissions_height();
        let submissions = if submit_height > 0 && bottom_group_top > list_top + 1 {
            let separator_y = bottom_group_top - 1;
            ui::hairline(
                frame,
                Rect {
                    x: area.x,
                    y: separator_y,
                    width: area.width,
                    height: 1,
                },
            );
            Rect {
                x: area.x,
                y: separator_y.saturating_sub(submit_height),
                width: area.width,
                height: submit_height,
            }
        } else {
            Rect::default()
        };

        let stack_top = if submissions.height > 0 {
            submissions.y
        } else {
            bottom_group_top
        };
        let status_rule = stack_top.saturating_sub(1);
        if status_rule > list_top {
            ui::hairline(
                frame,
                Rect {
                    x: area.x,
                    y: status_rule,
                    width: area.width,
                    height: 1,
                },
            );
        }

        let list_area = Rect {
            x: area.x + ui::GUTTER,
            y: list_top,
            width: area.width.saturating_sub(ui::GUTTER * 2),
            height: status_rule.saturating_sub(1).saturating_sub(list_top),
        };

        if list_area.width > 0 && list_area.height > 0 {
            if self.tc_list.testcases.is_empty() {
                render(
                    frame,
                    list_area,
                    self.throbber.index() as i64,
                    crate::core::receiver::listen_port(),
                );
            } else {
                let focus = match self.input_mode {
                    InputMode::Editing => CaseFocus::Edit,
                    InputMode::DeleteSelect | InputMode::ConfirmDelete => CaseFocus::Delete,
                    _ => CaseFocus::None,
                };
                self.tc_list.draw(frame, list_area, focus);
            }
        }

        self.draw_submissions(frame, submissions);

        match self.input_mode {
            InputMode::Config => {
                self.config_menu.draw(frame, area);
            }
            InputMode::ConfirmSubmit => {
                self.draw_submit_modal(frame, area);
            }
            InputMode::ConfirmDelete => {
                self.draw_delete_modal(frame, area);
            }
            _ => {}
        }

        if self.pane_dialog.is_some() {
            self.draw_pane_dialog(frame, area);
        }

        if self.show_help {
            self.draw_help(frame, area);
        }

        // Draw the console last so every mode's keys stay readable above the
        // dimmed backdrop and are never covered by a sheet.
        self.draw_console_bar(frame, console);
    }

    fn draw_app_bar(&self, frame: &mut Frame, area: Rect) {
        let inner = Rect {
            x: area.x + ui::GUTTER,
            width: area.width.saturating_sub(ui::GUTTER * 2),
            height: 1,
            ..area
        };
        let left = Line::from(vec![
            Span::styled("◆ ", Style::new().fg(theme::ACCENT()).bold()),
            Span::styled("zch", Style::new().fg(theme::TEXT()).bold()),
            Span::styled(
                format!("  v{}", env!("CARGO_PKG_VERSION")),
                Style::new().fg(theme::FAINT()),
            ),
        ]);
        let loaded = self.active_problem.is_some();
        let problem = self
            .active_problem
            .as_ref()
            .map(|problem| problem.name.as_str())
            .unwrap_or("no problem loaded");
        let dot_color = if loaded {
            theme::GREEN()
        } else {
            theme::FAINT()
        };
        let mut name_style = Style::new().fg(if loaded {
            theme::TEXT()
        } else {
            theme::FAINT()
        });
        if loaded {
            name_style = name_style.bold();
        }
        let room = (inner.width as usize)
            .saturating_sub(left.width())
            .saturating_sub(4); // two-cell gap plus the dot and its space
        let right = Line::from(vec![
            if loaded {
                ui::dot(dot_color)
            } else {
                Span::raw("")
            },
            Span::raw(" "),
            Span::styled(ui::truncate(problem, room), name_style),
        ]);
        ui::row(frame, inner, left, right);
    }

    fn submissions_height(&self) -> u16 {
        if self.submissions.is_empty() {
            0
        } else {
            self.submissions.len().min(SUBMISSION_VIEWPORT) as u16 + 1
        }
    }

    fn submissions_viewport(&self) -> usize {
        if self.submissions_area.height >= 2 {
            self.submissions_area.height.saturating_sub(1) as usize
        } else {
            SUBMISSION_VIEWPORT
        }
    }

    fn submissions_max_scroll(&self) -> usize {
        self.submissions
            .len()
            .saturating_sub(self.submissions_viewport())
    }

    /// Wheel scrolling inside the submissions panel (hovered area only).
    fn scroll_submissions(&mut self, down: bool) {
        let max_scroll = self.submissions_max_scroll();
        if down {
            self.submission_scroll = (self.submission_scroll + 1).min(max_scroll);
        } else {
            self.submission_scroll = self.submission_scroll.saturating_sub(1);
        }
        self.submissions_scrollbar_drag = None;
    }

    fn begin_submissions_scrollbar_drag(&mut self, position: Position) {
        if let Some((drag, scroll)) = ScrollbarDrag::begin(
            self.submissions_scrollbar_area,
            true,
            self.submissions_max_scroll(),
            self.submission_scroll,
            position,
        ) {
            self.submissions_scrollbar_drag = Some(drag);
            self.submission_scroll = scroll;
        }
    }

    fn draw_submissions(&mut self, frame: &mut Frame, area: Rect) {
        self.submissions_area = area;
        self.submissions_scrollbar_area = Rect::default();
        if area.height < 2 {
            self.submissions_scrollbar_drag = None;
            return;
        }

        ui::fill(frame, area, theme::SURFACE());
        let total = self.submissions.len();
        let viewport = area.height.saturating_sub(1) as usize;
        let max_scroll = total.saturating_sub(viewport);
        self.submission_scroll = self.submission_scroll.min(max_scroll);

        let needs_scrollbar = total > viewport;
        let inner = Rect {
            x: area.x + ui::GUTTER,
            width: area
                .width
                .saturating_sub(ui::GUTTER * 2 + u16::from(needs_scrollbar)),
            height: area.height,
            ..area
        };
        let header = Rect { height: 1, ..inner };
        ui::row(
            frame,
            header,
            Line::from(Span::styled(
                "SUBMISSIONS",
                Style::new().fg(theme::MUTED()).bold(),
            )),
            Line::from(Span::styled(
                format!("{total} result{}", if total == 1 { "" } else { "s" }),
                Style::new().fg(theme::FAINT()),
            )),
        );

        let spinner = self.spinner();
        let list_area = Rect {
            x: inner.x,
            y: inner.y + 1,
            width: inner.width,
            height: viewport as u16,
        };
        let lines: Vec<Line> = self
            .submissions
            .iter()
            .rev()
            .skip(self.submission_scroll)
            .take(viewport)
            .map(|submission| {
                let glyph = if !submission.done {
                    spinner
                } else if submission.color == theme::GREEN() {
                    "✔"
                } else if submission.color == theme::RED() {
                    "✘"
                } else {
                    "⚠"
                };
                Line::from(vec![
                    Span::styled(
                        format!("{glyph} "),
                        Style::new().fg(submission.color).bold(),
                    ),
                    Span::styled(
                        submission.label.to_string(),
                        Style::new().fg(theme::TEXT()).bold(),
                    ),
                    Span::styled("  ·  ", Style::new().fg(theme::LINE_STRONG())),
                    Span::styled(
                        submission.message.clone(),
                        Style::new().fg(submission.color),
                    ),
                ])
            })
            .collect();
        frame.render_widget(Paragraph::new(lines), list_area);

        if needs_scrollbar {
            let scrollbar_area = Rect {
                x: area.right() - 1,
                y: list_area.y,
                width: 1,
                height: list_area.height,
            };
            self.submissions_scrollbar_area = scrollbar_area;
            let mut scroll_state = ScrollbarState::new(max_scroll).position(self.submission_scroll);
            frame.render_stateful_widget(
                scrollbar(ScrollbarOrientation::VerticalRight),
                scrollbar_area,
                &mut scroll_state,
            );
        } else {
            self.submissions_scrollbar_drag = None;
        }
    }

    fn mode_badge(&self) -> (String, Color) {
        match self.input_mode {
            InputMode::Normal => ("NORMAL".to_string(), theme::ACCENT()),
            InputMode::Editing => (
                format!(
                    "EDIT {}/{}",
                    self.tc_list.active_area + 1,
                    self.tc_list.testcases.len()
                ),
                theme::GREEN(),
            ),
            InputMode::DeleteSelect => (
                format!(
                    "DELETE {}/{}",
                    self.tc_list.active_area + 1,
                    self.tc_list.testcases.len()
                ),
                theme::RED(),
            ),
            InputMode::ConfirmDelete => ("DELETE".to_string(), theme::RED()),
            InputMode::Directory => ("CONSOLE".to_string(), theme::CYAN()),
            InputMode::Config => ("CONFIG".to_string(), theme::YELLOW()),
            InputMode::ConfirmSubmit => ("SUBMIT".to_string(), theme::PURPLE()),
        }
    }

    /// Every mode writes its own keys into the console bar; the sheets stay
    /// content-only so a key hint never appears twice.
    fn mode_keys(&self, width: u16) -> Option<Line<'static>> {
        let pairs: &[(&str, &str)] = if self.show_help {
            &[("h/F1/Esc", "close")]
        } else if self.pane_dialog.is_some() {
            &[
                ("Esc", "close"),
                ("↑/↓", "scroll"),
                ("←/→", "pan"),
                ("PgUp/PgDn", "page"),
            ]
        } else {
            match self.input_mode {
                InputMode::Editing => &[("Tab/⇧Tab", "pane"), ("Esc", "save")],
                InputMode::DeleteSelect => &[
                    ("Tab/⇧Tab", "select"),
                    ("Enter", "confirm"),
                    ("Esc", "cancel"),
                ],
                InputMode::ConfirmDelete => &[("Enter", "delete"), ("Backspace", "back")],
                InputMode::Directory => &[("Tab", "complete"), ("Esc", "close")],
                InputMode::Config => self.config_menu.key_hints(),
                InputMode::ConfirmSubmit => &[("Enter", "submit"), ("Backspace", "cancel")],
                InputMode::Normal => return None,
            }
        };

        let build = |labels: bool| {
            let mut spans = Vec::new();
            for (index, (key, label)) in pairs.iter().enumerate() {
                if index > 0 {
                    spans.push(Span::raw("   "));
                }
                spans.push(ui::key(key));
                if labels {
                    spans.push(Span::styled(
                        format!(" {label}"),
                        Style::new().fg(theme::MUTED()),
                    ));
                }
            }
            Line::from(spans)
        };
        let full = build(true);
        // Drop the descriptions when the bar cannot hold them instead of
        // clipping a label halfway through.
        Some(if full.width() <= width as usize {
            full
        } else {
            build(false)
        })
    }

    fn draw_console_bar(&self, frame: &mut Frame, area: Rect) {
        ui::fill(frame, area, theme::SURFACE());
        let (mode, color) = self.mode_badge();
        let inner = Rect {
            x: area.x + ui::GUTTER,
            y: area.y,
            width: area.width.saturating_sub(ui::GUTTER * 2),
            height: 1,
        };
        let right = Line::from(ui::badge(&mode, color));
        let room = inner.width.saturating_sub(right.width() as u16 + 2) as usize;
        // A mode's keys own the console while it is active; a live error stays
        // visible in the modes that type into the console.
        let inline = matches!(
            self.input_mode,
            InputMode::Editing | InputMode::DeleteSelect | InputMode::Directory
        );
        let left = if inline && self.dir_terminal.has_error() {
            self.dir_terminal.console_line(room).unwrap_or_default()
        } else if let Some(keys) = self.mode_keys(room as u16) {
            keys
        } else {
            self.dir_terminal.console_line(room).unwrap_or_default()
        };
        ui::row(frame, inner, left, right);
    }

    fn draw_help(&self, frame: &mut Frame, area: Rect) {
        // Mode keys live in the console bar, so the map only teaches the
        // global keys. Help itself is a Normal-mode sheet.
        let rows: &[(&str, &str)] = &[
            ("a", "add case"),
            ("x", "delete case"),
            ("r", "run tests"),
            ("e", "edit cases"),
            ("s", "submit"),
            ("d", "directory"),
            ("c", "config"),
            ("q", "quit"),
        ];

        let key_width = rows
            .iter()
            .map(|(key, _)| key.chars().count())
            .max()
            .unwrap_or(0);
        let description_width = rows
            .iter()
            .map(|(_, description)| description.chars().count())
            .max()
            .unwrap_or(0);
        let width = (key_width + description_width + 12)
            .min(area.width.saturating_sub(2) as usize)
            .max(32) as u16;
        let height = (rows.len() as u16 + 4).min(area.height.saturating_sub(2).max(6));
        let sheet = ui::sheet(frame, area, "Keys", theme::ACCENT(), width, height);

        let mut lines = Vec::new();
        for (key, description) in rows {
            lines.push(Line::from(vec![
                Span::styled(
                    format!("[{}]", key),
                    Style::new().fg(theme::ACCENT()).bold(),
                ),
                Span::raw(" ".repeat(key_width.saturating_sub(key.chars().count()) + 3)),
                Span::styled(*description, Style::new().fg(theme::TEXT())),
            ]));
        }
        frame.render_widget(Paragraph::new(lines), sheet.body);
    }

    fn draw_submit_modal(&self, frame: &mut Frame, area: Rect) {
        let Some(pending) = &self.pending_submit else {
            return;
        };
        let info = &pending.info;

        let sheet = ui::sheet(
            frame,
            area,
            &format!("Submit · {}", info.platform),
            theme::ACCENT(),
            78,
            15.min(area.height),
        );
        let body_width = sheet.body.width.saturating_sub(14) as usize;

        let row = |label: &str, value: String, color: Color| -> Line<'static> {
            Line::from(vec![
                Span::styled(format!("{label:<13}"), Style::new().fg(theme::MUTED())),
                Span::styled(ui::truncate(&value, body_width), Style::new().fg(color)),
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
            row("Problem", problem_name, theme::TEXT()),
            row("File", info.file_name.clone(), theme::TEXT()),
            row("Platform", info.platform.to_string(), theme::TEXT()),
            row("Problem ID", info.problem_code.clone(), theme::TEXT()),
            row(
                "Language",
                format!("{}  ·  id {}", info.language, info.language_id),
                theme::TEXT(),
            ),
            row(
                "Browser",
                browser,
                if info.safari_available {
                    theme::TEXT()
                } else {
                    theme::RED()
                },
            ),
            row(
                "Local tests",
                format!(
                    "{} case{}  ·  TL {} ms",
                    pending.test_count,
                    if pending.test_count == 1 { "" } else { "s" },
                    pending.time_limit
                ),
                theme::MUTED(),
            ),
            row("Submit URL", info.submit_url.clone(), theme::MUTED()),
        ];

        frame.render_widget(Paragraph::new(rows), sheet.body);
    }

    fn draw_delete_modal(&self, frame: &mut Frame, area: Rect) {
        let total = self.tc_list.testcases.len();
        let Some(tc) = self.tc_list.testcases.get(self.tc_list.active_area) else {
            return;
        };

        let sheet = ui::sheet(
            frame,
            area,
            "Delete test case",
            theme::RED(),
            70,
            10.min(area.height),
        );

        let preview = |text: &str| -> String {
            let first = text.lines().next().unwrap_or("").trim();
            if first.is_empty() {
                return "(empty)".to_string();
            }
            ui::truncate(first, 46)
        };

        let input = preview(&tc.inp_content.lines().join("\n"));
        let expected = preview(&tc.exp_content.lines().join("\n"));
        let rows: Vec<Line> = vec![
            Line::from(vec![
                Span::styled(
                    format!("Delete case {} of {}?", self.tc_list.active_area + 1, total),
                    Style::new().fg(theme::RED()).bold(),
                ),
                Span::styled("  This cannot be undone.", Style::new().fg(theme::MUTED())),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled(format!("{:<10}", "Input"), Style::new().fg(theme::MUTED())),
                Span::styled(input, Style::new().fg(theme::TEXT())),
            ]),
            Line::from(vec![
                Span::styled(
                    format!("{:<10}", "Expected"),
                    Style::new().fg(theme::MUTED()),
                ),
                Span::styled(expected, Style::new().fg(theme::TEXT())),
            ]),
        ];

        frame.render_widget(Paragraph::new(rows), sheet.body);
    }

    // ------------------------------------------------------------------
    // Commands
    // ------------------------------------------------------------------

    pub fn cmd_run(&mut self) -> Option<PassingCommand> {
        match self.begin_run() {
            Ok(command) => Some(command),
            Err(refusal) => {
                self.dir_terminal
                    .set_error(refusal.color(), refusal.message());
                None
            }
        }
    }

    /// The only place that starts a run — both the TUI key and `zch send`
    /// go through it, so the "already running" guard cannot drift.
    fn begin_run(&mut self) -> Result<PassingCommand, RunRefusal> {
        if self.running {
            return Err(RunRefusal::AlreadyRunning);
        }
        let Some(mut problem) = self.active_problem.clone() else {
            return Err(RunRefusal::NoProblem);
        };

        let mut smoke = false;
        if problem.test_cases.is_empty() {
            // A file opened via `zch send r` has no stored cases; running it
            // with empty input is still useful as a compile + crash check.
            if self.tc_list.testcases.is_empty() {
                self.tc_list.testcases.push(TestCase::new("", ""));
            }
            problem.test_cases = vec![ReceivingTestCase {
                input: String::new(),
                expected_output: String::new(),
            }];
            smoke = true;
        }

        for tc in self.tc_list.testcases.iter_mut() {
            tc.reset_result();
        }
        self.last_compile = None;
        self.running = true;
        self.run_started = Some(Instant::now());
        self.run_id = self.run_id.wrapping_add(1);
        let cancel = Arc::new(AtomicBool::new(false));
        self.run_cancel = cancel.clone();
        self.dir_terminal.clear_result();
        self.dir_terminal.clear_error();
        self.dir_terminal.set_live(
            theme::CYAN(),
            if smoke {
                "no stored cases — smoke-running with empty input & expected output".to_string()
            } else {
                format!(
                    "running {} case{} · TL {}ms",
                    problem.test_cases.len(),
                    if problem.test_cases.len() == 1 {
                        ""
                    } else {
                        "s"
                    },
                    problem.effective_time_limit()
                )
            },
        );
        Ok(ToRunner(RunnerCommand::RunCode {
            problem,
            run_id: self.run_id,
            cancel,
        }))
    }

    /// Live compile/run status from the runner.
    pub fn run_phase(&mut self, run_id: u64, message: String) {
        if run_id != self.run_id || !self.running {
            return;
        }
        if message.starts_with("compiled in") {
            self.last_compile = Some(message.clone());
        } else if message.starts_with("sources unchanged") {
            self.last_compile = Some("compile skipped".to_string());
        }
        self.dir_terminal.set_live(theme::CYAN(), message);
    }

    fn cancel_run(&mut self) {
        self.dir_terminal.clear_live();
        if self.running
            && let Some(started) = self.run_started.take()
        {
            self.dir_terminal.set_result(
                theme::YELLOW(),
                format!("run cancelled after {}ms", started.elapsed().as_millis()),
            );
        }
        self.run_started = None;
        self.run_cancel.store(true, Ordering::SeqCst);
        self.running = false;
        self.run_id = self.run_id.wrapping_add(1);
    }

    pub fn cmd_add(&mut self) -> bool {
        if self.active_problem.is_none() {
            self.dir_terminal.set_error(
                theme::RED(),
                "no active problem — receive one to add custom cases",
            );
            return false;
        }
        // Editing mutates cases the runner may still be writing to — retire the run.
        self.cancel_run();
        self.input_mode = InputMode::Editing;
        self.adding_case = true;
        self.tc_list.add_tc();
        self.expand_all_cases();
        self.sync_problem_tests();
        true
    }

    pub fn cmd_edit(&mut self) -> bool {
        if self.active_problem.is_none() || self.tc_list.testcases.is_empty() {
            self.dir_terminal.set_error(theme::RED(), "nothing to edit");
            return false;
        }
        self.cancel_run();
        self.input_mode = InputMode::Editing;
        self.adding_case = false;
        self.tc_list.focus_first();
        self.expand_all_cases();
        true
    }

    /// `x` enters delete mode: the focused case is highlighted, `Tab` picks
    /// another one, `Enter` asks for confirmation, and only that deletes.
    pub fn cmd_delete(&mut self) {
        if self.tc_list.testcases.is_empty() {
            self.dir_terminal
                .set_error(theme::RED(), "no test case to delete");
            return;
        }
        // A stale run must not write verdicts into a list that is about to shift.
        self.cancel_run();
        self.expand_all_cases();
        self.tc_list.active_area = self
            .tc_list
            .active_area
            .min(self.tc_list.testcases.len() - 1);
        self.tc_list.update_scroll();
        self.input_mode = InputMode::DeleteSelect;
    }

    /// Moves the delete selection by one case, wrapping around.
    fn cycle_delete_selection(&mut self, backwards: bool) {
        let total = self.tc_list.testcases.len();
        if total == 0 {
            return;
        }
        self.tc_list.active_area = step_wrapped(self.tc_list.active_area, total, backwards);
        self.tc_list.update_scroll();
    }

    /// The confirmed half of the delete flow.
    fn delete_selected_case(&mut self) {
        let total = self.tc_list.testcases.len();
        if total == 0 {
            self.input_mode = InputMode::Normal;
            return;
        }
        let index = self.tc_list.active_area.min(total - 1);
        self.tc_list.testcases.remove(index);
        self.tc_list.active_area = self
            .tc_list
            .active_area
            .min(self.tc_list.testcases.len().saturating_sub(1));
        self.tc_list.active_box = ActiveBox::Input;
        self.tc_list.update_scroll();
        self.input_mode = InputMode::Normal;
        if self.sync_problem_tests() {
            self.dir_terminal.set_result(
                theme::YELLOW(),
                format!(
                    "deleted case {} · {} left",
                    index + 1,
                    self.tc_list.testcases.len()
                ),
            );
        }
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
            self.dir_terminal
                .set_error(theme::RED(), "no active problem to submit");
            return None;
        };

        let (config, _) = self.fs_ops.load_config();
        let payload = SubmitConfig {
            language: config.language,
            extra_flags: config.active_flags().to_vec(),
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
            }
            Err(e) => {
                self.dir_terminal
                    .set_error(theme::RED(), format!("cannot submit: {e}"));
            }
        }
        None
    }

    /// Handle an externally triggered command (HTTP / Zed task). The result is
    /// reported back to the caller so `zch send` can print a real reason.
    pub fn external_command(
        &mut self,
        file: Option<PathBuf>,
        action: RunAction,
    ) -> (Option<PassingCommand>, Result<String, String>) {
        if let Some(file) = file
            && let Err(error) = self.set_active_from_file(&file)
        {
            return (None, Err(error));
        }
        match action {
            RunAction::Run => match self.begin_run() {
                Ok(command) => {
                    let cases = self.tc_list.testcases.len();
                    (
                        Some(command),
                        Ok(format!(
                            "running {cases} test case{}",
                            if cases == 1 { "" } else { "s" }
                        )),
                    )
                }
                Err(refusal) => {
                    self.dir_terminal
                        .set_error(refusal.color(), refusal.message());
                    (None, Err(refusal.message().to_string()))
                }
            },
            RunAction::Add => {
                if self.cmd_add() {
                    (None, Ok("opened a new test case in the TUI".to_string()))
                } else {
                    (
                        None,
                        Err("no active problem — receive one first".to_string()),
                    )
                }
            }
            RunAction::Edit => {
                if self.cmd_edit() {
                    (None, Ok("opened the test editor in the TUI".to_string()))
                } else {
                    (None, Err("nothing to edit".to_string()))
                }
            }
            RunAction::Submit => {
                self.cmd_submit();
                if self.pending_submit.is_some() {
                    (
                        None,
                        Ok("confirmation opened — finish it in the TUI".to_string()),
                    )
                } else {
                    (None, Err("cannot submit — see the TUI console".to_string()))
                }
            }
        }
    }

    // ------------------------------------------------------------------
    // Problem lifecycle
    // ------------------------------------------------------------------

    /// Wire in a freshly received Competitive Companion problem. Returns the
    /// created source file path so the caller can open it in Zed.
    pub fn new_problem(&mut self, mut problem: ActiveProblem) -> Option<PathBuf> {
        let code_dir = self.dir_terminal.cur_dir.clone();
        problem.name = self
            .fs_ops
            .claim_problem_stem(&problem.name, &problem.url, &code_dir);

        let save_error = self.fs_ops.save_tc_file(&problem).err();

        let code_path = self.fs_ops.write_code_file(&problem, &code_dir);
        problem.code_file = code_path.clone();

        self.attach_problem(problem);
        self.input_mode = InputMode::Normal;
        if let Some(e) = save_error {
            self.dir_terminal
                .set_error(theme::RED(), format!("failed to save test cases: {e}"));
        }
        Some(code_path)
    }

    fn set_active_from_file(&mut self, file: &Path) -> Result<(), String> {
        if !file.exists() {
            let message = format!("file not found: {}", file.display());
            self.dir_terminal.set_error(theme::RED(), message.clone());
            return Err(message);
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

        self.attach_problem(problem);
        Ok(())
    }

    /// Make `problem` current, replacing the previous cases and retiring any
    /// in-flight run. Shared by receiver payloads and `zch send`.
    fn attach_problem(&mut self, problem: ActiveProblem) {
        self.tc_list.clear();
        for tc in &problem.test_cases {
            self.tc_list
                .testcases
                .push(TestCase::new(&tc.input, &tc.expected_output));
        }
        self.active_problem = Some(problem);
        self.adding_case = false;
        self.cancel_run();
        // A new problem starts on a clean console; the previous run's result
        // no longer describes what is on screen.
        self.dir_terminal.clear_live();
        self.dir_terminal.clear_result();
        self.dir_terminal.clear_error();
        self.dirty = true;
    }

    /// A case finished in the runner — show it immediately.
    pub fn apply_case_result(&mut self, run_id: u64, index: usize, result: Verdict) {
        if run_id != self.run_id {
            return;
        }
        let total = self.tc_list.testcases.len();
        let Some(tc) = self.tc_list.testcases.get_mut(index) else {
            return;
        };
        let expected = tc.exp_content.lines().join("\n");

        let (verdict, got) = match result {
            Verdict::Success { output, time } => {
                let got = String::from_utf8_lossy(&output.stdout)
                    .trim_end()
                    .to_string();
                let stderr = String::from_utf8_lossy(&output.stderr)
                    .trim_end()
                    .to_string();
                let is_ok = normalized_eq(&got, &expected);

                let mut display = got;
                if !stderr.is_empty() {
                    display.push_str(&format!("{STDERR_MARKER}{stderr}"));
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
                    self.active_problem
                        .as_ref()
                        .map(|p| p.effective_time_limit())
                        .unwrap_or(4000) as f64
                        / 1000.0
                ),
            ),
            Verdict::RuntimeError { error } => (CaseVerdict::RE, error),
            Verdict::CompilationError { error } => (CaseVerdict::CE, error),
        };

        tc.set_result(verdict, got);
        if self
            .pane_dialog
            .as_ref()
            .is_some_and(|dialog| dialog.pane == PaneKind::Got && dialog.case_index == index)
        {
            self.pane_dialog = None;
        }

        if verdict.is_failed() {
            let label = match verdict {
                CaseVerdict::AC { ms } => format!("✔ case {}/{} · AC · {}ms", index + 1, total, ms),
                CaseVerdict::WA { ms } => format!("✘ case {}/{} · WA · {}ms", index + 1, total, ms),
                CaseVerdict::Tle => format!("⏱ case {}/{} · TLE", index + 1, total),
                CaseVerdict::RE => format!("✘ case {}/{} · RE", index + 1, total),
                CaseVerdict::CE => format!("✘ case {}/{} · compilation failed", index + 1, total),
                CaseVerdict::Pending => String::new(),
            };
            self.dir_terminal.set_result(verdict.color(), label);
        }
    }

    /// All cases of a run are in: summarize and focus the first failure.
    pub fn run_finished(&mut self, run_id: u64, elapsed_ms: u128) {
        if run_id != self.run_id {
            return;
        }
        self.running = false;
        self.run_started = None;
        self.dir_terminal.clear_live();
        let wall = elapsed_ms;

        let total = self.tc_list.testcases.len();
        if total == 0 {
            self.dir_terminal
                .set_result(theme::YELLOW(), "no test cases to run");
            return;
        }

        let passed = self
            .tc_list
            .testcases
            .iter()
            .filter(|tc| tc.verdict.is_ac())
            .count();
        let failed = self
            .tc_list
            .testcases
            .iter()
            .filter(|tc| tc.verdict.is_failed())
            .count();
        let compile_error = self
            .tc_list
            .testcases
            .iter()
            .any(|tc| matches!(tc.verdict, CaseVerdict::CE));

        let compile = self
            .last_compile
            .take()
            .map(|phase| format!(" · {phase}"))
            .unwrap_or_default();
        let message = if compile_error {
            "✘ compilation failed — fix the errors, then press [r] again".to_string()
        } else if failed == 0 {
            format!("✔ ALL {passed}/{total} PASSED{compile} · {wall}ms total")
        } else {
            format!("✘ {failed}/{total} failed · {passed} passed{compile} · {wall}ms total")
        };

        self.dir_terminal.set_result(
            if failed == 0 {
                theme::GREEN()
            } else {
                theme::RED()
            },
            message,
        );

        if failed > 0
            && let Some(idx) = self
                .tc_list
                .testcases
                .iter()
                .position(|tc| tc.verdict.is_failed())
        {
            self.tc_list.active_area = idx;
            self.tc_list.update_scroll();
        }
    }

    pub fn submit_status(&mut self, id: u64, message: String) {
        if let Some(submission) = self.submissions.iter_mut().find(|s| s.id == id) {
            submission.message = message;
            submission.color = theme::PURPLE();
        }
    }

    pub fn submit_finished(&mut self, id: u64, result: String) {
        let color = match submitter::classify(&result) {
            submitter::Outcome::PartiallyAccepted => theme::YELLOW(),
            submitter::Outcome::Accepted => theme::GREEN(),
            submitter::Outcome::Rejected | submitter::Outcome::Failed => theme::RED(),
        };
        if let Some(submission) = self.submissions.iter_mut().find(|s| s.id == id) {
            submission.message = result;
            submission.color = color;
            submission.done = true;
        }
    }

    pub fn set_error(&mut self, message: String) {
        self.dir_terminal.set_error(theme::RED(), message);
    }

    // ------------------------------------------------------------------
    // Editing
    // ------------------------------------------------------------------

    fn finish_editing(&mut self) {
        self.input_mode = InputMode::Normal;
        self.adding_case = false;

        // Drop a trailing case that was added but left completely empty.
        if self.tc_list.testcases.len() > 1
            && let Some(last) = self.tc_list.testcases.last()
            && last.inp_content.lines().join("").trim().is_empty()
            && last.exp_content.lines().join("").trim().is_empty()
        {
            self.tc_list.remove_last();
        }

        if self.sync_problem_tests() {
            self.dir_terminal
                .set_result(theme::GREEN(), "test cases saved");
        }
    }

    fn sync_problem_tests(&mut self) -> bool {
        let Some(problem) = &mut self.active_problem else {
            return false;
        };
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
            self.dir_terminal
                .set_error(theme::RED(), format!("failed to save: {e}"));
            return false;
        }
        true
    }

    fn open_pane_dialog(&mut self, index: usize, pane: PaneKind) {
        let Some(tc) = self.tc_list.testcases.get(index) else {
            return;
        };
        let (label, color) = match pane {
            PaneKind::Input => ("Input", theme::CYAN()),
            PaneKind::Expected => ("Expected", theme::TEXT()),
            PaneKind::Got => (
                "Got",
                if matches!(tc.verdict, CaseVerdict::Pending) {
                    theme::MUTED()
                } else {
                    tc.verdict.color()
                },
            ),
        };
        let content = if pane == PaneKind::Got && tc.got_is_paged() {
            DialogContent::Paged {
                case_index: index,
                version: tc.got_version(),
                line_count: tc.got_line_count(),
                page_start: 0,
                page: tc.got_page_lines(0, GOT_PAGE_LINES),
                warning: true,
            }
        } else {
            let (mut lines, warning) = match pane {
                PaneKind::Got => tc.got_dialog_view(),
                PaneKind::Input | PaneKind::Expected => {
                    let text = tc.pane_text(pane);
                    let lines = if text.is_empty() {
                        vec![Line::from("(empty)")]
                    } else {
                        text.split('\n')
                            .map(|line| Line::from(line.to_string()))
                            .collect()
                    };
                    (lines, None)
                }
            };
            if let Some(warning) = warning {
                lines.insert(0, paged_warning_line(warning));
            }
            DialogContent::Lines(lines)
        };
        self.pane_dialog = Some(PaneDialog {
            title: format!("{label} · TestCase {}", index + 1),
            case_index: index,
            pane,
            content,
            scroll: 0,
            h_scroll: 0,
            viewport: 10,
            viewport_width: 10,
            v_scroll_area: Rect::default(),
            v_scroll_drag: None,
            h_scroll_area: Rect::default(),
            h_scroll_drag: None,
            color,
        });
        self.last_pane_click = None;
    }

    fn handle_pane_dialog_event(&mut self, event: Event) {
        match event {
            Event::Key(key) => match key.code {
                KeyCode::Esc => {
                    self.pane_dialog = None;
                }
                KeyCode::Up => self.scroll_pane_dialog(-1),
                KeyCode::Down => self.scroll_pane_dialog(1),
                KeyCode::Left => self.scroll_pane_dialog_horizontal(-1),
                KeyCode::Right => self.scroll_pane_dialog_horizontal(1),
                KeyCode::PageUp => {
                    let page = self
                        .pane_dialog
                        .as_ref()
                        .map(|dialog| dialog.viewport.max(1) as i32)
                        .unwrap_or(1);
                    self.scroll_pane_dialog(-page);
                }
                KeyCode::PageDown => {
                    let page = self
                        .pane_dialog
                        .as_ref()
                        .map(|dialog| dialog.viewport.max(1) as i32)
                        .unwrap_or(1);
                    self.scroll_pane_dialog(page);
                }
                KeyCode::Home => self.set_pane_dialog_scroll(0),
                KeyCode::End => self.set_pane_dialog_scroll(usize::MAX),
                _ => {}
            },
            Event::Mouse(mouse) => match mouse.kind {
                MouseEventKind::ScrollDown => self.scroll_pane_dialog(1),
                MouseEventKind::ScrollUp => self.scroll_pane_dialog(-1),
                MouseEventKind::ScrollLeft => self.scroll_pane_dialog_horizontal(-1),
                MouseEventKind::ScrollRight => self.scroll_pane_dialog_horizontal(1),
                MouseEventKind::Down(MouseButton::Left) => {
                    let position = Position::new(mouse.column, mouse.row);
                    let vertical = self.pane_dialog.as_ref().and_then(|dialog| {
                        dialog.v_scroll_area.contains(position).then_some((
                            dialog.v_scroll_area,
                            dialog
                                .content
                                .total_lines()
                                .saturating_sub(dialog.viewport.max(1) as usize),
                            dialog.scroll,
                        ))
                    });
                    if let Some((area, max_scroll, scroll)) = vertical {
                        if let Some((drag, scroll)) =
                            ScrollbarDrag::begin(area, true, max_scroll, scroll, position)
                            && let Some(dialog) = self.pane_dialog.as_mut()
                        {
                            dialog.v_scroll_drag = Some(drag);
                            dialog.h_scroll_drag = None;
                            dialog.scroll = scroll;
                        }
                        return;
                    }

                    let horizontal = self.pane_dialog.as_ref().and_then(|dialog| {
                        dialog.h_scroll_area.contains(position).then_some((
                            dialog.h_scroll_area,
                            dialog
                                .content
                                .max_width()
                                .saturating_sub(dialog.viewport_width.max(1) as usize),
                            dialog.h_scroll,
                        ))
                    });
                    if let Some((area, max_scroll, scroll)) = horizontal {
                        if let Some((drag, scroll)) =
                            ScrollbarDrag::begin(area, false, max_scroll, scroll, position)
                            && let Some(dialog) = self.pane_dialog.as_mut()
                        {
                            dialog.h_scroll_drag = Some(drag);
                            dialog.v_scroll_drag = None;
                            dialog.h_scroll = scroll;
                        }
                        return;
                    }

                    if let Some(dialog) = self.pane_dialog.as_mut() {
                        dialog.v_scroll_drag = None;
                        dialog.h_scroll_drag = None;
                    }
                }
                MouseEventKind::Drag(MouseButton::Left) => {
                    let position = Position::new(mouse.column, mouse.row);
                    if let Some(dialog) = self.pane_dialog.as_mut() {
                        if let Some(drag) = dialog.v_scroll_drag {
                            dialog.scroll = drag.scroll_at(position);
                        } else if let Some(drag) = dialog.h_scroll_drag {
                            dialog.h_scroll = drag.scroll_at(position);
                        }
                    }
                }
                MouseEventKind::Up(MouseButton::Left) => {
                    if let Some(dialog) = self.pane_dialog.as_mut() {
                        dialog.v_scroll_drag = None;
                        dialog.h_scroll_drag = None;
                    }
                }
                _ => {}
            },
            _ => {}
        }
    }

    fn scroll_pane_dialog(&mut self, rows: i32) {
        let Some(dialog) = self.pane_dialog.as_mut() else {
            return;
        };
        let max = dialog
            .content
            .total_lines()
            .saturating_sub(dialog.viewport.max(1) as usize);
        dialog.scroll = (dialog.scroll as i64 + rows as i64).clamp(0, max as i64) as usize;
    }

    fn set_pane_dialog_scroll(&mut self, scroll: usize) {
        let Some(dialog) = self.pane_dialog.as_mut() else {
            return;
        };
        let max = dialog
            .content
            .total_lines()
            .saturating_sub(dialog.viewport.max(1) as usize);
        dialog.scroll = scroll.min(max);
    }

    fn scroll_pane_dialog_horizontal(&mut self, columns: i32) {
        let Some(dialog) = self.pane_dialog.as_mut() else {
            return;
        };
        let max = dialog
            .content
            .max_width()
            .saturating_sub(dialog.viewport_width.max(1) as usize);
        dialog.h_scroll = (dialog.h_scroll as i64 + columns as i64).clamp(0, max as i64) as usize;
    }

    fn draw_pane_dialog(&mut self, frame: &mut Frame, area: Rect) {
        let Helper {
            pane_dialog,
            tc_list,
            ..
        } = self;
        let Some(dialog) = pane_dialog.as_mut() else {
            return;
        };

        let width = area.width.saturating_sub(8).max(20).min(area.width);
        let height = area.height.saturating_sub(5).max(8).min(area.height);
        let sheet = ui::sheet(frame, area, &dialog.title, dialog.color, width, height);
        if sheet.body.height == 0 || sheet.body.width == 0 {
            return;
        }

        if let DialogContent::Paged {
            case_index,
            version,
            line_count,
            page_start,
            warning,
            ..
        } = &mut dialog.content
            && let Some(tc) = tc_list.testcases.get(*case_index)
            && tc.got_version() != *version
        {
            *version = tc.got_version();
            *line_count = tc.got_line_count();
            *warning = tc.got_is_paged();
            *page_start = usize::MAX;
        }

        let total_lines = dialog.content.total_lines();
        dialog.scroll = dialog
            .scroll
            .min(total_lines.saturating_sub(sheet.body.height as usize));
        sync_paged_page(&mut dialog.content, tc_list, dialog.scroll);

        let content_width = dialog.content.max_width();
        let mut vscroll = total_lines > sheet.body.height as usize;
        let mut usable_width = sheet.body.width.saturating_sub(u16::from(vscroll));
        let mut hscroll = content_width > usable_width as usize;
        let mut content_height = sheet.body.height.saturating_sub(u16::from(hscroll));
        vscroll = total_lines > content_height as usize;
        usable_width = sheet.body.width.saturating_sub(u16::from(vscroll));
        if !hscroll && content_width > usable_width as usize {
            hscroll = true;
            content_height = sheet.body.height.saturating_sub(1);
            vscroll = total_lines > content_height as usize;
            usable_width = sheet.body.width.saturating_sub(u16::from(vscroll));
        }
        dialog.viewport = content_height.max(1);
        dialog.viewport_width = usable_width.max(1);
        dialog.h_scroll = dialog
            .h_scroll
            .min(content_width.saturating_sub(usable_width as usize));

        let max_scroll = total_lines.saturating_sub(dialog.viewport as usize);
        dialog.scroll = dialog.scroll.min(max_scroll);

        let scroll = dialog.scroll;
        let h_scroll = dialog.h_scroll;
        sync_paged_page(&mut dialog.content, tc_list, scroll);
        let (display_lines, local_scroll) = match &dialog.content {
            DialogContent::Lines(lines) => (lines.clone(), scroll),
            DialogContent::Paged {
                page_start,
                page,
                warning,
                ..
            } => {
                let warning_rows = usize::from(*warning);
                let mut display = Vec::new();
                if *warning && *page_start == 0 {
                    display.push(paged_warning_line(PAGED_OUTPUT_NOTE));
                }
                display.extend(page.iter().cloned());
                let base = if *page_start == 0 {
                    0
                } else {
                    warning_rows + *page_start
                };
                (display, scroll.saturating_sub(base))
            }
        };

        let content_area = Rect {
            width: usable_width,
            height: content_height,
            ..sheet.body
        };
        frame.render_widget(
            Paragraph::new(Text::from(display_lines))
                .style(Style::new().fg(theme::TEXT()).bg(theme::SURFACE_ALT()))
                .scroll((
                    local_scroll.min(u16::MAX as usize) as u16,
                    h_scroll.min(u16::MAX as usize) as u16,
                )),
            content_area,
        );

        dialog.v_scroll_area = Rect::default();
        if vscroll {
            let mut state =
                ScrollbarState::new(total_lines - dialog.viewport as usize).position(dialog.scroll);
            let scroll_area = Rect {
                x: sheet.body.right().saturating_sub(1),
                y: content_area.y,
                width: 1,
                height: content_height.max(1),
            };
            dialog.v_scroll_area = scroll_area;
            frame.render_stateful_widget(
                scrollbar(ScrollbarOrientation::VerticalRight),
                scroll_area,
                &mut state,
            );
        } else {
            dialog.v_scroll_drag = None;
        }

        dialog.h_scroll_area = Rect::default();
        if hscroll {
            let max_h_scroll = content_width.saturating_sub(usable_width as usize);
            let mut state = ScrollbarState::new(max_h_scroll).position(dialog.h_scroll);
            let scroll_area = Rect {
                x: sheet.body.x,
                y: sheet.body.y + content_height,
                width: usable_width,
                height: 1,
            };
            dialog.h_scroll_area = scroll_area;
            frame.render_stateful_widget(
                scrollbar(ScrollbarOrientation::HorizontalBottom),
                scroll_area,
                &mut state,
            );
        } else {
            dialog.h_scroll_drag = None;
        }
    }

    // ------------------------------------------------------------------
    // Event handling
    // ------------------------------------------------------------------

    pub fn handle_event(&mut self, event: Event) -> Option<PassingCommand> {
        self.dirty = true;
        if self.pane_dialog.is_some() {
            self.handle_pane_dialog_event(event);
            return None;
        }
        if self.show_help {
            if let Event::Key(key) = &event
                && matches!(key.code, KeyCode::Char('h') | KeyCode::F(1) | KeyCode::Esc)
            {
                self.show_help = false;
            }
            return None;
        }
        if let Event::Key(key) = &event
            && key.code == KeyCode::F(1)
            && matches!(self.input_mode, InputMode::Normal)
        {
            self.show_help = true;
            return None;
        }
        match event {
            Event::Key(key) => match self.input_mode {
                InputMode::Editing => {
                    if key.code == KeyCode::Esc {
                        self.finish_editing();
                        return None;
                    }
                    self.tc_list.handle_key(Event::Key(key), self.adding_case);
                    self.tc_list.update_scroll();
                }
                InputMode::Normal => match key.code {
                    KeyCode::Esc if self.running => self.cancel_run(),
                    KeyCode::Char('q') => return Some(ToHelper(HelperCommand::Quit)),
                    KeyCode::Char('a') => {
                        self.cmd_add();
                    }
                    KeyCode::Char('x') => self.cmd_delete(),
                    KeyCode::Char('r') => return self.cmd_run(),
                    KeyCode::Char('e') => {
                        self.cmd_edit();
                    }
                    KeyCode::Char('s') => return self.cmd_submit(),
                    KeyCode::Char('d') => {
                        self.dir_terminal.clear_dir_output();
                        self.input_mode = InputMode::Directory;
                    }
                    KeyCode::Char('c') => {
                        let (config, _) = self.fs_ops.load_config();
                        self.config_menu = ConfigMenu::new(&config);
                        self.input_mode = InputMode::Config;
                    }
                    KeyCode::Char('h') => self.show_help = true,
                    _ => {}
                },
                InputMode::DeleteSelect => match key.code {
                    KeyCode::BackTab => self.cycle_delete_selection(true),
                    KeyCode::Tab => {
                        let backwards = key.modifiers.contains(KeyModifiers::SHIFT);
                        self.cycle_delete_selection(backwards);
                    }
                    KeyCode::Enter => {
                        self.input_mode = InputMode::ConfirmDelete;
                    }
                    KeyCode::Esc => {
                        self.input_mode = InputMode::Normal;
                    }
                    _ => {}
                },
                InputMode::ConfirmDelete => match key.code {
                    KeyCode::Enter => self.delete_selected_case(),
                    KeyCode::Backspace | KeyCode::Esc => {
                        self.input_mode = InputMode::DeleteSelect;
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
                        if saved {
                            self.dir_terminal.set_result(
                                theme::GREEN(),
                                format!("config saved · language {}", config.language.label()),
                            );
                        } else {
                            self.dir_terminal
                                .set_error(theme::RED(), "failed to save config");
                        }
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
                                    pending.info.platform,
                                    pending.info.problem_code,
                                    pending.info.language
                                ),
                                color: theme::MUTED(),
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
                    }
                    _ => {}
                },
            },
            Event::Mouse(mouse) if matches!(self.input_mode, InputMode::Config) => {
                self.config_menu.handle_mouse(&mouse);
            }
            Event::Mouse(mouse) => {
                let position = Position::new(mouse.column, mouse.row);
                if self.submissions_scrollbar_area.contains(position) {
                    match mouse.kind {
                        MouseEventKind::Down(MouseButton::Left) => {
                            self.begin_submissions_scrollbar_drag(position);
                            return None;
                        }
                        MouseEventKind::Drag(MouseButton::Left) => {
                            if let Some(drag) = self.submissions_scrollbar_drag {
                                self.submission_scroll = drag.scroll_at(position);
                            }
                            return None;
                        }
                        MouseEventKind::Up(MouseButton::Left) => {
                            self.submissions_scrollbar_drag = None;
                            return None;
                        }
                        _ => {}
                    }
                }

                if matches!(mouse.kind, MouseEventKind::Up(MouseButton::Left)) {
                    self.submissions_scrollbar_drag = None;
                }
                if matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) {
                    self.submissions_scrollbar_drag = None;
                }

                if matches!(self.input_mode, InputMode::Normal | InputMode::Editing)
                    && let MouseEventKind::Down(MouseButton::Left) = mouse.kind
                {
                    let hit = self.tc_list.pane_at(mouse.column, mouse.row);
                    let double = hit.is_some_and(|(index, pane)| {
                        self.last_pane_click
                            .is_some_and(|(at, last_index, last_pane)| {
                                last_index == index
                                    && last_pane == pane
                                    && at.elapsed() < Duration::from_millis(450)
                            })
                    });
                    self.last_pane_click = hit.map(|(index, pane)| (Instant::now(), index, pane));
                    if double && let Some((index, pane)) = hit {
                        self.open_pane_dialog(index, pane);
                        return None;
                    }
                }

                let over_submissions = self
                    .submissions_area
                    .contains(Position::new(mouse.column, mouse.row));
                match mouse.kind {
                    MouseEventKind::ScrollDown if over_submissions => self.scroll_submissions(true),
                    MouseEventKind::ScrollUp if over_submissions => self.scroll_submissions(false),
                    MouseEventKind::ScrollDown => {
                        let editing = matches!(self.input_mode, InputMode::Editing);
                        if !editing || !self.tc_list.scroll_active_pane(&mouse, true) {
                            self.tc_list.handle_mouse(&mouse);
                        }
                    }
                    MouseEventKind::ScrollUp => {
                        let editing = matches!(self.input_mode, InputMode::Editing);
                        if !editing || !self.tc_list.scroll_active_pane(&mouse, false) {
                            self.tc_list.handle_mouse(&mouse);
                        }
                    }
                    _ => self.tc_list.handle_mouse(&mouse),
                }
            }
            _ => {}
        }
        None
    }
}

/// Compare trimmed output without building a second copy of a huge result.
fn normalized_eq(left: &str, right: &str) -> bool {
    let mut left = left.trim().lines().map(str::trim_end);
    let mut right = right.trim().lines().map(str::trim_end);
    loop {
        match (left.next(), right.next()) {
            (None, None) => return true,
            (Some(left), Some(right)) if left == right => {}
            _ => return false,
        }
    }
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
    use crate::core::{Language, ReceivingTestCase, UserConfig};
    use ratatui::{
        Terminal,
        backend::TestBackend,
        crossterm::event::{KeyEvent, KeyModifiers, MouseEvent, MouseEventKind},
        style::Modifier,
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
                ReceivingTestCase {
                    input: "1 2".into(),
                    expected_output: "3".into(),
                },
                ReceivingTestCase {
                    input: "5".into(),
                    expected_output: "10".into(),
                },
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
        helper.run_finished(helper.run_id, 0);
    }

    #[test]
    fn same_name_problems_keep_separate_state() {
        let root = temp_root("identity");
        let fs_ops = FsOps::with_root(root.clone());
        let work = root.join("problems");
        std::fs::create_dir_all(&work).unwrap();
        let mut helper = Helper::new(work, &fs_ops);

        let mut first = sample_problem();
        first.name = "A. Shared".into();
        first.url = "https://codeforces.com/contest/1111/problem/A".into();
        let first_path = helper.new_problem(first).unwrap();

        let mut second = sample_problem();
        second.name = "A. Shared".into();
        second.url = "https://codeforces.com/contest/2222/problem/A".into();
        second.test_cases = vec![ReceivingTestCase {
            input: "9".into(),
            expected_output: "9".into(),
        }];
        let second_path = helper.new_problem(second).unwrap();

        assert_eq!(
            first_path.file_stem().unwrap().to_string_lossy(),
            "A__Shared"
        );
        let second_stem = second_path
            .file_stem()
            .unwrap()
            .to_string_lossy()
            .to_string();
        assert!(second_stem.starts_with("A__Shared__"), "{second_stem}");
        assert_ne!(first_path, second_path);

        let first_saved = fs_ops.load_problem_by_stem("A__Shared").unwrap();
        assert_eq!(first_saved.test_cases[0].input, "1 2");
        let second_saved = fs_ops.load_problem_by_stem(&second_stem).unwrap();
        assert_eq!(second_saved.test_cases[0].input, "9");
        assert_eq!(
            second_saved.url,
            "https://codeforces.com/contest/2222/problem/A"
        );
        assert_ne!(
            fs_ops.binary_dir.join(&first_saved.name),
            fs_ops.binary_dir.join(&second_saved.name)
        );

        // Re-receiving the same URL must reuse its suffix, not add another one.
        let mut again = sample_problem();
        again.name = "A. Shared".into();
        again.url = "https://codeforces.com/contest/2222/problem/A".into();
        assert_eq!(helper.new_problem(again).unwrap(), second_path);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn anonymous_problems_with_the_same_name_do_not_overwrite() {
        let root = temp_root("anonymous-identity");
        let fs_ops = FsOps::with_root(root.clone());
        let work = root.join("problems");
        std::fs::create_dir_all(&work).unwrap();
        let mut helper = Helper::new(work, &fs_ops);

        let mut first = sample_problem();
        first.name = "A. Anonymous".into();
        first.url.clear();
        let first_path = helper.new_problem(first).unwrap();

        let mut second = sample_problem();
        second.name = "A. Anonymous".into();
        second.url.clear();
        second.test_cases = vec![ReceivingTestCase {
            input: "9".into(),
            expected_output: "9".into(),
        }];
        let second_path = helper.new_problem(second).unwrap();

        assert_eq!(
            first_path.file_stem().unwrap().to_string_lossy(),
            "A__Anonymous"
        );
        assert_eq!(
            second_path.file_stem().unwrap().to_string_lossy(),
            "A__Anonymous__2"
        );
        assert_eq!(
            fs_ops
                .load_problem_by_stem("A__Anonymous")
                .unwrap()
                .test_cases[0]
                .input,
            "1 2"
        );
        assert_eq!(
            fs_ops
                .load_problem_by_stem("A__Anonymous__2")
                .unwrap()
                .test_cases[0]
                .input,
            "9"
        );

        let _ = std::fs::remove_dir_all(&root);
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
            Verdict::Success {
                output: ok_output("3\n"),
                time: 4,
            },
            Verdict::Success {
                output: ok_output("0\n"),
                time: 9,
            },
        ];
        // Results are applied one at a time, exactly as they arrive from the runner.
        helper.apply_case_result(helper.run_id, 0, results[0].clone());
        assert!(matches!(
            helper.tc_list.testcases[0].verdict,
            CaseVerdict::AC { ms: 4 }
        ));
        assert!(matches!(
            helper.tc_list.testcases[1].verdict,
            CaseVerdict::Pending
        ));

        helper.apply_case_result(helper.run_id, 1, results[1].clone());
        helper.run_finished(helper.run_id, 0);
        terminal.draw(|f| helper.draw(f)).unwrap();

        assert!(matches!(
            helper.tc_list.testcases[1].verdict,
            CaseVerdict::WA { ms: 9 }
        ));
        assert!(helper.tc_list.testcases[0].collapsed);
        assert!(!helper.running);

        let failures = vec![
            Verdict::TimeLimitExceeded,
            Verdict::CompilationError {
                error: "boom".into(),
            },
        ];
        stream(&mut helper, failures);
        terminal.draw(|f| helper.draw(f)).unwrap();
        assert!(matches!(
            helper.tc_list.testcases[0].verdict,
            CaseVerdict::Tle
        ));
        assert!(matches!(
            helper.tc_list.testcases[1].verdict,
            CaseVerdict::CE
        ));

        dump_screen(&terminal);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn cancelling_a_modal_does_not_echo_into_the_console() {
        let root = temp_root("cancel_quiet");
        let fs_ops = FsOps::with_root(root.clone());
        let work = root.join("problems");
        std::fs::create_dir_all(&work).unwrap();

        let mut helper = Helper::new(work, &fs_ops);
        helper.new_problem(sample_problem());
        helper
            .dir_terminal
            .set_result(theme::GREEN(), "✔ ALL 2/2 PASSED");

        helper.handle_event(key(KeyCode::Char('x')));
        assert!(matches!(helper.input_mode, InputMode::DeleteSelect));
        helper.handle_event(key(KeyCode::Esc));
        assert!(matches!(helper.input_mode, InputMode::Normal));
        assert_eq!(
            console_text(&helper),
            "✔ ALL 2/2 PASSED",
            "cancelling a delete keeps the last result"
        );

        helper.handle_event(key(KeyCode::Char('s')));
        assert!(matches!(helper.input_mode, InputMode::ConfirmSubmit));
        helper.handle_event(key(KeyCode::Backspace));
        assert!(matches!(helper.input_mode, InputMode::Normal));
        assert_eq!(
            console_text(&helper),
            "✔ ALL 2/2 PASSED",
            "cancelling a submission keeps the last result"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn attaching_a_problem_clears_the_previous_console_result() {
        let root = temp_root("attach_console");
        let fs_ops = FsOps::with_root(root.clone());
        let work = root.join("problems");
        std::fs::create_dir_all(&work).unwrap();

        let mut helper = Helper::new(work, &fs_ops);
        helper.new_problem(sample_problem());
        helper
            .dir_terminal
            .set_result(theme::GREEN(), "✔ ALL 2/2 PASSED");
        assert!(helper.dir_terminal.console_line(120).is_some());

        let mut next = sample_problem();
        next.name = "Another Problem".into();
        next.url = "https://codeforces.com/contest/1/problem/A".into();
        helper.new_problem(next);
        assert!(
            helper.dir_terminal.console_line(120).is_none(),
            "the previous problem's result must not leak into the new one"
        );

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

    fn console_text(helper: &Helper) -> String {
        helper
            .dir_terminal
            .console_line(120)
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .unwrap_or_default()
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
    fn adding_a_case_keeps_tab_inside_that_case() {
        let root = temp_root("add_stay");
        let fs_ops = FsOps::with_root(root.clone());
        let work = root.join("problems");
        std::fs::create_dir_all(&work).unwrap();

        let mut helper = Helper::new(work, &fs_ops);
        helper.new_problem(sample_problem());
        helper.handle_event(key(KeyCode::Char('a')));
        let new_case = helper.tc_list.active_area;
        assert_eq!(new_case, 2);
        assert_eq!(
            helper.tc_list.active_box,
            crate::components::testcase::ActiveBox::Input
        );

        helper.handle_event(key(KeyCode::Tab));
        assert_eq!(helper.tc_list.active_area, new_case);
        assert_eq!(
            helper.tc_list.active_box,
            crate::components::testcase::ActiveBox::Expected
        );

        helper.handle_event(key(KeyCode::Tab));
        assert_eq!(helper.tc_list.active_area, new_case);
        assert_eq!(
            helper.tc_list.active_box,
            crate::components::testcase::ActiveBox::Input
        );

        helper.handle_event(Event::Key(KeyEvent::new(
            KeyCode::BackTab,
            KeyModifiers::SHIFT,
        )));
        assert_eq!(helper.tc_list.active_area, new_case);
        assert_eq!(
            helper.tc_list.active_box,
            crate::components::testcase::ActiveBox::Expected
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn edit_mode_still_walks_cases() {
        let root = temp_root("edit_walk");
        let fs_ops = FsOps::with_root(root.clone());
        let work = root.join("problems");
        std::fs::create_dir_all(&work).unwrap();

        let mut helper = Helper::new(work, &fs_ops);
        helper.new_problem(sample_problem());
        helper.handle_event(key(KeyCode::Char('e')));
        assert_eq!(helper.tc_list.active_area, 0);

        helper.handle_event(key(KeyCode::Tab));
        assert_eq!(helper.tc_list.active_area, 0);
        helper.handle_event(key(KeyCode::Tab));
        assert_eq!(helper.tc_list.active_area, 1);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn delete_selects_and_asks_for_confirmation() {
        let root = temp_root("delete");
        let fs_ops = FsOps::with_root(root.clone());
        let work = root.join("problems");
        std::fs::create_dir_all(&work).unwrap();

        let mut helper = Helper::new(work, &fs_ops);
        helper.new_problem(sample_problem());
        helper.running = true;

        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();

        helper.tc_list.testcases[0].collapsed = true;
        helper.tc_list.testcases[1].collapsed = true;
        helper.tc_list.active_area = 0;
        helper.handle_event(key(KeyCode::Char('x')));
        assert!(matches!(helper.input_mode, InputMode::DeleteSelect));
        assert_eq!(helper.tc_list.testcases.len(), 2, "x only starts the flow");
        assert!(
            helper.tc_list.testcases.iter().all(|tc| !tc.collapsed),
            "delete selection must show every case open"
        );
        assert!(
            !helper.running,
            "an in-flight run is retired before the list shifts"
        );
        terminal.draw(|f| helper.draw(f)).unwrap();
        dump_screen(&terminal);

        // Tab picks the next case; the selection wraps in both directions.
        helper.handle_event(key(KeyCode::Tab));
        assert_eq!(helper.tc_list.active_area, 1);
        helper.handle_event(key(KeyCode::Tab));
        assert_eq!(helper.tc_list.active_area, 0, "selection wraps around");
        helper.handle_event(key(KeyCode::BackTab));
        assert_eq!(helper.tc_list.active_area, 1);

        // Enter opens the confirmation; Esc there goes back to the selection.
        helper.handle_event(key(KeyCode::Enter));
        assert!(matches!(helper.input_mode, InputMode::ConfirmDelete));
        assert_eq!(
            helper.tc_list.testcases.len(),
            2,
            "confirmation must not delete yet"
        );
        terminal.draw(|f| helper.draw(f)).unwrap();
        dump_screen(&terminal);
        helper.handle_event(key(KeyCode::Esc));
        assert!(matches!(helper.input_mode, InputMode::DeleteSelect));

        // Esc leaves the flow entirely; nothing is deleted.
        helper.handle_event(key(KeyCode::Esc));
        assert!(matches!(helper.input_mode, InputMode::Normal));
        assert_eq!(helper.tc_list.testcases.len(), 2);

        // x -> Enter -> Enter deletes the selected case and persists the rest.
        helper.handle_event(key(KeyCode::Char('x')));
        helper.handle_event(key(KeyCode::Enter));
        helper.handle_event(key(KeyCode::Enter));
        assert!(matches!(helper.input_mode, InputMode::Normal));
        assert_eq!(helper.tc_list.testcases.len(), 1);
        assert_eq!(helper.tc_list.active_area, 0);
        let saved = fs_ops.load_problem_by_stem("E_Prime_Destruction").unwrap();
        assert_eq!(saved.test_cases.len(), 1);
        assert_eq!(saved.test_cases[0].input, "1 2");

        // Deleting the last case leaves an empty list without panicking.
        helper.handle_event(key(KeyCode::Char('x')));
        helper.handle_event(key(KeyCode::Enter));
        helper.handle_event(key(KeyCode::Enter));
        assert!(helper.tc_list.testcases.is_empty());
        let saved = fs_ops.load_problem_by_stem("E_Prime_Destruction").unwrap();
        assert!(saved.test_cases.is_empty());

        // With an empty list x just reports it and stays in Normal mode.
        helper.handle_event(key(KeyCode::Char('x')));
        assert!(matches!(helper.input_mode, InputMode::Normal));

        // In Editing mode `x` is a character, not the delete command.
        helper.handle_event(key(KeyCode::Char('a')));
        helper.handle_event(key(KeyCode::Char('x')));
        assert_eq!(helper.tc_list.testcases.len(), 1);
        helper.handle_event(key(KeyCode::Esc));
        assert_eq!(helper.tc_list.testcases.len(), 1);
        assert_eq!(
            helper.tc_list.testcases[0].inp_content.lines().join("\n"),
            "x"
        );

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

        helper.handle_event(key(KeyCode::Tab));
        type_text(&mut helper, "-O2");
        helper.handle_event(key(KeyCode::Enter));
        helper.handle_event(key(KeyCode::Tab));
        helper.handle_event(key(KeyCode::Right));
        assert_eq!(helper.config_menu.language, Language::Python);
        assert!(helper.config_menu.flags.is_empty());

        helper.handle_event(key(KeyCode::Tab));
        type_text(&mut helper, "-u");
        helper.handle_event(key(KeyCode::Enter));
        terminal.draw(|f| helper.draw(f)).unwrap();
        dump_screen(&terminal);

        helper.handle_event(key(KeyCode::Esc));
        assert!(matches!(helper.input_mode, InputMode::Normal));

        let (config, loaded) = fs_ops.load_config();
        assert!(loaded);
        assert_eq!(config.language, Language::Python);
        assert_eq!(config.extra_flags, ["-u"]);
        assert_eq!(config.flags_by_language[&Language::Cpp], ["-O2"]);
        assert_eq!(config.flags_by_language[&Language::Python], ["-u"]);

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
        assert_eq!(
            pending.info.submit_url,
            "https://codeforces.com/contest/2266/submit"
        );
        assert_eq!(pending.info.language_id, Language::Cpp.cf_id(&[]));
        assert_eq!(pending.test_count, 2);
        terminal.draw(|f| helper.draw(f)).unwrap();
        dump_screen(&terminal);

        helper.handle_event(key(KeyCode::Char('x')));
        assert!(
            helper.pending_submit.is_some(),
            "other keys must not dismiss the modal"
        );

        helper.handle_event(key(KeyCode::Backspace));
        assert!(matches!(helper.input_mode, InputMode::Normal));
        assert!(helper.pending_submit.is_none());

        helper.handle_event(key(KeyCode::Char('s')));
        let command = helper.handle_event(key(KeyCode::Enter));
        assert!(matches!(
            command,
            Some(PassingCommand::ToSubmitter(SubmitterCommand::SubmitCode {
                id: 1,
                ..
            }))
        ));
        assert_eq!(helper.submissions.len(), 1);
        assert!(!helper.submissions[0].done);
        assert!(matches!(helper.input_mode, InputMode::Normal));
        assert!(helper.pending_submit.is_none());

        helper.submit_status(1, "judging on Codeforces…".into());
        assert_eq!(helper.submissions[0].message, "judging on Codeforces…");
        assert_eq!(helper.submissions[0].color, theme::PURPLE());
        helper.submit_finished(1, "RESULT: ACCEPTED".into());
        assert!(helper.submissions[0].done);
        assert_eq!(helper.submissions[0].color, theme::GREEN());

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
                Some(PassingCommand::ToSubmitter(
                    SubmitterCommand::SubmitCode { .. }
                ))
            ));
        }
        assert_eq!(helper.submissions.len(), 2);
        assert_eq!(helper.submissions[0].id, 1);
        assert_eq!(helper.submissions[1].id, 2);

        helper.submit_status(2, "judging…".into());
        assert_eq!(helper.submissions[1].message, "judging…");
        assert_eq!(helper.submissions[0].color, theme::MUTED());

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
        assert_eq!(
            helper.submissions.last().unwrap().id,
            2 + MAX_SUBMISSIONS as u64
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn path_bar_focuses_the_active_working_directory() {
        let root = temp_root("active_dir_focus");
        let fs_ops = FsOps::with_root(root.clone());
        let work = root.join("problems");
        std::fs::create_dir_all(&work).unwrap();

        let mut helper = Helper::new(work, &fs_ops);
        helper.new_problem(sample_problem());
        helper.handle_event(key(KeyCode::Char('d')));
        assert!(matches!(helper.input_mode, InputMode::Directory));

        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
        terminal.draw(|f| helper.draw(f)).unwrap();
        dump_screen(&terminal);
        let buffer = terminal.backend().buffer();
        assert_eq!(
            buffer[(0, 0)].bg,
            theme::SURFACE(),
            "the app bar owns the first row"
        );
        assert_eq!(
            buffer[(2, 2)].fg,
            theme::ACCENT(),
            "the active command bar is focused in accent blue"
        );
        assert_eq!(
            buffer[(3, 2)].bg,
            theme::SURFACE_ALT(),
            "the focused command bar is a raised surface"
        );
        let console_bar = (0..buffer.area.width)
            .map(|x| buffer[(x, buffer.area.height - 1)].symbol())
            .collect::<String>();
        assert!(
            console_bar.contains("CONSOLE"),
            "the console bar names the console mode"
        );
        assert!(
            !console_bar.contains("DIRECTORY"),
            "the UI labels this mode as the console"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn console_bar_shows_the_mode_badge_and_mode_keys() {
        let root = temp_root("footer_keys");
        let fs_ops = FsOps::with_root(root.clone());
        let work = root.join("problems");
        std::fs::create_dir_all(&work).unwrap();

        let mut helper = Helper::new(work, &fs_ops);
        let mut terminal = Terminal::new(TestBackend::new(120, 24)).unwrap();
        terminal.draw(|f| helper.draw(f)).unwrap();
        dump_screen(&terminal);
        let buffer = terminal.backend().buffer();
        let last = buffer.area.height - 1;
        let console_bar = (0..buffer.area.width)
            .map(|x| buffer[(x, last)].symbol())
            .collect::<String>();
        assert!(
            console_bar.contains("NORMAL"),
            "normal mode has a badge: {console_bar}"
        );
        assert!(
            !console_bar.contains("[a]") && !console_bar.contains("[q]"),
            "global keys live on the splash and help sheet: {console_bar}"
        );
        let x = console_bar.find("NORMAL").unwrap() as u16;
        assert_eq!(buffer[(x, last)].fg, Color::Black);
        assert_eq!(
            buffer[(x, last)].bg,
            theme::ACCENT(),
            "the mode badge stays filled"
        );

        helper.handle_event(key(KeyCode::Char('d')));
        terminal.draw(|f| helper.draw(f)).unwrap();
        let buffer = terminal.backend().buffer();
        let console_bar = (0..buffer.area.width)
            .map(|x| buffer[(x, last)].symbol())
            .collect::<String>();
        let x = console_bar.find("CONSOLE").unwrap() as u16;
        assert_eq!(buffer[(x, last)].fg, Color::Black);
        assert_eq!(
            buffer[(x, last)].bg,
            theme::CYAN(),
            "mode labels are filled badges"
        );
        let keys_row = (0..buffer.area.width)
            .map(|x| buffer[(x, last - 1)].symbol())
            .collect::<String>();
        assert!(
            console_bar.contains("[Tab]")
                && console_bar.contains("[Esc]")
                && console_bar.contains("complete")
                && console_bar.contains("close"),
            "the console mode shows its keys inside the bar: {console_bar}"
        );
        assert!(
            !keys_row.contains("[Tab]"),
            "no separate keys row remains: {keys_row}"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn config_and_submit_keys_live_in_the_console() {
        let root = temp_root("console_modal_keys");
        let fs_ops = FsOps::with_root(root.clone());
        let work = root.join("problems");
        std::fs::create_dir_all(&work).unwrap();

        let mut helper = Helper::new(work, &fs_ops);
        helper.new_problem(sample_problem());
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
        let console_row = |terminal: &Terminal<TestBackend>| {
            let buffer = terminal.backend().buffer();
            (0..buffer.area.width)
                .map(|x| buffer[(x, buffer.area.height - 1)].symbol())
                .collect::<String>()
        };

        helper.handle_event(key(KeyCode::Char('c')));
        assert!(matches!(helper.input_mode, InputMode::Config));
        terminal.draw(|f| helper.draw(f)).unwrap();
        dump_screen(&terminal);
        let config = console_row(&terminal);
        assert!(config.contains("CONFIG"), "{config}");
        for part in ["[Tab]", "field", "[←/→]", "language", "[Esc]", "close"] {
            assert!(
                config.contains(part),
                "the language field keys live in the console: {config}"
            );
        }
        assert_eq!(
            config.matches("[Tab]").count(),
            1,
            "the config sheet must not repeat the console keys: {config}"
        );

        // Moving to the flags field changes what the console teaches.
        helper.handle_event(key(KeyCode::Tab));
        terminal.draw(|f| helper.draw(f)).unwrap();
        let flags = console_row(&terminal);
        for part in ["[Enter]", "add", "[x]", "delete"] {
            assert!(
                flags.contains(part),
                "the flags field keys live in the console: {flags}"
            );
        }

        helper.handle_event(key(KeyCode::Esc));
        assert!(matches!(helper.input_mode, InputMode::Normal));
        assert!(helper.handle_event(key(KeyCode::Char('s'))).is_none());
        assert!(matches!(helper.input_mode, InputMode::ConfirmSubmit));
        terminal.draw(|f| helper.draw(f)).unwrap();
        dump_screen(&terminal);
        let submit = console_row(&terminal);
        assert!(submit.contains("SUBMIT"), "{submit}");
        for part in ["[Enter]", "submit", "[Backspace]", "cancel"] {
            assert!(submit.contains(part), "submit keys in console: {submit}");
        }

        helper.handle_event(key(KeyCode::Esc));
        assert!(matches!(helper.input_mode, InputMode::Normal));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn mode_keys_fit_inside_the_console_at_eighty_columns() {
        let root = temp_root("footer_80");
        let fs_ops = FsOps::with_root(root.clone());
        let work = root.join("problems");
        std::fs::create_dir_all(&work).unwrap();

        let mut helper = Helper::new(work, &fs_ops);
        helper.new_problem(sample_problem());
        helper.handle_event(key(KeyCode::Char('d')));
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal.draw(|f| helper.draw(f)).unwrap();
        let buffer = terminal.backend().buffer();
        let row = |y| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        };
        let console_bar = row(buffer.area.height - 1);
        assert!(console_bar.contains("CONSOLE"), "{console_bar}");
        assert!(
            console_bar.contains("[Tab]") && console_bar.contains("complete"),
            "the console keys explain themselves when there is room: {console_bar}"
        );
        assert!(
            console_bar.contains("[Esc]") && console_bar.contains("close"),
            "{console_bar}"
        );
        assert!(
            row(buffer.area.height - 2).chars().all(|c| c == '─'),
            "the console keeps the status hairline above it"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn terminal_resize_keeps_the_chrome_inside_the_viewport() {
        let root = temp_root("resize");
        let fs_ops = FsOps::with_root(root.clone());
        let work = root.join("problems");
        std::fs::create_dir_all(&work).unwrap();

        let mut helper = Helper::new(work, &fs_ops);
        helper.new_problem(sample_problem());

        for (width, height) in [(120, 40), (48, 14)] {
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal.draw(|f| helper.draw(f)).unwrap();
            dump_screen(&terminal);
            let buffer = terminal.backend().buffer();
            let row = |y| {
                (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            };

            assert!(row(0).contains("zch"), "app bar at {width}x{height}");
            assert!(
                row(3).chars().all(|c| c == '─'),
                "the header rule survives at {width}x{height}"
            );
            let console_bar = row(height - 1);
            assert!(
                console_bar.contains("NORMAL"),
                "console bar at {width}x{height}: {console_bar}"
            );
            assert!(
                !console_bar.contains("[q]") && !console_bar.contains("[h]"),
                "no persistent key footer at {width}x{height}: {console_bar}"
            );
        }

        let mut terminal = Terminal::new(TestBackend::new(40, 10)).unwrap();
        terminal.draw(|f| helper.draw(f)).unwrap();
        let screen = (0..10)
            .map(|y| {
                (0..40)
                    .map(|x| terminal.backend().buffer()[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        if std::env::var("ZCH_DUMP").is_ok() {
            println!("{screen}");
        }
        assert!(screen.contains("Terminal too small"), "{screen}");
        assert!(screen.contains("48 × 14"), "{screen}");
        assert!(screen.contains("40 × 10"), "{screen}");

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn h_opens_and_closes_help() {
        let root = temp_root("footer_help");
        let fs_ops = FsOps::with_root(root.clone());
        let work = root.join("problems");
        std::fs::create_dir_all(&work).unwrap();

        let mut helper = Helper::new(work, &fs_ops);
        let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
        helper.handle_event(key(KeyCode::Char('h')));
        assert!(helper.show_help);
        terminal.draw(|f| helper.draw(f)).unwrap();
        dump_screen(&terminal);
        let buffer = terminal.backend().buffer();
        let screen = (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(screen.contains("KEYS"), "{screen}");
        assert!(screen.contains("add case"), "{screen}");
        assert!(
            screen.contains("NORMAL"),
            "the console bar keeps the mode: {screen}"
        );

        helper.handle_event(key(KeyCode::Char('h')));
        assert!(!helper.show_help);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn help_only_opens_in_normal_mode() {
        let root = temp_root("footer_help_editing");
        let fs_ops = FsOps::with_root(root.clone());
        let work = root.join("problems");
        std::fs::create_dir_all(&work).unwrap();

        let mut helper = Helper::new(work, &fs_ops);
        helper.new_problem(sample_problem());
        helper.handle_event(key(KeyCode::Char('a')));
        assert!(matches!(helper.input_mode, InputMode::Editing));

        // Edit mode already explains itself in the console; F1 does not open
        // the global map on top of it.
        helper.handle_event(key(KeyCode::F(1)));
        assert!(!helper.show_help);

        let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
        terminal.draw(|f| helper.draw(f)).unwrap();
        dump_screen(&terminal);
        let buffer = terminal.backend().buffer();
        let screen = (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(screen.contains("EDIT 3/3"), "{screen}");
        assert!(
            screen.contains("[Tab/⇧Tab]") && screen.contains("[Esc]"),
            "edit-mode keys live in the console bar: {screen}"
        );

        helper.handle_event(key(KeyCode::Esc));
        assert!(matches!(helper.input_mode, InputMode::Normal));

        // Normal mode is where the global map lives, and its own close keys
        // are written in the console.
        helper.handle_event(key(KeyCode::F(1)));
        assert!(helper.show_help);
        terminal.draw(|f| helper.draw(f)).unwrap();
        let buffer = terminal.backend().buffer();
        let screen = (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(screen.contains("add case"), "{screen}");
        assert!(
            screen.contains("[h/F1/Esc]") && screen.contains("close"),
            "the help close key is in the console: {screen}"
        );

        helper.handle_event(key(KeyCode::Esc));
        assert!(!helper.show_help);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn console_bar_does_not_repeat_mode_hints() {
        let root = temp_root("console_mode");
        let fs_ops = FsOps::with_root(root.clone());
        let work = root.join("problems");
        std::fs::create_dir_all(&work).unwrap();

        let mut helper = Helper::new(work, &fs_ops);
        helper.new_problem(sample_problem());
        helper.cmd_add();
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
        terminal.draw(|f| helper.draw(f)).unwrap();

        let buffer = terminal.backend().buffer();
        let screen = (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            screen.contains("EDIT 3/3"),
            "the console bar still shows the mode"
        );
        assert!(
            !screen.contains("[F1]") && !screen.contains("[h]"),
            "global help stays in the splash/help sheet:\n{screen}"
        );
        assert!(
            screen.contains("[Tab/⇧Tab]")
                && screen.contains("pane")
                && screen.contains("[Esc]")
                && screen.contains("save"),
            "edit-mode keys live in the console bar:\n{screen}"
        );
        assert!(
            !screen.contains("switch field"),
            "the console must not repeat inline key hints:\n{screen}"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn console_bar_does_not_repeat_the_run_status() {
        let root = temp_root("footer_busy");
        let fs_ops = FsOps::with_root(root.clone());
        let work = root.join("problems");
        std::fs::create_dir_all(&work).unwrap();

        let mut helper = Helper::new(work, &fs_ops);
        helper.new_problem(sample_problem());
        helper.running = true;
        helper.dir_terminal.clear_result();
        helper.dir_terminal.set_live(theme::CYAN(), "running tests");
        let mut terminal = Terminal::new(TestBackend::new(120, 24)).unwrap();
        terminal.draw(|f| helper.draw(f)).unwrap();

        let buffer = terminal.backend().buffer();
        let console_bar = (0..buffer.area.width)
            .map(|x| buffer[(x, buffer.area.height - 1)].symbol())
            .collect::<String>();
        assert_eq!(
            console_bar.matches("running tests").count(),
            1,
            "the status is printed exactly once: {console_bar}"
        );
        assert!(console_bar.contains("NORMAL"), "{console_bar}");
        assert!(
            !console_bar.contains("[r]") && !console_bar.contains("[Esc]"),
            "the console bar carries the mode, not keys: {console_bar}"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn app_bar_has_the_brand_and_the_path_bar_shows_the_directory() {
        let root = temp_root("sticky_header");
        let fs_ops = FsOps::with_root(root.clone());
        let work = root.join("problems");
        std::fs::create_dir_all(&work).unwrap();

        let mut helper = Helper::new(work, &fs_ops);
        helper.new_problem(sample_problem());
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
        terminal.draw(|f| helper.draw(f)).unwrap();
        dump_screen(&terminal);
        let buffer = terminal.backend().buffer();
        let header = (0..buffer.area.width)
            .map(|x| buffer[(x, 0)].symbol())
            .collect::<String>();
        assert!(
            header.contains("zch") && header.contains(env!("CARGO_PKG_VERSION")),
            "the app bar carries the brand and version: {header}"
        );
        assert!(
            header.contains(&helper.active_problem.as_ref().unwrap().name),
            "the active problem is visible: {header}"
        );
        assert!(
            !header.contains("LISTENING")
                && !header.contains(&format!(":{}", crate::core::receiver::listen_port())),
            "the app bar keeps only the brand, version and problem: {header}"
        );
        assert_eq!(buffer[(2, 0)].fg, theme::ACCENT(), "the brand mark");
        let brand_x = header[..header.find("zch").unwrap()].chars().count() as u16;
        assert_eq!(buffer[(brand_x, 0)].fg, theme::TEXT());
        let problem = helper.active_problem.as_ref().unwrap().name.clone();
        let problem_x = header[..header.find(&problem).unwrap()].chars().count();
        assert!(
            problem_x > buffer.area.width as usize / 2,
            "the problem name is pinned right, not beside the brand: {header}"
        );
        assert!(
            buffer[(problem_x as u16, 0)]
                .modifier
                .contains(Modifier::BOLD),
            "the problem name is bold"
        );
        let dot_x = header[..header.find('●').unwrap()].chars().count() as u16;
        assert_eq!(
            buffer[(dot_x, 0)].fg,
            theme::GREEN(),
            "the right-aligned problem is marked with a green dot"
        );
        assert!(
            (0..buffer.area.width).all(|x| buffer[(x, 1)].symbol() == "─"),
            "a hairline separates the app bar from the path bar"
        );

        let row = (0..buffer.area.width)
            .map(|x| buffer[(x, 2)].symbol())
            .collect::<String>();
        assert!(
            row.contains("problems"),
            "the path bar shows the working directory: {row}"
        );
        assert!(
            !row.contains("Working Directory"),
            "the path is the title now: {row}"
        );
        let path_x = row.find("problems").unwrap() as u16;
        assert_eq!(buffer[(path_x, 2)].fg, theme::MUTED());
        assert_eq!(
            buffer[(3, 2)].bg,
            theme::BG(),
            "the idle path bar sits on the canvas"
        );

        helper.handle_event(key(KeyCode::Char('d')));
        terminal.draw(|f| helper.draw(f)).unwrap();
        let buffer = terminal.backend().buffer();
        let row = (0..buffer.area.width)
            .map(|x| buffer[(x, 2)].symbol())
            .collect::<String>();
        assert!(
            row.contains("problems"),
            "the active command bar keeps the path visible: {row}"
        );
        assert_eq!(buffer[(2, 2)].fg, theme::ACCENT());
        assert_eq!(buffer[(3, 2)].bg, theme::SURFACE_ALT());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn canvas_surfaces_replace_the_old_boxed_sections() {
        let root = temp_root("sticky_bg");
        let fs_ops = FsOps::with_root(root.clone());
        let work = root.join("problems");
        std::fs::create_dir_all(&work).unwrap();

        let mut helper = Helper::new(work, &fs_ops);
        helper.new_problem(sample_problem());
        helper.handle_event(key(KeyCode::Char('s')));
        helper.handle_event(key(KeyCode::Enter));
        helper
            .dir_terminal
            .set_result(theme::PURPLE(), "submission queued");

        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
        terminal.draw(|f| helper.draw(f)).unwrap();
        let buffer = terminal.backend().buffer();

        assert_eq!(
            buffer[(0, 0)].bg,
            theme::SURFACE(),
            "the app bar is a raised surface"
        );
        assert_eq!(
            buffer[(3, 2)].bg,
            theme::BG(),
            "the idle path bar has no raised fill"
        );
        assert!(
            (0..buffer.area.width).all(|x| buffer[(x, 1)].symbol() == "─"),
            "a hairline separates the app bar from the path bar"
        );

        let submissions = helper.submissions_area;
        assert_eq!(
            buffer[(submissions.x, submissions.y)].bg,
            theme::SURFACE(),
            "submissions have their own surface"
        );
        let header = (0..buffer.area.width)
            .map(|x| buffer[(x, submissions.y)].symbol())
            .collect::<String>();
        assert!(header.contains("SUBMISSIONS"), "{header}");
        assert!(
            header.contains("1 result"),
            "the header carries the count: {header}"
        );

        let console_y = buffer.area.height - 1;
        assert_eq!(buffer[(2, console_y)].fg, theme::PURPLE());
        assert_eq!(
            buffer[(2, console_y)].bg,
            theme::SURFACE(),
            "the console bar is the single bottom surface"
        );
        let console_row = (0..buffer.area.width)
            .map(|x| buffer[(x, console_y)].symbol())
            .collect::<String>();
        assert!(console_row.contains("submission queued"), "{console_row}");
        assert!(console_row.contains("NORMAL"), "{console_row}");

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn hairlines_separate_the_chrome_from_the_content() {
        let root = temp_root("chrome_hairline");
        let fs_ops = FsOps::with_root(root.clone());
        let work = root.join("problems");
        std::fs::create_dir_all(&work).unwrap();

        let mut helper = Helper::new(work, &fs_ops);
        helper.new_problem(sample_problem());
        let mut terminal = Terminal::new(TestBackend::new(80, 30)).unwrap();
        terminal.draw(|f| helper.draw(f)).unwrap();
        let buffer = terminal.backend().buffer();
        let row = |y| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        };
        // Identity bar, hairline, path bar, then a full-width rule.
        assert!(row(1).chars().all(|c| c == '─'), "brand rule: {}", row(1));
        assert!(row(3).chars().all(|c| c == '─'), "top rule: {}", row(3));
        let console_y = buffer.area.height - 1;
        let bottom_rule = console_y - 1;
        assert!(
            row(bottom_rule).chars().all(|c| c == '─'),
            "bottom rule: {}",
            row(bottom_rule)
        );
        assert!(
            !row(4).contains("↑ more"),
            "the hairline replaces the overflow marker"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn test_case_list_reserves_a_scroll_viewport() {
        let root = temp_root("list_edge");
        let fs_ops = FsOps::with_root(root.clone());
        let work = root.join("problems");
        std::fs::create_dir_all(&work).unwrap();

        let mut helper = Helper::new(work, &fs_ops);
        helper.new_problem(sample_problem());
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
        terminal.draw(|f| helper.draw(f)).unwrap();

        assert_eq!(helper.tc_list.testcases[0].area.rect.x, 2);
        assert_eq!(
            helper.tc_list.testcases[0].area.rect.y, 5,
            "the first case sits under the rule and its quiet row"
        );

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
        assert_eq!(helper.submissions_height(), SUBMISSION_VIEWPORT as u16 + 1);

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

        helper.submission_scroll = 0;
        terminal.draw(|f| helper.draw(f)).unwrap();
        let scrollbar = helper.submissions_scrollbar_area;
        assert!(
            scrollbar.height > 2,
            "the submissions panel must expose a scrollbar"
        );
        let mouse = |kind, column, row| {
            Event::Mouse(MouseEvent {
                kind,
                column,
                row,
                modifiers: KeyModifiers::NONE,
            })
        };
        let bottom = scrollbar.y + scrollbar.height - 2;
        helper.handle_event(mouse(
            MouseEventKind::Down(MouseButton::Left),
            scrollbar.x,
            bottom,
        ));
        assert!(
            helper.submission_scroll > 0,
            "clicking the track must seek down"
        );

        helper.handle_event(mouse(
            MouseEventKind::Drag(MouseButton::Left),
            scrollbar.x,
            scrollbar.y + 1,
        ));
        assert_eq!(
            helper.submission_scroll, 0,
            "dragging to the top must seek back"
        );
        helper.handle_event(mouse(
            MouseEventKind::Up(MouseButton::Left),
            scrollbar.x,
            scrollbar.y + 1,
        ));
        assert!(helper.submissions_scrollbar_drag.is_none());

        helper.handle_event(key(KeyCode::Char('s')));
        helper.handle_event(key(KeyCode::Enter));
        assert_eq!(
            helper.submission_scroll, 0,
            "new submission snaps back to newest"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn successful_cases_do_not_churn_the_console() {
        let root = temp_root("console_lanes");
        let fs_ops = FsOps::with_root(root.clone());
        let work = root.join("problems");
        std::fs::create_dir_all(&work).unwrap();

        let mut helper = Helper::new(work, &fs_ops);
        helper.new_problem(sample_problem());
        helper.running = true;
        helper.dir_terminal.clear_result();
        helper.run_phase(helper.run_id, "running case 1/2…".to_string());
        helper.apply_case_result(
            helper.run_id,
            0,
            Verdict::Success {
                output: ok_output("3\n"),
                time: 1,
            },
        );

        let message = &helper.dir_terminal.output.as_ref().unwrap().text;
        assert!(
            message.contains("running case 1/2"),
            "live status stays visible: {message}"
        );
        assert!(
            !message.contains("AC"),
            "successful cases must not spam the console: {message}"
        );

        helper.apply_case_result(
            helper.run_id,
            1,
            Verdict::Success {
                output: ok_output("0\n"),
                time: 2,
            },
        );
        let message = &helper.dir_terminal.output.as_ref().unwrap().text;
        assert!(message.contains("WA"), "failures still surface: {message}");

        helper.run_finished(helper.run_id, 10);
        let message = &helper.dir_terminal.output.as_ref().unwrap().text;
        assert!(
            message.contains("1/2 failed"),
            "the final summary replaces the failure: {message}"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn run_summary_uses_the_runner_elapsed_time() {
        let root = temp_root("run_elapsed");
        let fs_ops = FsOps::with_root(root.clone());
        let work = root.join("problems");
        std::fs::create_dir_all(&work).unwrap();

        let mut helper = Helper::new(work, &fs_ops);
        helper.new_problem(sample_problem());
        helper.running = true;
        helper.run_started = Some(Instant::now() - Duration::from_secs(5));
        helper.apply_case_result(
            helper.run_id,
            0,
            Verdict::Success {
                output: ok_output("3\n"),
                time: 123,
            },
        );
        helper.apply_case_result(
            helper.run_id,
            1,
            Verdict::Success {
                output: ok_output("10\n"),
                time: 456,
            },
        );
        helper.run_finished(helper.run_id, 397);

        let message = &helper.dir_terminal.output.as_ref().unwrap().text;
        assert!(
            message.contains("397ms total"),
            "the runner's elapsed time is the total: {message}"
        );
        assert!(
            !message.contains("579ms"),
            "per-case times must not be summed into a second total: {message}"
        );
        assert!(
            !message.contains("5000ms"),
            "a stalled UI must not inflate the run time: {message}"
        );

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
        problem.test_cases = vec![ReceivingTestCase {
            input: "1".into(),
            expected_output: String::new(),
        }];
        helper.new_problem(problem);

        // Output against an empty expected output is a mismatch, not a pass.
        stream(
            &mut helper,
            vec![Verdict::Success {
                output: ok_output("lkej\n"),
                time: 1,
            }],
        );
        assert!(matches!(
            helper.tc_list.testcases[0].verdict,
            CaseVerdict::WA { .. }
        ));

        // Staying silent still passes.
        stream(
            &mut helper,
            vec![Verdict::Success {
                output: ok_output(""),
                time: 1,
            }],
        );
        assert!(matches!(
            helper.tc_list.testcases[0].verdict,
            CaseVerdict::AC { .. }
        ));

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
                Verdict::Success {
                    output: ok_output("3\n"),
                    time: 1,
                },
                Verdict::Success {
                    output: ok_output("10\n"),
                    time: 1,
                },
            ],
        );
        assert!(
            helper.tc_list.testcases.iter().all(|tc| tc.collapsed),
            "AC cases auto-collapse"
        );

        helper.handle_event(key(KeyCode::Char('e')));
        assert!(matches!(helper.input_mode, InputMode::Editing));
        assert!(
            helper.tc_list.testcases.iter().all(|tc| !tc.collapsed),
            "every case must be visible while editing"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn shift_tab_walks_backwards_while_editing() {
        let root = temp_root("edit-back");
        let fs_ops = FsOps::with_root(root.clone());
        let work = root.join("problems");
        std::fs::create_dir_all(&work).unwrap();

        let mut helper = Helper::new(work, &fs_ops);
        helper.new_problem(sample_problem());
        helper.handle_event(key(KeyCode::Char('e')));
        assert!(matches!(helper.input_mode, InputMode::Editing));
        assert_eq!(
            helper.tc_list.active_box,
            crate::components::testcase::ActiveBox::Input
        );

        helper.handle_event(key(KeyCode::Tab));
        assert_eq!(
            helper.tc_list.active_box,
            crate::components::testcase::ActiveBox::Expected
        );
        helper.handle_event(Event::Key(KeyEvent::new(
            KeyCode::BackTab,
            KeyModifiers::SHIFT,
        )));
        assert_eq!(
            helper.tc_list.active_box,
            crate::components::testcase::ActiveBox::Input
        );
        helper.handle_event(Event::Key(KeyEvent::new(KeyCode::Tab, KeyModifiers::SHIFT)));
        assert_eq!(
            helper.tc_list.active_box,
            crate::components::testcase::ActiveBox::Expected,
            "a terminal that reports Shift+Tab as Tab must still go back"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn editing_keeps_the_cursor_line_visible() {
        let root = temp_root("edit_scroll");
        let fs_ops = FsOps::with_root(root.clone());
        let work = root.join("problems");
        std::fs::create_dir_all(&work).unwrap();

        let mut helper = Helper::new(work, &fs_ops);
        let mut problem = sample_problem();
        problem.test_cases[0].input = (0..100)
            .map(|i| format!("line-{i:03}"))
            .collect::<Vec<_>>()
            .join("\n");
        helper.new_problem(problem);

        helper.handle_event(key(KeyCode::Char('e')));
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal.draw(|f| helper.draw(f)).unwrap();
        dump_screen(&terminal);

        let buffer = terminal.backend().buffer();
        let screen = (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            screen.contains("line-099"),
            "the cursor line must stay on screen:\n{screen}"
        );

        // Adding more lines must not shift the list under the cursor.
        helper.handle_event(key(KeyCode::Enter));
        type_text(&mut helper, "fresh-line");
        terminal.draw(|f| helper.draw(f)).unwrap();
        let buffer = terminal.backend().buffer();
        let screen = (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            screen.contains("fresh-line"),
            "new text must stay on screen:\n{screen}"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn switching_to_expected_does_not_shrink_input() {
        let root = temp_root("edit_stable_panes");
        let fs_ops = FsOps::with_root(root.clone());
        let work = root.join("problems");
        std::fs::create_dir_all(&work).unwrap();

        let mut helper = Helper::new(work, &fs_ops);
        let mut problem = sample_problem();
        problem.test_cases[0].input = (0..40)
            .map(|i| format!("line-{i:03}"))
            .collect::<Vec<_>>()
            .join("\n");
        helper.new_problem(problem);
        helper.handle_event(key(KeyCode::Char('e')));

        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal.draw(|f| helper.draw(f)).unwrap();
        let input_height = helper.tc_list.testcases[0].input_box.rect.height;

        helper.handle_event(key(KeyCode::Tab));
        terminal.draw(|f| helper.draw(f)).unwrap();
        dump_screen(&terminal);
        assert_eq!(
            helper.tc_list.testcases[0].input_box.rect.height, input_height,
            "focusing Expected must not shrink the Input pane"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn mouse_wheel_scrolls_the_editing_textarea() {
        let root = temp_root("edit_wheel");
        let fs_ops = FsOps::with_root(root.clone());
        let work = root.join("problems");
        std::fs::create_dir_all(&work).unwrap();

        let mut helper = Helper::new(work, &fs_ops);
        let mut problem = sample_problem();
        problem.test_cases[0].input = (0..40)
            .map(|i| format!("line-{i:03}"))
            .collect::<Vec<_>>()
            .join("\n");
        helper.new_problem(problem);
        helper.handle_event(key(KeyCode::Char('e')));

        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal.draw(|f| helper.draw(f)).unwrap();

        let rect = helper.tc_list.testcases[0].input_box.rect;
        let wheel = |kind| {
            Event::Mouse(MouseEvent {
                kind,
                column: rect.x + 2,
                row: rect.y + 2,
                modifiers: KeyModifiers::NONE,
            })
        };
        let before = helper.tc_list.testcases[0].inp_content.cursor().0;
        let list_before = helper.tc_list.scroll_row;
        helper.handle_event(wheel(MouseEventKind::ScrollUp));
        assert_eq!(
            helper.tc_list.testcases[0].inp_content.cursor().0,
            before - 1
        );
        assert_eq!(
            helper.tc_list.scroll_row, list_before,
            "the list must not move"
        );
        helper.handle_event(wheel(MouseEventKind::ScrollDown));
        assert!(helper.tc_list.testcases[0].inp_content.cursor().0 <= before);
        assert_eq!(
            helper.tc_list.scroll_row, list_before,
            "the list must not move"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn manual_list_scroll_survives_editing_redraws() {
        let root = temp_root("edit_list_wheel");
        let fs_ops = FsOps::with_root(root.clone());
        let work = root.join("problems");
        std::fs::create_dir_all(&work).unwrap();

        let mut helper = Helper::new(work, &fs_ops);
        let mut problem = sample_problem();
        let body = (0..30)
            .map(|i| format!("line-{i:03}"))
            .collect::<Vec<_>>()
            .join("\n");
        problem.test_cases = (0..4)
            .map(|_| ReceivingTestCase {
                input: body.clone(),
                expected_output: "ok".into(),
            })
            .collect();
        helper.new_problem(problem);
        helper.handle_event(key(KeyCode::Char('e')));

        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal.draw(|f| helper.draw(f)).unwrap();

        let wheel = Event::Mouse(MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: 1,
            row: 1,
            modifiers: KeyModifiers::NONE,
        });
        helper.handle_event(wheel);
        let scrolled = helper.tc_list.scroll_row;
        assert!(scrolled > 0);
        terminal.draw(|f| helper.draw(f)).unwrap();
        assert_eq!(
            helper.tc_list.scroll_row, scrolled,
            "redraw must not snap back"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn mouse_wheel_scrolls_the_delete_list() {
        let root = temp_root("delete_list_wheel");
        let fs_ops = FsOps::with_root(root.clone());
        let work = root.join("problems");
        std::fs::create_dir_all(&work).unwrap();

        let mut helper = Helper::new(work, &fs_ops);
        let mut problem = sample_problem();
        let body = (0..30)
            .map(|i| format!("line-{i:03}"))
            .collect::<Vec<_>>()
            .join("\n");
        problem.test_cases = (0..4)
            .map(|_| ReceivingTestCase {
                input: body.clone(),
                expected_output: "ok".into(),
            })
            .collect();
        helper.new_problem(problem);
        helper.handle_event(key(KeyCode::Char('x')));
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal.draw(|f| helper.draw(f)).unwrap();

        let wheel = Event::Mouse(MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: 1,
            row: 1,
            modifiers: KeyModifiers::NONE,
        });
        helper.handle_event(wheel);
        let scrolled = helper.tc_list.scroll_row;
        assert!(scrolled > 0);
        terminal.draw(|f| helper.draw(f)).unwrap();
        assert_eq!(
            helper.tc_list.scroll_row, scrolled,
            "delete mode keeps manual scroll"
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
    fn double_click_opens_a_bigger_pane_dialog() {
        let root = temp_root("pane_dialog");
        let fs_ops = FsOps::with_root(root.clone());
        let work = root.join("problems");
        std::fs::create_dir_all(&work).unwrap();

        let mut helper = Helper::new(work, &fs_ops);
        helper.new_problem(sample_problem());
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        terminal.draw(|f| helper.draw(f)).unwrap();

        let rect = helper.tc_list.testcases[0].input_box.rect;
        let click = || {
            Event::Mouse(MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: rect.x + 2,
                row: rect.y + 1,
                modifiers: KeyModifiers::NONE,
            })
        };

        helper.handle_event(click());
        assert!(
            helper.pane_dialog.is_none(),
            "one click must not open the dialog"
        );
        helper.handle_event(click());
        let dialog = helper
            .pane_dialog
            .as_ref()
            .expect("double click opens the dialog");
        assert!(dialog.title.starts_with("Input · TestCase 1"));
        assert!(
            dialog
                .visible_lines()
                .iter()
                .any(|line| line.to_string() == "1 2")
        );

        terminal.draw(|f| helper.draw(f)).unwrap();
        dump_screen(&terminal);

        helper.handle_event(key(KeyCode::Esc));
        assert!(helper.pane_dialog.is_none());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn pane_dialog_pans_horizontally() {
        let root = temp_root("pane_dialog_hscroll");
        let fs_ops = FsOps::with_root(root.clone());
        let work = root.join("problems");
        std::fs::create_dir_all(&work).unwrap();

        let mut helper = Helper::new(work, &fs_ops);
        let mut problem = sample_problem();
        problem.test_cases[0].input = format!("START{}END", "x".repeat(300));
        helper.new_problem(problem);
        helper.open_pane_dialog(0, PaneKind::Input);

        let mut terminal = Terminal::new(TestBackend::new(60, 20)).unwrap();
        terminal.draw(|f| helper.draw(f)).unwrap();
        dump_screen(&terminal);
        let screen = |terminal: &Terminal<TestBackend>| {
            let buffer = terminal.backend().buffer();
            (0..buffer.area.height)
                .map(|y| {
                    (0..buffer.area.width)
                        .map(|x| buffer[(x, y)].symbol())
                        .collect::<String>()
                })
                .collect::<Vec<_>>()
                .join("\n")
        };

        let dialog = helper.pane_dialog.as_ref().expect("dialog opened");
        assert_eq!(dialog.h_scroll, 0);
        assert!(dialog.viewport_width < 300);
        assert!(screen(&terminal).contains("START"));

        helper.handle_event(key(KeyCode::Right));
        terminal.draw(|f| helper.draw(f)).unwrap();
        assert_eq!(helper.pane_dialog.as_ref().unwrap().h_scroll, 1);
        assert!(!screen(&terminal).contains("START"));

        helper.handle_event(key(KeyCode::Left));
        terminal.draw(|f| helper.draw(f)).unwrap();
        assert_eq!(helper.pane_dialog.as_ref().unwrap().h_scroll, 0);
        assert!(screen(&terminal).contains("START"));

        let area = helper.pane_dialog.as_ref().unwrap().h_scroll_area;
        assert!(area.width > 1, "the horizontal scrollbar must be hittable");
        let buffer = terminal.backend().buffer();
        let gap_row = (0..area.width)
            .map(|x| buffer[(area.x + x, area.y - 1)].symbol())
            .collect::<String>();
        assert!(
            gap_row.chars().all(|c| c == ' '),
            "the horizontal scrollbar keeps a blank row above it: {gap_row:?}"
        );
        let mouse = |kind, column, row| {
            Event::Mouse(MouseEvent {
                kind,
                column,
                row,
                modifiers: KeyModifiers::NONE,
            })
        };

        helper.handle_event(mouse(
            MouseEventKind::Down(MouseButton::Left),
            area.x + area.width - 1,
            area.y,
        ));
        let dialog = helper.pane_dialog.as_ref().unwrap();
        let max = dialog
            .content
            .max_width()
            .saturating_sub(dialog.viewport_width as usize);
        assert!(max > 0);
        assert_eq!(dialog.h_scroll, max, "clicking the track seeks to the end");

        helper.handle_event(mouse(
            MouseEventKind::Drag(MouseButton::Left),
            area.x,
            area.y,
        ));
        assert_eq!(
            helper.pane_dialog.as_ref().unwrap().h_scroll,
            0,
            "dragging the scrollbar seeks back"
        );

        helper.handle_event(mouse(MouseEventKind::Up(MouseButton::Left), area.x, area.y));
        assert!(helper.pane_dialog.as_ref().unwrap().h_scroll_drag.is_none());

        helper.handle_event(mouse(MouseEventKind::ScrollRight, area.x + 1, area.y));
        assert_eq!(helper.pane_dialog.as_ref().unwrap().h_scroll, 1);
        helper.handle_event(mouse(MouseEventKind::ScrollLeft, area.x + 1, area.y));
        assert_eq!(helper.pane_dialog.as_ref().unwrap().h_scroll, 0);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn pane_dialog_vertical_scrollbar_drags() {
        let root = temp_root("pane_dialog_vscroll");
        let fs_ops = FsOps::with_root(root.clone());
        let work = root.join("problems");
        std::fs::create_dir_all(&work).unwrap();

        let mut helper = Helper::new(work, &fs_ops);
        let input = (0..100)
            .map(|index| format!("line-{index:03}"))
            .collect::<Vec<_>>()
            .join("\n");
        let mut problem = sample_problem();
        problem.test_cases = vec![ReceivingTestCase {
            input,
            expected_output: String::new(),
        }];
        helper.new_problem(problem);
        helper.open_pane_dialog(0, PaneKind::Input);

        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        terminal.draw(|f| helper.draw(f)).unwrap();
        dump_screen(&terminal);
        let area = helper.pane_dialog.as_ref().unwrap().v_scroll_area;
        assert!(
            area.height > 2,
            "the pane dialog must expose a vertical scrollbar"
        );
        let buffer = terminal.backend().buffer();
        for row in area.y..area.y + area.height {
            assert_eq!(
                buffer[(area.x - 1, row)].symbol(),
                " ",
                "the vertical scrollbar keeps a blank column beside it"
            );
        }

        let mouse = |kind, column, row| {
            Event::Mouse(MouseEvent {
                kind,
                column,
                row,
                modifiers: KeyModifiers::NONE,
            })
        };
        let bottom = area.y + area.height - 2;
        helper.handle_event(mouse(
            MouseEventKind::Down(MouseButton::Left),
            area.x,
            bottom,
        ));
        assert!(
            helper.pane_dialog.as_ref().unwrap().scroll > 0,
            "clicking the track must seek down"
        );

        helper.handle_event(mouse(
            MouseEventKind::Drag(MouseButton::Left),
            area.x,
            area.y + 1,
        ));
        assert_eq!(
            helper.pane_dialog.as_ref().unwrap().scroll,
            0,
            "dragging to the top must seek back"
        );
        helper.handle_event(mouse(
            MouseEventKind::Up(MouseButton::Left),
            area.x,
            area.y + 1,
        ));
        assert!(helper.pane_dialog.as_ref().unwrap().v_scroll_drag.is_none());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn pane_dialog_only_closes_on_esc() {
        let root = temp_root("pane_dialog_close");
        let fs_ops = FsOps::with_root(root.clone());
        let work = root.join("problems");
        std::fs::create_dir_all(&work).unwrap();

        let mut helper = Helper::new(work, &fs_ops);
        helper.new_problem(sample_problem());
        helper.open_pane_dialog(0, PaneKind::Input);

        let click = |column, row| {
            Event::Mouse(MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column,
                row,
                modifiers: KeyModifiers::NONE,
            })
        };

        helper.handle_event(click(2, 2));
        assert!(
            helper.pane_dialog.is_some(),
            "a click inside must not close the dialog"
        );
        helper.handle_event(click(90, 28));
        assert!(
            helper.pane_dialog.is_some(),
            "a click outside must not close the dialog"
        );
        helper.handle_event(key(KeyCode::Enter));
        assert!(
            helper.pane_dialog.is_some(),
            "Enter must not close the dialog"
        );
        helper.handle_event(key(KeyCode::Char('q')));
        assert!(helper.pane_dialog.is_some(), "q must not close the dialog");

        helper.handle_event(key(KeyCode::Esc));
        assert!(helper.pane_dialog.is_none());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn paged_got_dialog_keeps_the_diff_and_reads_pages() {
        let root = temp_root("got_dialog_paged");
        let fs_ops = FsOps::with_root(root.clone());
        let work = root.join("problems");
        std::fs::create_dir_all(&work).unwrap();

        let mut helper = Helper::new(work, &fs_ops);
        helper.new_problem(sample_problem());
        let output = (0..20_001)
            .map(|index| format!("line-{index:05}"))
            .collect::<Vec<_>>()
            .join("\n");
        helper.tc_list.testcases[0].set_result(CaseVerdict::WA { ms: 1 }, output);
        helper.open_pane_dialog(0, PaneKind::Got);

        let dialog = helper.pane_dialog.as_ref().expect("dialog opened");
        assert!(matches!(dialog.content, DialogContent::Paged { .. }));
        assert_eq!(
            dialog.total_lines(),
            20_002,
            "one warning row plus the output"
        );
        assert_eq!(dialog.visible_lines().len(), GOT_PAGE_LINES);
        assert!(
            dialog.visible_lines()[0]
                .spans
                .iter()
                .any(|span| span.style.fg == Some(theme::RED())),
            "paged WA output still gets its diff"
        );

        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        terminal.draw(|f| helper.draw(f)).unwrap();
        let buffer = terminal.backend().buffer();
        let first_screen = (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            first_screen.contains("paged view"),
            "the first page must explain that it is paged:\n{first_screen}"
        );
        dump_screen(&terminal);

        helper.set_pane_dialog_scroll(1_000);
        terminal.draw(|f| helper.draw(f)).unwrap();
        let dialog = helper.pane_dialog.as_ref().expect("dialog still open");
        assert_eq!(
            dialog.visible_lines()[487].to_string(),
            "line-00999",
            "the second page must start at line 512 and line up with the scroll"
        );
        dump_screen(&terminal);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn safe_wa_output_keeps_its_diff_in_the_dialog() {
        let root = temp_root("got_dialog_diff");
        let fs_ops = FsOps::with_root(root.clone());
        let work = root.join("problems");
        std::fs::create_dir_all(&work).unwrap();

        let mut helper = Helper::new(work, &fs_ops);
        helper.new_problem(sample_problem());
        helper.tc_list.testcases[0].set_result(CaseVerdict::WA { ms: 1 }, "4".into());
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        terminal.draw(|f| helper.draw(f)).unwrap();

        let rect = helper.tc_list.testcases[0].got_box.rect;
        let click = || {
            Event::Mouse(MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: rect.x + 2,
                row: rect.y + 1,
                modifiers: KeyModifiers::NONE,
            })
        };
        helper.handle_event(click());
        helper.handle_event(click());

        let dialog = helper
            .pane_dialog
            .as_ref()
            .expect("double click opens the Got dialog");
        assert!(dialog.title.starts_with("Got · TestCase 1"));
        assert!(
            dialog
                .visible_lines()
                .iter()
                .flat_map(|line| &line.spans)
                .any(|span| span.style.fg == Some(theme::RED())),
            "safe WA output keeps its red diff in the dialog"
        );
        assert!(
            dialog
                .visible_lines()
                .iter()
                .all(|line| !line.to_string().contains("too large"))
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn double_click_does_not_open_a_dialog_in_delete_mode() {
        let root = temp_root("pane_dialog_delete");
        let fs_ops = FsOps::with_root(root.clone());
        let work = root.join("problems");
        std::fs::create_dir_all(&work).unwrap();

        let mut helper = Helper::new(work, &fs_ops);
        helper.new_problem(sample_problem());
        helper.handle_event(key(KeyCode::Char('x')));
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        terminal.draw(|f| helper.draw(f)).unwrap();

        let rect = helper.tc_list.testcases[0].input_box.rect;
        let click = || {
            Event::Mouse(MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: rect.x + 2,
                row: rect.y + 1,
                modifiers: KeyModifiers::NONE,
            })
        };
        helper.handle_event(click());
        helper.handle_event(click());
        assert!(
            helper.pane_dialog.is_none(),
            "delete mode must keep its own modal"
        );

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

        assert_eq!(
            helper.dir_terminal.cur_dir,
            work.join("cses").canonicalize().unwrap()
        );

        helper.handle_event(key(KeyCode::Esc));
        assert!(matches!(helper.input_mode, InputMode::Normal));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn directory_output_expands_the_path_bar() {
        let root = temp_root("dir_output");
        let fs_ops = FsOps::with_root(root.clone());
        let work = root.join("problems");
        std::fs::create_dir_all(&work).unwrap();
        std::fs::write(work.join("alpha.txt"), "").unwrap();
        std::fs::write(work.join("beta.txt"), "").unwrap();

        let mut helper = Helper::new(work, &fs_ops);
        helper.handle_event(key(KeyCode::Char('d')));
        type_text(&mut helper, "ls");
        helper.handle_event(key(KeyCode::Enter));

        let mut terminal = Terminal::new(TestBackend::new(120, 24)).unwrap();
        terminal.draw(|f| helper.draw(f)).unwrap();
        let buffer = terminal.backend().buffer();
        let row = |y| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        };

        assert!(
            row(2).contains("problems"),
            "the path keeps the first row: {}",
            row(2)
        );
        assert!(
            row(3).contains("alpha.txt") && row(3).contains("beta.txt"),
            "the listing expands the path bar: {}",
            row(3)
        );
        assert!(
            row(4).chars().all(|c| c == '─'),
            "the rule moves below the expanded bar: {}",
            row(4)
        );
        let console = row(buffer.area.height - 1);
        assert!(
            !console.contains("alpha.txt") && !console.contains("beta.txt"),
            "directory output stays out of the run console: {console}"
        );

        helper.handle_event(key(KeyCode::Esc));
        terminal.draw(|f| helper.draw(f)).unwrap();
        let buffer = terminal.backend().buffer();
        let row = |y| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        };
        assert!(
            row(3).chars().all(|c| c == '─'),
            "leaving the console collapses the path bar: {}",
            row(3)
        );

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
        let (_, reply) = fresh.external_command(Some(code_path.clone()), RunAction::Add);
        assert!(reply.is_ok());
        assert!(matches!(fresh.input_mode, InputMode::Editing));
        assert_eq!(fresh.tc_list.testcases.len(), 3);
        assert!(fresh.active_problem.is_some());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn run_without_stored_cases_uses_a_smoke_case() {
        let root = temp_root("smoke");
        let fs_ops = FsOps::with_root(root.clone());
        let work = root.join("problems");
        std::fs::create_dir_all(&work).unwrap();
        assert!(fs_ops.save_config(&UserConfig {
            language: Language::Python,
            extra_flags: vec![],
            ..Default::default()
        }));

        let code_path = work.join("Solo.py");
        std::fs::write(&code_path, "print('hi')\n").unwrap();

        let mut helper = Helper::new(work, &fs_ops);
        let (command, reply) = helper.external_command(Some(code_path), RunAction::Run);
        assert!(reply.is_ok());
        assert!(matches!(
            command,
            Some(PassingCommand::ToRunner(RunnerCommand::RunCode { .. }))
        ));
        assert_eq!(helper.tc_list.testcases.len(), 1, "one smoke case is added");
        assert!(helper.running);

        // A second run reuses that case instead of piling up more.
        helper.running = false;
        let (command, reply) = helper.external_command(None, RunAction::Run);
        assert!(reply.is_ok());
        assert!(command.is_some());
        assert_eq!(helper.tc_list.testcases.len(), 1);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn external_command_reports_missing_files() {
        let root = temp_root("external_missing");
        let fs_ops = FsOps::with_root(root.clone());
        let work = root.join("problems");
        std::fs::create_dir_all(&work).unwrap();

        let mut helper = Helper::new(work, &fs_ops);
        let (command, reply) = helper.external_command(Some(root.join("nope.cpp")), RunAction::Run);
        assert!(command.is_none());
        assert!(reply.unwrap_err().contains("file not found"));

        let _ = std::fs::remove_dir_all(&root);
    }
}
