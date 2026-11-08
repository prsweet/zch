use std::{
    fs,
    io::Read,
    path::Path,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use crate::core::Checker;

const CUSTOM_CHECKER_TIMEOUT: Duration = Duration::from_secs(10);
const NOTE_MAX_CHARS: usize = 2_000;

/// What a checker decided about one output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckOutcome {
    pub accepted: bool,
    /// The checker's own explanation (custom checkers only).
    pub note: Option<String>,
}

impl CheckOutcome {
    fn plain(accepted: bool) -> Self {
        Self {
            accepted,
            note: None,
        }
    }
}

/// Judge `got` against `expected`. `scratch` is a writable directory for
/// the files a custom checker reads.
pub fn check(
    checker: &Checker,
    input: &str,
    expected: &str,
    got: &str,
    scratch: &Path,
) -> CheckOutcome {
    match checker {
        Checker::Tokens => CheckOutcome::plain(tokens_eq(expected, got, |a, b| a == b)),
        Checker::CaseInsensitive => {
            CheckOutcome::plain(tokens_eq(expected, got, |a, b| a.eq_ignore_ascii_case(b)))
        }
        Checker::Lines => CheckOutcome::plain(lines_eq(expected, got)),
        Checker::Float { epsilon } => {
            let epsilon = if epsilon.is_finite() && *epsilon >= 0.0 {
                *epsilon
            } else {
                1e-6
            };
            CheckOutcome::plain(tokens_eq(expected, got, |a, b| float_eq(a, b, epsilon)))
        }
        Checker::Custom { command } => run_custom(command, input, expected, got, scratch),
    }
}

fn tokens_eq(expected: &str, got: &str, same: impl Fn(&str, &str) -> bool) -> bool {
    let mut expected = expected.split_ascii_whitespace();
    let mut got = got.split_ascii_whitespace();
    loop {
        match (expected.next(), got.next()) {
            (None, None) => return true,
            (Some(a), Some(b)) if same(a, b) => {}
            _ => return false,
        }
    }
}

/// Line by line, ignoring trailing spaces and leading/trailing blank lines.
fn lines_eq(expected: &str, got: &str) -> bool {
    let mut expected = expected.trim().lines().map(str::trim_end);
    let mut got = got.trim().lines().map(str::trim_end);
    loop {
        match (expected.next(), got.next()) {
            (None, None) => return true,
            (Some(a), Some(b)) if a == b => {}
            _ => return false,
        }
    }
}

/// testlib's `doubleCompare`: equal within `epsilon`, absolute or relative.
/// Tokens that are not both numbers must match exactly.
fn float_eq(expected: &str, got: &str, epsilon: f64) -> bool {
    if expected == got {
        return true;
    }
    let (Ok(a), Ok(b)) = (expected.parse::<f64>(), got.parse::<f64>()) else {
        return false;
    };
    if a.is_nan() || b.is_nan() {
        return false;
    }
    if a.is_infinite() || b.is_infinite() {
        return a == b;
    }
    let diff = (a - b).abs();
    diff <= epsilon + 1e-15 || diff <= epsilon * a.abs() + 1e-15
}

fn run_custom(
    command: &str,
    input: &str,
    expected: &str,
    got: &str,
    scratch: &Path,
) -> CheckOutcome {
    let command = command.trim();
    if command.is_empty() {
        return failed("custom checker has no command");
    }

    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let base = format!("zch_check_{}_{stamp}", std::process::id());
    let input_path = scratch.join(format!("{base}.in"));
    let output_path = scratch.join(format!("{base}.out"));
    let answer_path = scratch.join(format!("{base}.ans"));
    let paths = [&input_path, &output_path, &answer_path];

    let written = fs::write(&input_path, input)
        .and_then(|_| fs::write(&output_path, got))
        .and_then(|_| fs::write(&answer_path, expected));
    let outcome = match written {
        Err(error) => failed(&format!("cannot write checker files: {error}")),
        Ok(()) => spawn_custom(command, &input_path, &output_path, &answer_path),
    };
    for path in paths {
        let _ = fs::remove_file(path);
    }
    outcome
}

