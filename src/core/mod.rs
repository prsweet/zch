use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    process::Output,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicI32},
        mpsc::{Receiver, Sender},
    },
};

use derive_name::VariantName;
use serde::{Deserialize, Serialize};

pub mod checker;
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
    /// Per-problem override of the configured output checker.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checker: Option<Checker>,
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

    /// The language a source file is written in, judged by its extension.
    pub fn from_path(path: &Path) -> Option<Language> {
        let ext = path.extension()?.to_str()?.to_ascii_lowercase();
        match ext.as_str() {
            "cpp" | "cc" | "cxx" | "c++" => Some(Language::Cpp),
            "py" => Some(Language::Python),
            "java" => Some(Language::Java),
            _ => None,
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
    /// Python goes to PyPy 3 (70) and Java to Java 21 (87); both can be
    /// overridden per language with `cf_language_ids` in the config.
    pub fn cf_id(&self, flags: &[String]) -> &'static str {
        match self {
            Language::Python => "70",
            Language::Java => "87",
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

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct UserConfig {
    #[serde(default)]
    pub language: Language,
    #[serde(alias = "fallback_flags", default)]
    pub extra_flags: Vec<String>,
    #[serde(default)]
    pub flags_by_language: HashMap<Language, Vec<String>>,
    /// How outputs are compared with the expected answers.
    #[serde(default)]
    pub checker: Checker,
    /// Scales every problem's time limit locally (a machine slower than the
    /// judge wants a value above 1).
    #[serde(
        default = "default_time_limit_multiplier",
        skip_serializing_if = "is_default_multiplier"
    )]
    pub time_limit_multiplier: f64,
    /// Codeforces `programTypeId` overrides, e.g. `{"Python": "31"}`.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub cf_language_ids: HashMap<Language, String>,
    /// The colour theme: `prsweet`, `catppuccin` or `obsidian`.
    #[serde(default)]
    pub theme: crate::components::theme::ThemeName,
}

impl Default for UserConfig {
    fn default() -> Self {
        Self {
            language: Language::default(),
            extra_flags: Vec::new(),
            flags_by_language: HashMap::new(),
            checker: Checker::default(),
            time_limit_multiplier: default_time_limit_multiplier(),
            cf_language_ids: HashMap::new(),
            theme: Default::default(),
        }
    }
}

fn default_time_limit_multiplier() -> f64 {
    1.0
}

fn is_default_multiplier(value: &f64) -> bool {
    *value == 1.0
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

    /// The flags stored for `language`, which need not be the selected one.
    pub fn flags_for(&self, language: Language) -> &[String] {
        match self.flags_by_language.get(&language) {
            Some(flags) => flags,
            None if language == self.language => &self.extra_flags,
            None => &[],
        }
    }

    /// Language and flags for a source file. The extension decides the
    /// language, so a `.py` file never goes through `g++` just because C++
    /// is selected; the selected language only covers unknown extensions.
    pub fn toolchain_for(&self, file: &Path) -> Toolchain {
        let language = Language::from_path(file).unwrap_or(self.language);
        Toolchain {
            language,
            flags: self.flags_for(language).to_vec(),
        }
    }

    /// The Codeforces `programTypeId` to submit `toolchain` as.
    pub fn cf_id(&self, toolchain: &Toolchain) -> String {
        self.cf_language_ids
            .get(&toolchain.language)
            .map(|id| id.trim())
            .filter(|id| !id.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| toolchain.language.cf_id(&toolchain.flags).to_string())
    }

    /// The local time limit for a problem in ms, after the multiplier.
    pub fn time_limit_ms(&self, problem: &ActiveProblem) -> u64 {
        let multiplier = self.time_limit_multiplier;
        let multiplier = if multiplier.is_finite() && multiplier > 0.0 {
            multiplier
        } else {
            1.0
        };
        ((problem.effective_time_limit() as f64) * multiplier)
            .round()
            .max(1.0) as u64
    }
}

/// The language a file is built with and the user's flags for it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Toolchain {
    pub language: Language,
    pub flags: Vec<String>,
}

/// How a program's output is compared with the expected answer.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Default)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum Checker {
    /// Whitespace-separated tokens must match exactly (the judge default).
    #[default]
    Tokens,
    /// Lines must match, ignoring trailing spaces and trailing blank lines.
    Lines,
    /// Tokens must match, ignoring ASCII case (`YES` == `yes`).
    CaseInsensitive,
    /// Numeric tokens may differ by `epsilon`, absolute or relative.
    Float {
        #[serde(default = "default_epsilon")]
        epsilon: f64,
    },
    /// A testlib-style checker run as `<command> <input> <output> <answer>`;
    /// exit code 0 means accepted.
    Custom { command: String },
}

fn default_epsilon() -> f64 {
    1e-6
}

#[derive(Debug, Clone)]
pub struct SubmitConfig {
    pub language: Language,
    /// Codeforces `programTypeId`, resolved from the flags and overrides.
    pub language_id: String,
    pub url: String,
    pub file_path: PathBuf,
}

pub enum RunnerCommand {
    RunCode {
        problem: ActiveProblem,
        run_id: u64,
        cancel: Arc<AtomicBool>,
    },
    /// Compile the solution and keep it running so the UI can type to it.
    /// Used when a received problem has no sample cases.
    Interact {
        problem: ActiveProblem,
        session_id: u64,
        cancel: Arc<AtomicBool>,
        input: Receiver<SessionInput>,
        /// Set by the runner once the child is alive, so the UI can kill the
        /// process group synchronously on stop/quit.
        pgid: Arc<AtomicI32>,
    },
}

