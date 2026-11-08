//! Which problem is loaded: received from Competitive Companion, opened
//! by `zch send`, and the replies `zch send` prints.

use super::*;

impl Helper {
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
                    let message = if matches!(
                        command,
                        PassingCommand::ToRunner(RunnerCommand::Interact { .. })
                    ) {
                        "interactive session started".to_string()
                    } else {
                        let cases = self.tc_list.testcases.len();
                        format!("running {cases} case{}", if cases == 1 { "" } else { "s" })
                    };
                    (Some(command), Ok(message))
                }
                Err(refusal) => {
                    self.status.warn(refusal.message());
                    (None, Err(refusal.message().to_string()))
                }
            },
            // `zch send` prints the same words the status line shows.
            RunAction::Add => {
                if self.cmd_add() {
                    (None, Ok("added a case — fill it in zch".to_string()))
                } else {
                    (None, Err(self.status_text()))
                }
            }
            RunAction::Edit => {
                if self.cmd_edit() {
                    (None, Ok("editing cases in zch".to_string()))
                } else {
                    (None, Err(self.status_text()))
                }
            }
            RunAction::Submit => {
                self.cmd_submit();
                if self.pending_submit.is_some() {
                    (None, Ok("confirm the submission in zch".to_string()))
                } else {
                    (None, Err(self.status_text()))
                }
            }
        }
    }

    pub(super) fn status_text(&self) -> String {
        self.status.text().unwrap_or("refused").to_string()
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

        // Only the user's own config may name a program to execute.
        problem.checker = None;

        let save_error = self.fs_ops.save_tc_file(&problem).err();

        let code_path = match self.fs_ops.write_code_file(&problem, &code_dir) {
            Ok(path) => path,
            Err(e) => {
                self.status
                    .error(format!("cannot create the source file — {e}"));
                return None;
            }
        };
        problem.code_file = code_path.clone();

        let samples = problem.test_cases.len();
        self.attach_problem(problem);
        self.input_mode = InputMode::Normal;
        match save_error {
            // The header already names the problem.
            Some(e) => self.status.error(format!("cases not saved — {e}")),
            None => self.status.success(match samples {
                0 => "received · no samples, r runs it live".to_string(),
                1 => "received · 1 sample".to_string(),
                n => format!("received · {n} samples"),
            }),
        }
        Some(code_path)
    }

    pub(super) fn set_active_from_file(&mut self, file: &Path) -> Result<(), String> {
        if !file.exists() {
            let message = format!("file not found — {}", file.display());
            self.status.error(message.clone());
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
                checker: None,
            },
        };

        self.attach_problem(problem);
        Ok(())
    }

    /// Make `problem` current, replacing the previous cases and retiring any
    /// in-flight run. Shared by receiver payloads and `zch send`.
    pub(super) fn attach_problem(&mut self, problem: ActiveProblem) {
        self.stop_session();
        self.session = None;
        if matches!(self.input_mode, InputMode::Session) {
            self.input_mode = InputMode::Normal;
        }
        self.tc_list.clear();
        for tc in &problem.test_cases {
            self.tc_list
                .testcases
                .push(TestCase::new(&tc.input, &tc.expected_output));
        }
        self.active_problem = Some(problem);
        self.adding_case = false;
        self.selection_visible = false;
        self.cancel_run();
        // A new problem starts on a clean status line; the previous run's
        // result no longer describes what is on screen.
        self.status.clear();
        self.motion.problem_changed();
        self.dirty = true;
    }

    /// An error from a background worker (the listener, a payload).
    pub fn set_error(&mut self, message: String) {
        self.status.error(message);
    }

    /// The problem listener could not start; the splash says so.
    pub fn listener_down(&mut self, reason: String) {
        self.listener = Listener::Down(reason.clone());
        self.status.error(reason);
    }
}

pub(super) fn url_from_code(path: &Path) -> Option<String> {
    let content = std::fs::read_to_string(path).ok()?;
    content.lines().take(5).find_map(|line| {
        line.strip_prefix("// URL: ")
            .or_else(|| line.strip_prefix("# URL: "))
            .map(|s| s.trim().to_string())
    })
}
