use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use crate::{
    core::{ActiveProblem, Language, UserConfig},
    error::AppError,
};

const DEFAULT_CPP_BOILERPLATE: &str = r#"#include <bits/stdc++.h>
using namespace std;
#define all(x) (x).begin(), (x).end()
#define rall(x) (x).rbegin(), (x).rend()
#define int long long

void solve()
{
    
}

signed main()
{
    ios_base::sync_with_stdio(false);
    cin.tie(nullptr);
    cout.tie(nullptr);

    int tt = 1;
    // cin >> tt;
    while (tt--) solve();
}
"#;
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

    /// v1-compatible problem naming: spaces and dots become `_`, any other
    /// punctuation is dropped, everything else is kept as-is (no collapsing),
    /// so `A. ADjacent` -> `A__ADjacent` exactly like the v1 `.testcases` era.
    pub fn sanitize_name(name: &str) -> String {
        let clean: String = name
            .chars()
            .map(|c| if c == ' ' || c == '.' { '_' } else { c })
            .filter(|c| c.is_alphanumeric() || *c == '_')
            .collect();
        if clean.is_empty() {
            "problem".to_string()
        } else {
            clean
        }
    }

    /// The stem used for this problem's `.json`, binary, and source file.
    ///
    /// Sanitized names alone are not unique: `A. Sum` exists in every contest,
    /// so two problems would share (and overwrite) the same state. When the
    /// plain stem already belongs to a different URL, append a short stable
    /// hash of that URL. Re-receiving the same problem reuses its suffix.
    pub fn claim_problem_stem(&self, name: &str, url: &str, code_dir: &Path) -> String {
        let base = Self::sanitize_name(name);

        // Anonymous payloads have no URL to tell them apart, so a second task
        // with the same name must get its own numbered stem rather than
        // overwriting the first. Two different anonymous tasks are not
        // duplicates, even when their names match.
        if url.is_empty() {
            if !self.stem_taken(&base, code_dir) {
                return base;
            }
            for counter in 2.. {
                let candidate = format!("{base}__{counter}");
                if !self.stem_taken(&candidate, code_dir) {
                    return candidate;
                }
            }
        }

        if !self.stem_conflicts(&base, url, code_dir) {
            return base;
        }

        let suffix = short_hash(url);
        let candidate = format!("{base}__{suffix}");
        if !self.stem_conflicts(&candidate, url, code_dir) {
            return candidate;
        }

        // Same URL hash but a different stored problem (should be impossible
        // unless files were copied by hand): keep looking for a free slot.
        for counter in 2.. {
            let candidate = format!("{base}__{suffix}_{counter}");
            if !self.stem_conflicts(&candidate, url, code_dir) {
                return candidate;
            }
        }
        unreachable!()
    }

    fn stem_taken(&self, stem: &str, code_dir: &Path) -> bool {
        if self.tc_dir.join(format!("{stem}.json")).exists()
            || self.binary_dir.join(stem).exists()
            || code_dir.join(format!("{stem}.class")).exists()
        {
            return true;
        }
        ["cpp", "py", "java"]
            .iter()
            .any(|ext| code_dir.join(format!("{stem}.{ext}")).exists())
    }

    fn stem_conflicts(&self, stem: &str, url: &str, code_dir: &Path) -> bool {
        let json = self.tc_dir.join(format!("{stem}.json"));
        if json.exists() {
            return match self.load_problem_by_stem(stem) {
                Ok(problem) => problem.url != url,
                // A corrupt/foreign file is not ours to overwrite.
                Err(_) => true,
            };
        }

        for ext in ["cpp", "py", "java"] {
            let source = code_dir.join(format!("{stem}.{ext}"));
            if let Some(existing) = source_url(&source)
                && existing != url
            {
                return true;
            }
        }
        false
    }

    fn ensure_default_boilerplate(&self) {
        let path = self.root_dir.join("boilerplate.cpp");
        if !path.exists() {
            let _ = fs::write(path, DEFAULT_CPP_BOILERPLATE);
        }
    }

    /// The code template for a language: `~/.zch/boilerplate.<ext>`, else an
    /// embedded default for that language. Another language's template is
    /// never used. `__CLASS__` becomes the class name in every Java template,
    /// since the public class has to match the file name.
    pub fn boilerplate(&self, language: Language, class_name: &str) -> String {
        let candidates: &[&str] = match language {
            Language::Python => &["boilerplate.py"],
            Language::Java => &["boilerplate.java"],
            Language::Cpp => &["boilerplate.cpp", "boilerplate.cc"],
        };

        let custom = candidates.iter().find_map(|name| {
            fs::read_to_string(self.root_dir.join(name))
                .ok()
                .filter(|content| !content.trim().is_empty())
        });
        let template = custom.unwrap_or_else(|| {
            match language {
                Language::Python => DEFAULT_PY_BOILERPLATE,
                Language::Java => DEFAULT_JAVA_BOILERPLATE,
                Language::Cpp => DEFAULT_CPP_BOILERPLATE,
            }
            .to_string()
        });

        match language {
            Language::Java => template.replace("__CLASS__", class_name),
            _ => template,
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
    pub fn write_code_file(
        &self,
        problem: &ActiveProblem,
        code_dir: &Path,
    ) -> Result<PathBuf, AppError> {
        let language = self.load_config().0.language;
        let file_path = code_dir.join(format!("{}.{}", problem.name, language.ext()));
        let write_error =
            |e: std::io::Error| AppError::Io(format!("cannot write {}: {e}", file_path.display()));

        if file_path.exists() {
            if !problem.url.is_empty() {
                let content = fs::read_to_string(&file_path).map_err(|e| {
                    AppError::Io(format!("cannot read {}: {e}", file_path.display()))
                })?;
                let has_url = content
                    .lines()
                    .take(3)
                    .any(|l| l.starts_with("// URL:") || l.starts_with("# URL:"));
                if !has_url {
                    write_atomic(
                        &file_path,
                        format!(
                            "{}{}",
                            Self::url_comment_line(language, &problem.url),
                            content
                        )
                        .as_bytes(),
                    )
                    .map_err(write_error)?;
                }
            }
            return Ok(file_path);
        }

        let mut content = String::new();
        if !problem.url.is_empty() {
            content.push_str(&Self::url_comment_line(language, &problem.url));
        }
        content.push_str(&self.boilerplate(language, &problem.name));
        write_atomic(&file_path, content.as_bytes()).map_err(write_error)?;
        Ok(file_path)
    }

    /// Overwrite the test case database for a problem (`.tc/<name>.json`).
    pub fn save_tc_file(&self, problem: &ActiveProblem) -> Result<PathBuf, AppError> {
        let file_path = self.tc_dir.join(format!("{}.json", problem.name));
        let content = serde_json::to_string_pretty(problem)
            .map_err(|e| AppError::ParseError(e.to_string()))?;
        write_atomic(&file_path, content.as_bytes()).map_err(|e| AppError::Io(e.to_string()))?;
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
        let Ok(entries) = fs::read_dir(&self.binary_dir) else {
            return;
        };
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

    fn config_path(&self) -> PathBuf {
        self.root_dir.join("config.json")
    }

    /// The config, or defaults when there is none. A config that exists but
    /// does not parse is copied to `config.json.bak` first, so falling back
    /// to defaults (and saving them later) can never destroy the user's file.
    pub fn load_config(&self) -> (UserConfig, bool) {
        let file_path = self.config_path();
        if let Ok(file) = fs::read_to_string(&file_path) {
            match serde_json::from_str::<UserConfig>(&file) {
                Ok(mut parsed) => {
                    parsed.normalize();
                    return (parsed, true);
                }
                Err(_) => {
                    let backup = self.root_dir.join("config.json.bak");
                    if fs::read_to_string(&backup).ok().as_deref() != Some(file.as_str()) {
                        let _ = write_atomic(&backup, file.as_bytes());
                    }
                }
            }
        }
        let mut config = UserConfig::default();
        config.normalize();
        (config, false)
    }

    /// Why `config.json` cannot be used, if it exists and is broken.
    pub fn config_error(&self) -> Option<String> {
        let file = fs::read_to_string(self.config_path()).ok()?;
        let error = serde_json::from_str::<UserConfig>(&file).err()?;
        // Loading makes the backup the message points at.
        let _ = self.load_config();
        Some(format!(
            "config.json is invalid ({error}) — using defaults; your file was copied to ~/.zch/config.json.bak"
        ))
    }

    pub fn save_config(&self, config: &UserConfig) -> bool {
        let mut config = config.clone();
        config.normalize();
        match serde_json::to_string_pretty(&config) {
            Ok(content) => write_atomic(&self.config_path(), content.as_bytes()).is_ok(),
            Err(_) => false,
        }
    }
}

/// Write via a temp file in the same directory and rename it into place, so
/// a crash or a full disk never leaves a half-written file behind.
pub fn write_atomic(path: &Path, content: &[u8]) -> std::io::Result<()> {
    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let temp = path.with_file_name(format!(".{file_name}.{}.{nanos}.tmp", std::process::id()));
    let result = (|| {
        let mut file = fs::File::create(&temp)?;
        std::io::Write::write_all(&mut file, content)?;
        file.sync_all()?;
        fs::rename(&temp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

/// Stable across runs (unlike `DefaultHasher`), so a problem keeps its suffix.
fn short_hash(input: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in input.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{:08x}", hash & 0xffff_ffff)
}

/// The `// URL:` / `# URL:` header written into generated source files.
fn source_url(path: &Path) -> Option<String> {
    let content = fs::read_to_string(path).ok()?;
    content.lines().take(3).find_map(|line| {
        line.strip_prefix("// URL: ")
            .or_else(|| line.strip_prefix("# URL: "))
            .map(str::trim)
            .map(str::to_string)
    })
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

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_ops(tag: &str) -> FsOps {
        let root = std::env::temp_dir().join(format!("zcph_fs_{tag}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        FsOps::with_root(root)
    }

    #[test]
    fn each_language_gets_its_own_template() {
        let ops = temp_ops("templates");
        // boilerplate.cpp always exists, and must never leak into other languages.
        assert!(ops.root_dir.join("boilerplate.cpp").exists());
        assert!(ops.boilerplate(Language::Python, "A").contains("def solve"));
        let java = ops.boilerplate(Language::Java, "A__Sum");
        assert!(java.contains("public class A__Sum"), "{java}");

        // A custom Java template gets its class name filled in too.
        fs::write(
            ops.root_dir.join("boilerplate.java"),
            "public class __CLASS__ { }",
        )
        .unwrap();
        assert_eq!(
            ops.boilerplate(Language::Java, "B_Two"),
            "public class B_Two { }"
        );
        let _ = fs::remove_dir_all(&ops.root_dir);
    }

    #[test]
    fn a_broken_config_is_backed_up_not_lost() {
        let ops = temp_ops("broken_config");
        let path = ops.root_dir.join("config.json");
        fs::write(&path, "{ \"language\": \"Cpp\", oops").unwrap();

        assert!(ops.config_error().is_some());
        let (config, loaded) = ops.load_config();
        assert!(!loaded);
        assert_eq!(config.language, Language::Cpp);
        assert_eq!(
            fs::read_to_string(ops.root_dir.join("config.json.bak")).unwrap(),
            "{ \"language\": \"Cpp\", oops"
        );

        // Saving afterwards replaces the broken file; the backup stays.
        assert!(ops.save_config(&config));
        assert!(ops.config_error().is_none());
        assert!(ops.root_dir.join("config.json.bak").exists());
        let _ = fs::remove_dir_all(&ops.root_dir);
    }

    #[test]
    fn atomic_writes_leave_no_temp_files() {
        let ops = temp_ops("atomic");
        let path = ops.root_dir.join("x.json");
        write_atomic(&path, b"one").unwrap();
        write_atomic(&path, b"two").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "two");
        let leftovers = fs::read_dir(&ops.root_dir)
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
            .count();
        assert_eq!(leftovers, 0);
        let _ = fs::remove_dir_all(&ops.root_dir);
    }

    #[test]
    fn unwritable_code_dir_is_an_error() {
        let ops = temp_ops("unwritable");
        let problem = ActiveProblem {
            name: "A".into(),
            url: String::new(),
            time_limit: 0,
            test_cases: Vec::new(),
            code_file: PathBuf::new(),
            checker: None,
        };
        let missing = ops.root_dir.join("does/not/exist");
        assert!(ops.write_code_file(&problem, &missing).is_err());
        let _ = fs::remove_dir_all(&ops.root_dir);
    }
}
