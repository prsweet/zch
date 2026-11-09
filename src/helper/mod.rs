//! The application: all UI state, owned by the main thread.
//!
//! `Helper` is the one place state lives. Workers (listener, runner,
//! submitter) never touch it; they send messages that `main` hands to the
//! methods here. The behaviour is split by concern:
//!
//! | module      | owns                                                   |
//! |-------------|--------------------------------------------------------|
//! | `run`       | running cases, verdicts, the run summary               |
//! | `cases`     | adding, editing, deleting and saving cases             |
//! | `submit`    | the submit sheet and each submission's progress        |
//! | `problem`   | which problem is loaded, `zch send` replies            |
//! | `session`   | interactive programs for problems without samples      |
//! | `dialog`    | the large pane viewer                                  |
//! | `selection` | mouse text selection and the clipboard                 |
//! | `input`     | keyboard and mouse dispatch                            |
//! | `view`      | everything drawn                                       |

use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicI32, Ordering},
        mpsc::{self, Sender},
    },
    time::{Duration, Instant},
};

use ratatui::{
    crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind},
    layout::{Position, Rect},
    style::Style,
    text::{Line, Span},
};

use crate::{
    components::{
        ScrollbarDrag,
        config::ConfigMenu,
        dir_terminal::DirTerminal,
        splash::Listener,
        status::{Status, Tone, format_ms},
        step_wrapped,
        tc_list::TestCaseList,
        testcase::{
            ActiveBox, CaseVerdict, GOT_PAGE_LINES, PAGED_OUTPUT_NOTE, PaneKind, STDERR_MARKER,
            TestCase,
        },
        theme,
    },
    core::{
        ActiveProblem, HelperCommand,
        PassingCommand::{self, ToHelper, ToRunner, ToSubmitter},
        RunAction, RunnerCommand, SessionInput, SessionStream, SubmitConfig, SubmitterCommand,
        Verdict,
        fs_ops::FsOps,
        submitter::{self, SubmissionInfo},
    },
};
use unicode_width::UnicodeWidthStr;

mod cases;
mod dialog;
mod input;
mod problem;
mod run;
mod selection;
mod session;
mod submit;
mod view;

#[cfg(test)]
mod tests;

use dialog::*;
use run::*;
use selection::*;
use session::*;
use submit::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputMode {
    Normal,
    Editing,
    DeleteSelect,
    ConfirmDelete,
    Directory,
    Config,
    ConfirmSubmit,
    /// Quitting would lose submissions that are still being judged.
    ConfirmQuit,
    /// Typing into a live program. Only used when the problem has no samples.
    Session,
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
    /// The status line and its message log.
    pub status: Status,
    pending_submit: Option<PendingSubmit>,
    run_id: u64,
    next_submission_id: u64,
    submission_scroll: usize,
    submissions_area: Rect,
    submissions_scrollbar_area: Rect,
    submissions_scrollbar_drag: Option<ScrollbarDrag>,
    run_cancel: Arc<AtomicBool>,
    run_started: Option<Instant>,
    run_progress: RunProgress,
    last_run: Option<RunRecord>,
    pane_dialog: Option<PaneDialog>,
    last_click: Option<(Instant, ClickTarget)>,
    show_help: bool,
    show_log: bool,
    /// The Normal-mode selection rail is shown once the user (or a failed
    /// run) has picked a case; before that nothing is marked.
    selection_visible: bool,
    adding_case: bool,
    session: Option<InteractiveSession>,
    next_session_id: u64,
    listener: Listener,
    motion: view::Motion,
    /// Where the left button went down, for telling a drag from a click.
    press: Option<Position>,
    /// Text being (or just) selected with the mouse.
    text_selection: Option<TextSelection>,
    /// The last frame's text, row by row, for copying a selection.
    screen_rows: Vec<String>,
    /// Panels drawn last frame that a selection stays inside.
    text_regions: Vec<Rect>,
}

impl Helper {
    pub fn new(initial_dir: PathBuf, fs_ops: &FsOps) -> Self {
        let config_error = fs_ops.config_error();
        let (config, _) = fs_ops.load_config();
        theme::set(config.theme);
        let mut status = Status::default();
        if let Some(error) = config_error {
            status.error(error);
        }

        Self {
            dir_terminal: DirTerminal::new(initial_dir),
            active_problem: None,
            input_mode: InputMode::Normal,
            tc_list: TestCaseList::new(),
            config_menu: ConfigMenu::new(&config),
            fs_ops: fs_ops.clone(),
            submissions: Vec::new(),
            running: false,
            dirty: true,
            status,
            pending_submit: None,
            run_id: 0,
            next_submission_id: 1,
            submission_scroll: 0,
            submissions_area: Rect::default(),
            submissions_scrollbar_area: Rect::default(),
            submissions_scrollbar_drag: None,
            run_cancel: Arc::new(AtomicBool::new(false)),
            run_started: None,
            run_progress: RunProgress::default(),
            last_run: None,
            pane_dialog: None,
            last_click: None,
            show_help: false,
            show_log: false,
            selection_visible: false,
            adding_case: false,
            listener: Listener::Up(crate::core::receiver::listen_port()),
            motion: view::Motion::new(),
            press: None,
            text_selection: None,
            screen_rows: Vec::new(),
            text_regions: Vec::new(),
            session: None,
            next_session_id: 1,
        }
    }
}

/// When a source file last changed, to tell whether a run is still current.
fn source_stamp(path: &Path) -> Option<std::time::SystemTime> {
    std::fs::metadata(path)
        .and_then(|meta| meta.modified())
        .ok()
}
