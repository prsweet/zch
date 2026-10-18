use std::{fs, path::{Path, PathBuf}, time::{SystemTime, UNIX_EPOCH}};

use crate::{core::{ActiveProblem, Language, UserConfig}, error::AppError};

const DEFAULT_CPP_BOILERPLATE: &str = include_str!("../../boilerplate.cpp");
const DEFAULT_PY_BOILERPLATE: &str =
    "import sys\n\n\ndef solve():\n    pass\n\n\nif __name__ == \"__main__\":\n    solve()\n";
const DEFAULT_JAVA_BOILERPLATE: &str = "import java.util.*;\nimport java.io.*;\n\npublic class __CLASS__ {\n    public static void main(String[] args) {\n        Scanner sc = new Scanner(System.in);\n    }\n}\n";

#[derive(Debug, Clone)]
pub struct FsOps {
    pub root_dir: PathBuf,
    pub tc_dir: PathBuf,
    pub binary_dir: PathBuf,
}

impl FsOps {
    pub fn new() -> Self {
        let home = dirs::home_dir().expect("Could not find home directory");
        let root = home.join(".zch");
        if !root.exists() {
            migrate_legacy_root(&home.join(".zed-cp-helper"), &root);
        }
        Self::with_root(root)
    }

    pub fn with_root(root_dir: PathBuf) -> Self {
        let tc_dir = root_dir.join(".tc");
        let binary_dir = root_dir.join(".bin");
        let _ = fs::create_dir_all(&tc_dir);
        let _ = fs::create_dir_all(&binary_dir);

        let this = Self {
            tc_dir,
            binary_dir,
            root_dir,
        };
        this.ensure_default_boilerplate();
        this
    }

    pub fn sanitize_name(name: &str) -> String {
        let mut clean: String = name
            .chars()
            .map(|c| if c.is_alphanumeric() || c == '_' { c } else { '_' })
            .collect();
        while clean.contains("__") {
            clean = clean.replace("__", "_");
        }
        let clean = clean.trim_matches('_').to_string();
        if clean.is_empty() { "problem".to_string() } else { clean }
    }

    fn ensure_default_boilerplate(&self) {
        let path = self.root_dir.join("boilerplate.cpp");
        if !path.exists() {
            let _ = fs::write(path, DEFAULT_CPP_BOILERPLATE);
        }
    }

    /// The code template for a language. Prefers `~/.zch/boilerplate.<ext>`,
    /// falls back to the C++ boilerplate, then to an embedded default.
    pub fn boilerplate(&self, language: Language, class_name: &str) -> String {
        let candidates = match language {
            Language::Python => vec![
                self.root_dir.join("boilerplate.py"),
                self.root_dir.join("boilerplate.cpp"),
            ],
            Language::Java => vec![
                self.root_dir.join("boilerplate.java"),
                self.root_dir.join("boilerplate.cpp"),
            ],
            _ => vec![
                self.root_dir.join("boilerplate.cpp"),
                self.root_dir.join("boilerplate.cc"),
            ],
        };

        for path in candidates {
            if let Ok(content) = fs::read_to_string(&path)
                && !content.trim().is_empty() {
                    return content;
                }
        }

        match language {
            Language::Python => DEFAULT_PY_BOILERPLATE.to_string(),
            Language::Java => DEFAULT_JAVA_BOILERPLATE.replace("__CLASS__", class_name),
            _ => DEFAULT_CPP_BOILERPLATE.to_string(),
        }
    }

    fn url_comment_line(language: Language, url: &str) -> String {
        match language {
            Language::Python => format!("# URL: {url}\n"),
            _ => format!("// URL: {url}\n"),
        }
    }