fn spawn_custom(command: &str, input: &Path, output: &Path, answer: &Path) -> CheckOutcome {
    // Through `sh` so the command can carry its own arguments and `~`.
    let child = Command::new("sh")
        .arg("-c")
        .arg(format!("{command} \"$@\""))
        .arg("sh")
        .arg(input)
        .arg(output)
        .arg(answer)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();
    let mut child = match child {
        Ok(child) => child,
        Err(error) => return failed(&format!("cannot start checker `{command}`: {error}")),
    };

    let mut stdout = child.stdout.take();
    let mut stderr = child.stderr.take();
    let stdout_reader = thread::spawn(move || read_capped(stdout.as_mut()));
    let stderr_reader = thread::spawn(move || read_capped(stderr.as_mut()));

    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if started.elapsed() >= CUSTOM_CHECKER_TIMEOUT => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            Ok(None) => thread::sleep(Duration::from_millis(2)),
            Err(_) => break None,
        }
    };
    let message = [
        stderr_reader.join().unwrap_or_default(),
        stdout_reader.join().unwrap_or_default(),
    ]
    .iter()
    .map(|text| text.trim())
    .filter(|text| !text.is_empty())
    .collect::<Vec<_>>()
    .join("\n");

    match status {
        None => failed(&format!(
            "checker timed out after {}s",
            CUSTOM_CHECKER_TIMEOUT.as_secs()
        )),
        Some(status) => {
            let accepted = status.success();
            let note = if message.is_empty() {
                (!accepted).then(|| format!("checker: {status}"))
            } else {
                Some(format!("checker: {}", truncate(&message)))
            };
            CheckOutcome { accepted, note }
        }
    }
}

fn read_capped<R: Read>(reader: Option<&mut R>) -> String {
    let Some(reader) = reader else {
        return String::new();
    };
    let mut buffer = Vec::new();
    let _ = reader.take(64 * 1024).read_to_end(&mut buffer);
    // Keep draining so a chatty checker never blocks on a full pipe.
    let _ = std::io::copy(reader, &mut std::io::sink());
    String::from_utf8_lossy(&buffer).into_owned()
}

fn truncate(text: &str) -> String {
    if text.chars().count() <= NOTE_MAX_CHARS {
        return text.to_string();
    }
    let mut out: String = text.chars().take(NOTE_MAX_CHARS).collect();
    out.push('…');
    out
}

fn failed(message: &str) -> CheckOutcome {
    CheckOutcome {
        accepted: false,
        note: Some(format!("checker: {message}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn judge(checker: &Checker, expected: &str, got: &str) -> bool {
        check(checker, "", expected, got, &std::env::temp_dir()).accepted
    }

    #[test]
    fn tokens_ignore_whitespace_layout() {
        assert!(judge(&Checker::Tokens, "1 2\n3", "1  2 3\n\n"));
        assert!(judge(&Checker::Tokens, "", ""));
        assert!(!judge(&Checker::Tokens, "1 2", "1 2 3"));
        assert!(!judge(&Checker::Tokens, "YES", "yes"));
        // Output against an empty expected answer is a mismatch, not a pass.
        assert!(!judge(&Checker::Tokens, "", "lkej"));
    }

    #[test]
    fn lines_keep_line_structure() {
        assert!(judge(&Checker::Lines, "1 2\n3\n", "1 2  \n3"));
        assert!(!judge(&Checker::Lines, "1 2\n3", "1 2 3"));
        assert!(!judge(&Checker::Lines, "1 2", "1  2"));
    }

    #[test]
    fn case_insensitive_tokens() {
        assert!(judge(&Checker::CaseInsensitive, "YES\nNo", "yes no"));
        assert!(!judge(&Checker::CaseInsensitive, "YES", "YE"));
    }

    #[test]
    fn float_tokens_use_absolute_or_relative_error() {
        let checker = Checker::Float { epsilon: 1e-6 };
        assert!(judge(&checker, "0.3333333", "0.33333331"));
        assert!(judge(&checker, "1000000000", "1000000500"));
        assert!(!judge(&checker, "0.5", "0.51"));
        assert!(judge(&checker, "case 1.0", "case 1"));
        assert!(!judge(&checker, "abc", "abd"));
        assert!(!judge(&checker, "1.5", "nan"));
    }

    #[cfg(unix)]
    #[test]
    fn custom_checker_uses_exit_code_and_message() {
        // testlib order: input, output, answer.
        let compare = Checker::Custom {
            command: r#"f() { cmp -s "$2" "$3"; }; f"#.into(),
        };
        let outcome = check(&compare, "in", "42\n", "42\n", &std::env::temp_dir());
        assert!(outcome.accepted, "{outcome:?}");
        let outcome = check(&compare, "in", "42\n", "43\n", &std::env::temp_dir());
        assert!(!outcome.accepted);

        let reject = Checker::Custom {
            command: "echo 'wrong answer' >&2; false".into(),
        };
        let outcome = check(&reject, "in", "1", "2", &std::env::temp_dir());
        assert!(!outcome.accepted);
        assert_eq!(outcome.note.as_deref(), Some("checker: wrong answer"));
    }
}
