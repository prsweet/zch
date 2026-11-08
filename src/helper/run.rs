//! Running the cases: starting a run, following the runner's phases,
//! taking each verdict as it arrives and summing the run up.

use super::*;

#[derive(Clone, Copy)]
pub(super) enum RunRefusal {
    AlreadyRunning,
    NoProblem,
}

impl RunRefusal {
    pub(super) fn message(self) -> &'static str {
        match self {
            Self::AlreadyRunning => "a run is already in progress — esc cancels it",
            Self::NoProblem => "no problem loaded — send one from Competitive Companion",
        }
    }
}

/// How the last run went, kept so the submit sheet can say whether the
/// code being submitted is the code that was tested.
#[derive(Debug, Clone)]
pub struct RunRecord {
    pub passed: usize,
    pub total: usize,
    pub compile_error: bool,
    pub file: PathBuf,
    /// The source's modification time when the run started.
    pub source_stamp: Option<std::time::SystemTime>,
}

/// The live run, for the progress rule and the running-case spinner.
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct RunProgress {
    pub(super) total: usize,
    pub(super) done: usize,
    /// The case running now; `None` while compiling.
    pub(super) current: Option<usize>,
    pub(super) failed: usize,
}

impl Helper {
    pub fn cmd_run(&mut self) -> Option<PassingCommand> {
        match self.begin_run() {
            Ok(command) => Some(command),
            Err(refusal) => {
                self.status.warn(refusal.message());
                None
            }
        }
    }

    /// The only place that starts a run — both the TUI key and `zch send`
    /// go through it, so the "already running" guard cannot drift.
    pub(super) fn begin_run(&mut self) -> Result<PassingCommand, RunRefusal> {
        if self.running {
            return Err(RunRefusal::AlreadyRunning);
        }
        let Some(mut problem) = self.active_problem.clone() else {
            return Err(RunRefusal::NoProblem);
        };
        // No samples: run the program live instead of inventing an empty case.
        if problem.test_cases.is_empty() && self.tc_list.testcases.is_empty() {
            return self.cmd_interact().ok_or(RunRefusal::AlreadyRunning);
        }
        // The runner judges against what is on screen, including edits that
        // have not been saved yet (a `zch send r` while editing).
        problem.test_cases = self.current_test_cases();
        self.stop_session();

        for tc in self.tc_list.testcases.iter_mut() {
            tc.reset_result();
        }
        self.running = true;
        self.run_started = Some(Instant::now());
        self.run_id = self.run_id.wrapping_add(1);
        let cancel = Arc::new(AtomicBool::new(false));
        self.run_cancel = cancel.clone();
        let total = problem.test_cases.len();
        self.run_progress = RunProgress {
            total,
            ..RunProgress::default()
        };
        self.last_run = Some(RunRecord {
            passed: 0,
            total,
            compile_error: false,
            file: problem.code_file.clone(),
            source_stamp: source_stamp(&problem.code_file),
        });
        self.motion.run_started();
        self.status.clear();
        self.status.progress(format!(
            "running {total} case{}",
            if total == 1 { "" } else { "s" }
        ));
        Ok(ToRunner(RunnerCommand::RunCode {
            problem,
            run_id: self.run_id,
            cancel,
        }))
    }

    /// Live compile/run status from the runner, rephrased into the app's
    /// own words so the status line reads the same whatever sent it.
    pub fn run_phase(&mut self, run_id: u64, message: String) {
        if run_id != self.run_id || !self.running {
            return;
        }
        let total = self.run_progress.total;
        // Short on purpose: the cards already show which case runs and how
        // each one went; the console only says where the run is.
        let text = if message.starts_with("compiling") {
            "compiling".to_string()
        } else if let Some(ms) = message
            .strip_prefix("compiled in ")
            .and_then(|rest| rest.trim_end_matches("ms").parse::<u128>().ok())
        {
            format!("compiled in {}", format_ms(ms))
        } else if message.starts_with("sources unchanged") {
            "build cached".to_string()
        } else if let Some((index, _)) = message
            .strip_prefix("running case ")
            .and_then(|rest| rest.trim_end_matches('…').split_once('/'))
        {
            let index = index.parse::<usize>().unwrap_or(1);
            self.run_progress.current = Some(index.saturating_sub(1));
            format!("running {index}/{total}")
        } else {
            message
        };
        self.status.progress(text);
    }

