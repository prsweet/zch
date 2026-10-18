use std::{path::{Path, PathBuf}, process::Output};

use derive_name::VariantName;
use serde::{Deserialize, Serialize};

pub mod fs_ops;
pub mod receiver;
pub mod runner;
pub mod submitter;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ReceivingTestCase {
    pub input: String,
    #[serde(rename = "output")]
    pub expected_output: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ActiveProblem {
    pub name: String,
    #[serde(default)]
    pub url: String,
    #[serde(rename = "timeLimit", default)]
    pub time_limit: u64,
    #[serde(rename = "tests", default)]
    pub test_cases: Vec<ReceivingTestCase>,
    #[serde(default)]
    pub code_file: PathBuf,
}

impl ActiveProblem {
    /// Effective local time limit (judge limit, with sane fallbacks).
    pub fn effective_time_limit(&self) -> u64 {
        match self.time_limit {
            0 => 4000,
            tl => tl.max(500),
        }
    }
}

#[derive(Debug, Serialize, Clone, Copy, Deserialize, PartialEq, Eq)]
#[derive(Default)]
pub enum Language {
    Cpp17,
    Cpp20,
    #[serde(alias = "Cpp")]
    #[default]
    Cpp23,
    Python,
    Java,
}

impl Language {
    pub const ALL: [Language; 5] = [
        Language::Cpp23,
        Language::Cpp20,
        Language::Cpp17,
        Language::Python,
        Language::Java,
    ];

    pub fn label(&self) -> &'static str {
        match self {
            Language::Cpp17 => "C++17",
            Language::Cpp20 => "C++20",
            Language::Cpp23 => "C++23",
            Language::Python => "Python 3",
            Language::Java => "Java",
        }
    }

    pub fn ext(&self) -> &'static str {
        match self {
            Language::Python => "py",
            Language::Java => "java",
            _ => "cpp",
        }
    }

    /// Submit language id on Codeforces.
    pub fn cf_id(&self) -> &'static str {
        match self {
            Language::Cpp17 => "54",
            Language::Cpp20 => "89",
            Language::Cpp23 => "91",
            Language::Python => "31",
            Language::Java => "36",
        }
    }

    /// Submit language id on AtCoder.
    pub fn ac_id(&self) -> &'static str {
        match self {
            Language::Cpp17 | Language::Cpp20 => "5001",
            Language::Cpp23 => "5002",
            Language::Python => "5055",
            Language::Java => "5005",
        }
    }

    pub fn next(&self) -> Language {
        let idx = Self::ALL.iter().position(|l| l == self).unwrap_or(0);
        Self::ALL[(idx + 1) % Self::ALL.len()]
    }

    pub fn prev(&self) -> Language {
        let idx = Self::ALL.iter().position(|l| l == self).unwrap_or(0);
        Self::ALL[(idx + Self::ALL.len() - 1) % Self::ALL.len()]
    }

    /// Compilation step, `None` for interpreted languages.
    pub fn compile_cmd(&self, src: &Path, bin: &Path, extra_flags: &[String]) -> Option<(String, Vec<String>)> {
        let src = src.to_string_lossy().to_string();
        let bin = bin.to_string_lossy().to_string();
        match self {
            Language::Cpp17 | Language::Cpp20 | Language::Cpp23 => {
                let std = match self {
                    Language::Cpp17 => "-std=c++17",
                    Language::Cpp20 => "-std=c++20",
                    _ => "-std=c++23",
                };
                let mut args = vec![std.to_string(), "-O2".to_string(), "-Wall".to_string(), "-Wextra".to_string()];
                args.extend(extra_flags.iter().cloned());
                args.extend([src, "-o".to_string(), bin]);
                Some(("g++".to_string(), args))
            }
            Language::Java => {
                let mut args = extra_flags.to_vec();
                args.push(src);
                Some(("javac".to_string(), args))
            }
            Language::Python => None,
        }
    }

    /// Execution command for one test case.
    pub fn run_cmd(&self, src: &Path, bin: &Path) -> (String, Vec<String>) {
        match self {
            Language::Python => ("python3".to_string(), vec![src.to_string_lossy().to_string()]),
            Language::Java => {
                let class = src
                    .file_stem()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_else(|| "Main".to_string());
                let dir = src
                    .parent()
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_else(|| ".".to_string());
                ("java".to_string(), vec!["-cp".to_string(), dir, class])
            }
            _ => (bin.to_string_lossy().to_string(), Vec::new()),
        }
    }
}


#[derive(Debug, Serialize, Deserialize, Clone)]
#[derive(Default)]
pub struct UserConfig {
    #[serde(default)]
    pub language: Language,
    #[serde(alias = "fallback_flags", default)]
    pub extra_flags: Vec<String>,
}


#[derive(Debug, Clone)]
pub struct SubmitConfig {
    pub language: Language,
    pub url: String,
    pub file_path: PathBuf,
}

pub enum RunnerCommand {
    RunCode { problem: ActiveProblem, run_id: u64 },
}

#[derive(Debug, VariantName, Clone)]
pub enum Verdict {
    Success { output: Output, time: u128 },
    TimeLimitExceeded,
    CompilationError { error: String },
    RuntimeError { error: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunAction {
    Run,
    Add,
    Edit,
    Submit,
}

pub enum HelperCommand {
    New(ActiveProblem),
    /// A command triggered externally (Zed task / HTTP), optionally targeting a file.
    Command { file: Option<PathBuf>, action: RunAction },
    /// One test case finished — streamed as soon as it completes.
    CaseResult { run_id: u64, index: usize, verdict: Verdict },
    /// Every test case of a run finished (or compilation failed).
    RunFinished { run_id: u64 },
    /// A live status line for one in-flight submission, tagged with its id.
    SubmitStatus { id: u64, message: String },
    /// A submission reached its final verdict (or failed) — tagged with its id.
    SubmitFinished { id: u64, result: String },
    Error(String),
    Quit,
}

pub enum SubmitterCommand {
    SubmitCode { id: u64, config: SubmitConfig },
}

#[allow(clippy::enum_variant_names)]
pub enum PassingCommand {
    ToHelper(HelperCommand),
    ToRunner(RunnerCommand),
    ToSubmitter(SubmitterCommand),
}

/*
 * Actor-style wiring: every background thread (receiver, runner, submitter)
 * talks to the UI thread through one PassingCommand channel. The UI thread
 * routes each envelope to the owning sender, so no worker needs a handle
 * to any other worker.
 */