    /// Create the source file if missing (boilerplate + URL header), otherwise
    /// make sure an existing file carries the URL header. Returns the file path.
    pub fn write_code_file(&self, problem: &ActiveProblem, code_dir: &Path) -> PathBuf {
        let language = self.load_config().0.language;
        let file_path = code_dir.join(format!("{}.{}", problem.name, language.ext()));

        if file_path.exists() {
            if !problem.url.is_empty()
                && let Ok(content) = fs::read_to_string(&file_path) {
                    let has_url = content
                        .lines()
                        .take(3)
                        .any(|l| l.starts_with("// URL:") || l.starts_with("# URL:"));
                    if !has_url {
                        let _ = fs::write(
                            &file_path,
                            format!("{}{}", Self::url_comment_line(language, &problem.url), content),
                        );
                    }
                }
            return file_path;
        }

        let mut content = String::new();
        if !problem.url.is_empty() {
            content.push_str(&Self::url_comment_line(language, &problem.url));
        }
        content.push_str(&self.boilerplate(language, &problem.name));
        let _ = fs::write(&file_path, content);
        file_path
    }

    /// Overwrite the test case database for a problem (`.tc/<name>.json`).
    pub fn save_tc_file(&self, problem: &ActiveProblem) -> Result<PathBuf, AppError> {
        let file_path = self.tc_dir.join(format!("{}.json", problem.name));
        let content = serde_json::to_string_pretty(problem)
            .map_err(|e| AppError::ParseError(e.to_string()))?;
        fs::write(&file_path, content).map_err(|e| AppError::Io(e.to_string()))?;
        Ok(file_path)
    }

    /// Load a problem database by file stem (e.g. `E_Prime_Destruction.json`).
    pub fn load_problem_by_stem(&self, stem: &str) -> Result<ActiveProblem, AppError> {
        let file_path = self.tc_dir.join(format!("{stem}.json"));
        if !file_path.exists() {
            return Err(AppError::FileNotFound(file_path));
        }
        let content = fs::read_to_string(&file_path).map_err(|e| AppError::Io(e.to_string()))?;
        serde_json::from_str(&content).map_err(|e| AppError::ParseError(e.to_string()))
    }

    /// Delete compiled binaries older than 24h.
    pub fn prune_binaries(&self) {
        let Ok(entries) = fs::read_dir(&self.binary_dir) else { return };
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        for entry in entries.flatten() {
            let Ok(meta) = entry.metadata() else { continue };
            let Ok(mtime) = meta.modified() else { continue };
            let age = now.saturating_sub(
                mtime
                    .duration_since(UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(now),
            );
            if age > 86_400 {
                let _ = fs::remove_file(entry.path());
            }
        }
    }

    pub fn load_config(&self) -> (UserConfig, bool) {
        let file_path = self.root_dir.join("config.json");
        if let Ok(file) = fs::read_to_string(&file_path)
            && let Ok(parsed) = serde_json::from_str::<UserConfig>(&file) {
                return (parsed, true);
            }
        (UserConfig::default(), false)
    }

    pub fn save_config(&self, config: &UserConfig) -> bool {
        let file_path = self.root_dir.join("config.json");
        match serde_json::to_string_pretty(config) {
            Ok(content) => fs::write(file_path, content).is_ok(),
            Err(_) => false,
        }
    }
}

/// One-time move from the old `~/.zed-cp-helper` layout (best effort).
fn migrate_legacy_root(legacy: &Path, root: &Path) {
    if !legacy.is_dir() {
        return;
    }
    let _ = fs::create_dir_all(root);
    let moves = [
        (".testcases", ".tc"),
        (".binaries", ".bin"),
        ("config.json", "config.json"),
        ("boilerplate.cpp", "boilerplate.cpp"),
        ("boilerplate.cc", "boilerplate.cc"),
        ("boilerplate.py", "boilerplate.py"),
        ("boilerplate.java", "boilerplate.java"),
    ];
    for (from, to) in moves {
        let source = legacy.join(from);
        let destination = root.join(to);
        if source.exists() && !destination.exists() {
            let _ = fs::rename(&source, &destination);
        }
    }
}