/// A single thing the user can send to a live interactive program.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionInput {
    /// A complete line, without its trailing newline.
    Line(String),
    /// End of input (Ctrl-D).
    Eof,
    /// Interrupt the foreground process group (Ctrl-C).
    Interrupt,
}

/// Where a live interactive-session chunk came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionStream {
    Status,
    Out,
    Err,
}

#[derive(Debug, VariantName, Clone)]
pub enum Verdict {
    /// The program exited normally; `accepted` is the checker's decision and
    /// `note` its explanation, if it gave one.
    Success {
        output: Output,
        /// CPU time (user + system) in ms, which is what judges measure.
        time: u128,
        memory_kb: Option<u64>,
        accepted: bool,
        note: Option<String>,
    },
    /// Over the time limit. `killed` means it was still running at the hard
    /// cap, so `time` is a lower bound; otherwise it is the real CPU time.
    TimeLimitExceeded {
        time: u128,
        limit: u128,
        killed: bool,
        stdout: String,
    },
    /// Printed more than `limit` bytes and was stopped.
    OutputLimitExceeded {
        limit: usize,
    },
    CompilationError {
        error: String,
    },
    /// Non-zero exit or a signal; `stdout` is what it printed before that.
    RuntimeError {
        error: String,
        stdout: String,
    },
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
    /// Bytes from a live interactive session. `text` may be a partial line.
    SessionChunk {
        session_id: u64,
        stream: SessionStream,
        text: String,
    },
    /// The interactive process finished, was stopped, or failed to start.
    SessionEnded {
        session_id: u64,
        message: String,
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
    /// The problem listener could not start (port taken, permissions).
    ListenerDown(String),
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
    fn legacy_theme_names_still_load() {
        use crate::components::theme::ThemeName;
        let json = r#"{"language":"Cpp","theme":"Mocha"}"#;
        let config: UserConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.language, Language::Cpp);
        assert_eq!(config.theme, ThemeName::Catppuccin);

        let config: UserConfig = serde_json::from_str(r#"{"language":"Cpp"}"#).unwrap();
        assert_eq!(config.theme, ThemeName::Prsweet, "prsweet is the default");
    }

    #[test]
    fn legacy_fallback_flags_become_the_current_language_flags() {
        let json = r#"{"language":"Python","fallback_flags":["-u"]}"#;
        let mut config: UserConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.flags_for(Language::Python), ["-u"]);

        config.normalize();
        assert_eq!(config.extra_flags, ["-u"]);
        assert_eq!(config.flags_by_language[&Language::Python], ["-u"]);
        assert_eq!(config.flags_for(Language::Python), ["-u"]);
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
        assert_eq!(config.flags_for(Language::Python), ["-B"]);
    }

    #[test]
    fn the_file_extension_picks_the_toolchain() {
        let config = UserConfig {
            language: Language::Cpp,
            extra_flags: vec!["-O2".into()],
            flags_by_language: HashMap::from([
                (Language::Cpp, vec!["-O2".into()]),
                (Language::Python, vec!["-B".into()]),
            ]),
            ..Default::default()
        };

        let python = config.toolchain_for(Path::new("/tmp/A.py"));
        assert_eq!(python.language, Language::Python);
        assert_eq!(python.flags, ["-B"]);

        let java = config.toolchain_for(Path::new("/tmp/Main.java"));
        assert_eq!(java.language, Language::Java);
        assert!(java.flags.is_empty(), "C++ flags must not leak into javac");

        let cpp = config.toolchain_for(Path::new("/tmp/A.cc"));
        assert_eq!(cpp.language, Language::Cpp);
        assert_eq!(cpp.flags, ["-O2"]);

        // Unknown extensions fall back to the selected language.
        assert_eq!(
            config.toolchain_for(Path::new("/tmp/A.txt")).language,
            Language::Cpp
        );
    }

    #[test]
    fn language_ids_can_be_overridden() {
        let mut config = UserConfig::default();
        let python = Toolchain {
            language: Language::Python,
            flags: Vec::new(),
        };
        assert_eq!(config.cf_id(&python), "70");
        config
            .cf_language_ids
            .insert(Language::Python, " 31 ".into());
        assert_eq!(config.cf_id(&python), "31");

        let json = r#"{"language":"Cpp","cf_language_ids":{"Java":"36"}}"#;
        let config: UserConfig = serde_json::from_str(json).unwrap();
        let java = Toolchain {
            language: Language::Java,
            flags: Vec::new(),
        };
        assert_eq!(config.cf_id(&java), "36");
    }

    #[test]
    fn new_settings_have_defaults_and_parse() {
        let config: UserConfig = serde_json::from_str(r#"{"language":"Cpp"}"#).unwrap();
        assert_eq!(config.checker, Checker::Tokens);
        assert_eq!(config.time_limit_multiplier, 1.0);

        let json = r#"{"checker":{"mode":"float","epsilon":1e-9},"time_limit_multiplier":1.5}"#;
        let config: UserConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.checker, Checker::Float { epsilon: 1e-9 });

        let problem = ActiveProblem {
            name: "A".into(),
            url: String::new(),
            time_limit: 2000,
            test_cases: Vec::new(),
            code_file: PathBuf::new(),
            checker: None,
        };
        assert_eq!(config.time_limit_ms(&problem), 3000);

        let custom: Checker =
            serde_json::from_str(r#"{"mode":"custom","command":"./check"}"#).unwrap();
        assert_eq!(
            custom,
            Checker::Custom {
                command: "./check".into()
            }
        );
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
        assert_eq!(Language::Python.cf_id(&["-std=c++20".into()]), "70");
        assert_eq!(Language::Java.cf_id(&[]), "87");
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