    pub(super) fn cancel_run(&mut self) {
        self.status.clear_progress();
        if self.running
            && let Some(started) = self.run_started.take()
        {
            self.status.info(format!(
                "run cancelled after {}",
                format_ms(started.elapsed().as_millis())
            ));
            self.motion.flash(Tone::Info);
        }
        self.run_started = None;
        self.run_cancel.store(true, Ordering::SeqCst);
        self.running = false;
        self.run_progress.current = None;
        self.run_id = self.run_id.wrapping_add(1);
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

        // stdout, then a stderr section for diagnostics, when there are any.
        let with_details = |stdout: &str, details: &str| {
            let stdout = stdout.trim_end();
            let details = details.trim_end();
            match (stdout.is_empty(), details.is_empty()) {
                (_, true) => stdout.to_string(),
                (true, false) => details.to_string(),
                (false, false) => format!("{stdout}{STDERR_MARKER}{details}"),
            }
        };

        let mut memory = None;
        let (verdict, got) = match result {
            Verdict::Success {
                output,
                time,
                memory_kb,
                accepted,
                note,
            } => {
                memory = memory_kb;
                let stdout = String::from_utf8_lossy(&output.stdout);
                let mut details = String::from_utf8_lossy(&output.stderr)
                    .trim_end()
                    .to_string();
                if let Some(note) = note {
                    if !details.is_empty() {
                        details.push('\n');
                    }
                    details.push_str(&note);
                }
                // Unlike failures, the stderr section is kept even when stdout
                // is empty, so debug output is never diffed as the answer.
                let mut display = stdout.trim_end().to_string();
                if !details.is_empty() {
                    display.push_str(&format!("{STDERR_MARKER}{details}"));
                }
                if accepted {
                    (CaseVerdict::AC { ms: time }, display)
                } else {
                    (CaseVerdict::WA { ms: time }, display)
                }
            }
            Verdict::TimeLimitExceeded {
                time,
                limit,
                killed,
                stdout,
            } => {
                let (time, limit) = (format_ms(time), format_ms(limit));
                let summary = if killed {
                    format!("still running after {time}, stopped · limit {limit}")
                } else {
                    format!("took {time} · limit {limit}")
                };
                (CaseVerdict::Tle, with_details(&stdout, &summary))
            }
            Verdict::OutputLimitExceeded { limit } => (
                CaseVerdict::Ole,
                format!(
                    "output limit exceeded: printed more than {} MB and was stopped",
                    limit / (1024 * 1024)
                ),
            ),
            Verdict::RuntimeError { error, stdout } => {
                (CaseVerdict::RE, with_details(&stdout, &error))
            }
            Verdict::CompilationError { error } => (CaseVerdict::CE, error),
        };

        tc.set_result(verdict, got);
        tc.memory_kb = memory;
        if self
            .pane_dialog
            .as_ref()
            .is_some_and(|dialog| dialog.pane == PaneKind::Got && dialog.case_index == index)
        {
            self.pane_dialog = None;
        }

        // Progress counts what is done; the card itself shows the verdict, so
        // a failing case does not also shout in the status line mid-run.
        self.run_progress.done = (index + 1).max(self.run_progress.done).min(total);
        if verdict.is_failed() {
            self.run_progress.failed += 1;
        }
    }

    /// All cases of a run are in: summarize and focus the first failure.
    pub fn run_finished(&mut self, run_id: u64, elapsed_ms: u128) {
        if run_id != self.run_id {
            return;
        }
        self.running = false;
        self.run_started = None;
        self.run_progress.current = None;
        self.status.clear_progress();

        let total = self.tc_list.testcases.len();
        if total == 0 {
            self.status.warn("no test cases — press a to add one");
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

        if let Some(record) = self.last_run.as_mut() {
            record.passed = passed;
            record.total = total;
            record.compile_error = compile_error;
        }

        let took = format_ms(elapsed_ms);
        let first_failure = self
            .tc_list
            .testcases
            .iter()
            .position(|tc| tc.verdict.is_failed());

        // The cards carry the detail (which case, which verdict, the
        // compiler's message); the console only sums the run up.
        let tone = if compile_error {
            self.status.error("compile error");
            Tone::Error
        } else if failed == 0 {
            self.status.success(format!("all {passed} passed · {took}"));
            Tone::Success
        } else {
            self.status
                .error(format!("{failed} of {total} failed · {took}"));
            Tone::Error
        };
        self.motion.flash(tone);

        if failed > 0
            && let Some(idx) = first_failure
        {
            self.tc_list.active_area = idx;
            self.selection_visible = true;
            self.tc_list.update_scroll();
        }
    }
}
