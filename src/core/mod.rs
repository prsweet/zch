use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    process::Output,
    sync::{Arc, atomic::AtomicBool, mpsc::Sender},
};

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

#[derive(Debug, Serialize, Clone, Copy, Deserialize, PartialEq, Eq, Hash, Default)]
pub enum Language {
    #[serde(alias = "Cpp17", alias = "Cpp20", alias = "Cpp23")]
    #[default]
    Cpp,
    Python,
    Java,
}

impl Language {
    pub const ALL: [Language; 3] = [Language::Cpp, Language::Python, Language::Java];

    pub fn label(&self) -> &'static str {
        match self {
            Language::Cpp => "C++",
            Language::Python => "Python 3",
            Language::Java => "Java",
        }
    }

    pub fn ext(&self) -> &'static str {
        match self {
            Language::Python => "py",
            Language::Java => "java",
            Language::Cpp => "cpp",
        }
    }

    /// Submit language id on Codeforces. The standard version is not part of
    /// the language any more — it comes from the user's `-std=` flag.
    pub fn cf_id(&self, flags: &[String]) -> &'static str {
        match self {
            Language::Python => "31",
            Language::Java => "36",
            Language::Cpp => match std_from_flags(flags) {
                Some(std) if std >= 23 => "91",
                Some(std) if std >= 20 => "89",
                _ => "54",
            },
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

    /// Compilation step, `None` for interpreted languages. No flags are added
    /// implicitly — the user's extra flags are passed through verbatim.
    pub fn compile_cmd(
        &self,
        src: &Path,
        bin: &Path,
        extra_flags: &[String],
    ) -> Option<(String, Vec<String>)> {
        let src = src.to_string_lossy().to_string();
        let bin = bin.to_string_lossy().to_string();
        match self {
            Language::Cpp => {
                let mut args = extra_flags.to_vec();
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
            Language::Python => (
                "python3".to_string(),
                vec![src.to_string_lossy().to_string()],
            ),
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

/// The standard version encoded in a `-std=c++20` / `-std=gnu++17` /
/// `--std=c++2b` flag. Unknown versions return `None`, so a typo can never be
/// mistaken for a real standard.
pub(crate) fn std_from_flag(flag: &str) -> Option<u32> {
    let lower = flag.to_ascii_lowercase();
    let value = lower
        .strip_prefix("-std=")
        .or_else(|| lower.strip_prefix("--std="))?;
    let version = value
        .strip_prefix("gnu++")
        .or_else(|| value.strip_prefix("c++"))?;
    match version {
        "98" => Some(98),
        "03" => Some(3),
        "0x" | "11" => Some(11),
        "1y" | "14" => Some(14),
        "1z" | "17" => Some(17),
        "2a" | "20" => Some(20),
        "2b" | "23" => Some(23),
        "2c" | "26" => Some(26),
        _ => None,
    }
}

/// Last valid `-std=` flag wins.
fn std_from_flags(flags: &[String]) -> Option<u32> {
    flags.iter().rev().find_map(|flag| std_from_flag(flag))
}

/// Named palette. The config section cycles this list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum AppTheme {
    #[default]
    Mocha,
    #[serde(rename = "prsweet's favourite")]
    Prsweet,
}

impl AppTheme {
    pub const ALL: [AppTheme; 2] = [AppTheme::Mocha, AppTheme::Prsweet];

    pub fn label(self) -> &'static str {
        match self {
            AppTheme::Mocha => "Mocha",
            AppTheme::Prsweet => "prsweet's favourite",
        }
    }

    pub fn next(self) -> Self {
        let index = Self::ALL.iter().position(|theme| *theme == self).unwrap_or(0);
        Self::ALL[(index + 1) % Self::ALL.len()]
    }

    pub fn prev(self) -> Self {
        let index = Self::ALL.iter().position(|theme| *theme == self).unwrap_or(0);
        Self::ALL[(index + Self::ALL.len() - 1) % Self::ALL.len()]
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct UserConfig {
    #[serde(default)]
    pub language: Language,
    #[serde(alias = "fallback_flags", default)]
    pub extra_flags: Vec<String>,
    #[serde(default)]
    pub flags_by_language: HashMap<Language, Vec<String>>,
    #[serde(default)]
    pub theme: AppTheme,
}

impl UserConfig {
    pub fn normalize(&mut self) {
        if self.flags_by_language.is_empty() {
            self.flags_by_language
                .insert(self.language, self.extra_flags.clone());
            return;
        }
        match self.flags_by_language.get(&self.language) {
            Some(flags) => self.extra_flags = flags.clone(),
            None => {
                self.flags_by_language
                    .insert(self.language, self.extra_flags.clone());
            }
        }
    }

    pub fn active_flags(&self) -> &[String] {
        self.flags_by_language
            .get(&self.language)
            .map(Vec::as_slice)
            .unwrap_or(&self.extra_flags)
    }
}

#[derive(Debug, Clone)]
pub struct SubmitConfig {
    pub language: Language,
    pub extra_flags: Vec<String>,
    pub url: String,
    pub file_path: PathBuf,
}

pub enum RunnerCommand {
    RunCode {
        problem: ActiveProblem,
        run_id: u64,
        cancel: Arc<AtomicBool>,
    },
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
    /// The reply is sent once the UI thread has handled it.
    Command {
        file: Option<PathBuf>,
        action: RunAction,
        reply: Sender<Result<String, String>>,
    },
    /// A live status line while a run compiles / executes.
    RunPhase {
        run_id: u64,
        message: String,
    },
    /// One test case finished — streamed as soon as it completes.
    CaseResult {
        run_id: u64,
        index: usize,
        verdict: Verdict,
    },
    /// Every test case of a run finished (or compilation failed).
    RunFinished {
        run_id: u64,
        elapsed_ms: u128,
    },
    /// A live status line for one in-flight submission, tagged with its id.
    SubmitStatus {
        id: u64,
        message: String,
    },
    /// A submission reached its final verdict (or failed) — tagged with its id.
    SubmitFinished {
        id: u64,
        result: String,
    },
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_language_names_still_load_as_cpp() {
        for name in ["Cpp", "Cpp17", "Cpp20", "Cpp23"] {
            let json = format!(r#"{{"language":"{name}"}}"#);
            let config: UserConfig = serde_json::from_str(&json).unwrap();
            assert_eq!(config.language, Language::Cpp);
        }
    }

    #[test]
    fn legacy_fallback_flags_become_the_current_language_flags() {
        let json = r#"{"language":"Python","fallback_flags":["-u"]}"#;
        let mut config: UserConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.active_flags(), ["-u"]);

        config.normalize();
        assert_eq!(config.extra_flags, ["-u"]);
        assert_eq!(config.flags_by_language[&Language::Python], ["-u"]);
        assert_eq!(config.active_flags(), ["-u"]);
    }

    #[test]
    fn normalize_prefers_the_selected_language_flags() {
        let mut config = UserConfig {
            language: Language::Python,
            extra_flags: vec!["-u".into()],
            flags_by_language: HashMap::from([
                (Language::Cpp, vec!["-O2".into()]),
                (Language::Python, vec!["-B".into()]),
            ]),
            ..Default::default()
        };

        config.normalize();
        assert_eq!(config.extra_flags, ["-B"]);
        assert_eq!(config.active_flags(), ["-B"]);
    }

    #[test]
    fn cpp_compiles_without_implicit_flags() {
        let src = Path::new("A.cpp");
        let bin = Path::new("A");

        let (program, args) = Language::Cpp.compile_cmd(src, bin, &[]).unwrap();
        assert_eq!(program, "g++");
        assert_eq!(args, ["A.cpp", "-o", "A"]);

        let flags = vec!["-std=c++20".to_string(), "-O2".to_string()];
        let (_, args) = Language::Cpp.compile_cmd(src, bin, &flags).unwrap();
        assert_eq!(args, ["-std=c++20", "-O2", "A.cpp", "-o", "A"]);

        assert!(Language::Python.compile_cmd(src, bin, &[]).is_none());
    }

    #[test]
    fn cf_id_follows_the_std_flag() {
        assert_eq!(Language::Cpp.cf_id(&[]), "54");
        assert_eq!(Language::Cpp.cf_id(&["-std=c++17".into()]), "54");
        assert_eq!(Language::Cpp.cf_id(&["-std=c++20".into()]), "89");
        assert_eq!(Language::Cpp.cf_id(&["-std=gnu++20".into()]), "89");
        assert_eq!(Language::Cpp.cf_id(&["-std=c++23".into()]), "91");
        assert_eq!(Language::Cpp.cf_id(&["-std=c++2b".into()]), "91");
        assert_eq!(Language::Cpp.cf_id(&["-std=c++26".into()]), "91");
        assert_eq!(Language::Cpp.cf_id(&["-std=c++1000".into()]), "54");
        assert_eq!(
            Language::Cpp.cf_id(&["-O2".into(), "-std=c++20".into()]),
            "89"
        );
        assert_eq!(Language::Python.cf_id(&["-std=c++20".into()]), "31");
    }

    #[test]
    fn std_parser_rejects_unknown_versions() {
        assert_eq!(std_from_flag("-std=c++98"), Some(98));
        assert_eq!(std_from_flag("-std=gnu++03"), Some(3));
        assert_eq!(std_from_flag("--std=c++2c"), Some(26));
        assert_eq!(std_from_flag("-STD=C++1z"), Some(17));
        assert_eq!(std_from_flag("-std=c++1000"), None);
        assert_eq!(std_from_flag("-std=c++19"), None);
        assert_eq!(std_from_flag("-std="), None);
        assert_eq!(std_from_flag("-O2"), None);
    }
}
