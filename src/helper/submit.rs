//! Submitting: the confirm sheet's facts and each submission's progress
//! and verdict, worded the way the rest of the app speaks.

use super::*;

pub struct Submission {
    pub id: u64,
    pub label: String,
    pub tone: Tone,
    pub message: String,
    pub started: Instant,
    /// When the verdict came in; `None` while it is still being judged.
    pub finished: Option<Instant>,
}

impl Submission {
    pub fn done(&self) -> bool {
        self.finished.is_some()
    }
}

pub(super) const MAX_SUBMISSIONS: usize = 20;
pub(super) const SUBMISSION_VIEWPORT: usize = 4;

/// What the submit sheet says about local tests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LocalTests {
    Passed { total: usize },
    Failed { failed: usize, total: usize },
    CompileError,
    Stale,
    NotRun { total: usize },
    NoCases,
}

pub struct PendingSubmit {
    pub config: SubmitConfig,
    pub info: SubmissionInfo,
    pub tests: LocalTests,
    pub time_limit: u64,
}

impl Helper {
    pub fn cmd_submit(&mut self) -> Option<PassingCommand> {
        let Some(problem) = &self.active_problem else {
            self.status.warn(RunRefusal::NoProblem.message());
            return None;
        };

        let (config, _) = self.fs_ops.load_config();
        let toolchain = config.toolchain_for(&problem.code_file);
        let payload = SubmitConfig {
            language: toolchain.language,
            language_id: config.cf_id(&toolchain),
            url: problem.url.clone(),
            file_path: problem.code_file.clone(),
        };
        let time_limit = config.time_limit_ms(problem);

        match submitter::describe(&payload) {
            Ok(info) => {
                self.pending_submit = Some(PendingSubmit {
                    config: payload,
                    info,
                    tests: self.local_tests(),
                    time_limit,
                });
                self.input_mode = InputMode::ConfirmSubmit;
            }
            Err(e) => {
                self.status.error(format!("cannot submit — {e}"));
            }
        }
        None
    }

    /// Whether the file about to be submitted is the one that last passed.
    pub(super) fn local_tests(&self) -> LocalTests {
        let Some(problem) = &self.active_problem else {
            return LocalTests::NoCases;
        };
        let total = self.tc_list.testcases.len();
        if total == 0 {
            return LocalTests::NoCases;
        }
        let Some(record) = self
            .last_run
            .as_ref()
            .filter(|record| record.file == problem.code_file && !self.running)
        else {
            return LocalTests::NotRun { total };
        };
        let stamp = source_stamp(&problem.code_file);
        if stamp.is_none() || stamp != record.source_stamp || record.total != total {
            return LocalTests::Stale;
        }
        if record.compile_error {
            LocalTests::CompileError
        } else if record.passed == record.total {
            LocalTests::Passed { total }
        } else {
            LocalTests::Failed {
                failed: record.total - record.passed,
                total,
            }
        }
    }

    pub fn submit_status(&mut self, id: u64, message: String) {
        let (tone, text) = present_submit_progress(&message);
        if let Some(submission) = self.submissions.iter_mut().find(|s| s.id == id) {
            submission.tone = tone;
            submission.message = text;
        }
    }

    pub fn submit_finished(&mut self, id: u64, result: String) {
        let (tone, text) = present_submit_result(&result);
        if let Some(submission) = self.submissions.iter_mut().find(|s| s.id == id) {
            submission.tone = tone;
            submission.message = text;
            submission.finished = Some(Instant::now());
        }
    }
}

/// A submitter progress line, in the app's words. The submitter reports
/// Codeforces states ("In queue", "Running on test 4") and its own steps.
pub(super) fn present_submit_progress(raw: &str) -> (Tone, String) {
    let text = raw.trim();
    let lower = text.to_ascii_lowercase();
    if lower.starts_with("captcha") {
        return (
            Tone::Warning,
            "captcha — solve it in Safari and the submission continues".to_string(),
        );
    }
    (Tone::Progress, sentence_case(text))
}

/// Codeforces writes "Running on test 4", "ACCEPTED" or "TLE"; the app
/// writes lowercase. All-caps text is lowered whole, an acronym is kept,
/// and otherwise only the first letter changes.
pub(super) fn sentence_case(text: &str) -> String {
    let text = text.trim();
    let letters: Vec<char> = text.chars().filter(|c| c.is_alphabetic()).collect();
    if letters.len() > 1 && letters.iter().all(|c| c.is_uppercase()) {
        return text.to_lowercase();
    }
    let mut chars = text.chars();
    match (chars.next(), text.chars().nth(1)) {
        (Some(first), Some(second)) if second.is_uppercase() => {
            // An acronym ("TLE on test 3"): leave it.
            std::iter::once(first).chain(chars).collect()
        }
        (Some(first), _) => first.to_lowercase().chain(chars).collect(),
        (None, _) => String::new(),
    }
}

/// The final line of a submission, in the app's words, with its tone.
pub(super) fn present_submit_result(raw: &str) -> (Tone, String) {
    let lowercase_first = sentence_case;
    let detail = |prefix: &str| raw.strip_prefix(prefix).map(str::trim).unwrap_or(raw);
    match submitter::classify(raw) {
        submitter::Outcome::Accepted => (Tone::Success, lowercase_first(detail("RESULT:"))),
        submitter::Outcome::PartiallyAccepted => {
            (Tone::Partial, lowercase_first(detail("RESULT:")))
        }
        submitter::Outcome::Rejected if raw.starts_with("REJECTED") => (
            Tone::Error,
            format!("rejected — {}", lowercase_first(detail("REJECTED:"))),
        ),
        submitter::Outcome::Rejected => (Tone::Error, lowercase_first(detail("RESULT:"))),
        submitter::Outcome::Failed => {
            for (prefix, word) in [
                ("INTERRUPTED:", "interrupted"),
                ("TIMED OUT:", "timed out"),
                ("ERROR:", "failed"),
                ("UNKNOWN:", "unknown"),
            ] {
                if let Some(rest) = raw.strip_prefix(prefix) {
                    let tone = if word == "failed" {
                        Tone::Error
                    } else {
                        Tone::Warning
                    };
                    // The cause keeps its own casing: it often starts with
                    // a name ("Safari tab was closed").
                    return (tone, format!("{word} — {}", rest.trim()));
                }
            }
            (Tone::Error, lowercase_first(raw))
        }
    }
}
