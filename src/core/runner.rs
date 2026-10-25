use std::{
    collections::HashMap,
    fs,
    hash::{DefaultHasher, Hash, Hasher},
    path::{Path, PathBuf},
    process::Output,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{Sender, channel},
    },
    thread,
    time::{Duration, Instant, SystemTime},
};

use crate::core::{
    ActiveProblem, HelperCommand, Language, PassingCommand, RunnerCommand, UserConfig, Verdict,
    fs_ops::FsOps,
};

const POLL_INTERVAL: Duration = Duration::from_millis(1);
const MAX_CACHE_ENTRIES: usize = 128;

#[derive(Debug, PartialEq, Eq, Hash)]
struct CacheKey {
    file: PathBuf,
    source_hash: u64,
    language: Language,
    flags: Vec<String>,
    binary: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ArtifactStamp {
    modified: SystemTime,
    len: u64,
}

struct CacheEntry {
    artifact: Option<ArtifactStamp>,
}

enum Compile {
    NotNeeded,
    Skipped,
    Compiled(Duration),
    Cancelled,
}

pub fn spawn_runner(main_tx: Sender<PassingCommand>, fs_ops: FsOps) -> Sender<RunnerCommand> {
    let (tx, rx) = channel::<RunnerCommand>();
    thread::spawn(move || {
        let mut cache: HashMap<CacheKey, CacheEntry> = HashMap::new();
        while let Ok(RunnerCommand::RunCode { problem, run_id, cancel }) = rx.recv() {
            run_streaming(&problem, run_id, &fs_ops, &main_tx, &mut cache, &cancel);
        }
    });
    tx
}

/// Compiles once (when the source or flags changed), then streams every verdict
/// to the UI as soon as the case finishes.
fn run_streaming(
    problem: &ActiveProblem,
    run_id: u64,
    fs_ops: &FsOps,
    tx: &Sender<PassingCommand>,
    cache: &mut HashMap<CacheKey, CacheEntry>,
    cancel: &AtomicBool,
) {
    let (config, _) = fs_ops.load_config();
    let case_count = problem.test_cases.len().max(1);
    let started = Instant::now();
    let cancelled = || cancel.load(Ordering::SeqCst);
    let finish = || {
        let _ = tx.send(PassingCommand::ToHelper(HelperCommand::RunFinished {
            run_id,
            elapsed_ms: started.elapsed().as_millis(),
        }));
    };

    if cancelled() {
        finish();
        return;
    }

    let phase = |message: String| {
        let _ = tx.send(PassingCommand::ToHelper(HelperCommand::RunPhase { run_id, message }));
    };

    let compiles = config
        .language
        .compile_cmd(&problem.code_file, &fs_ops.binary_dir.join(&problem.name), config.active_flags())
        .is_some();
    if compiles {
        phase("compiling…".to_string());
    }

    match compile(problem, &config, fs_ops, cache, cancel) {
        Err(error) => {
            for index in 0..case_count {
                let sent = tx.send(PassingCommand::ToHelper(HelperCommand::CaseResult {
                    run_id,
                    index,
                    verdict: Verdict::CompilationError { error: error.clone() },
                }));
                if sent.is_err() {
                    return;
                }
            }
            finish();
            return;
        }
        Ok(Compile::Cancelled) => {
            finish();
            return;
        }
        Ok(Compile::Compiled(took)) => phase(format!("compiled in {}ms", took.as_millis())),
        Ok(Compile::Skipped) => phase("sources unchanged — compile skipped".to_string()),
        Ok(Compile::NotNeeded) => {}
    }

    let binary_path = fs_ops.binary_dir.join(&problem.name);
    let (program, args) = config.language.run_cmd(&problem.code_file, &binary_path);
    let time_limit = Duration::from_millis(problem.effective_time_limit());

    for (index, tc) in problem.test_cases.iter().enumerate() {
        if cancelled() {
            finish();
            return;
        }
        phase(format!("running case {}/{}…", index + 1, case_count));
        let Some(verdict) = run_single(&program, &args, &tc.input, time_limit, cancel) else {
            finish();
            return;
        };
        let sent = tx.send(PassingCommand::ToHelper(HelperCommand::CaseResult {
            run_id,
            index,
            verdict,
        }));
        if sent.is_err() {
            return;
        }
    }

    finish();
}

fn cache_key(
    problem: &ActiveProblem,
    config: &UserConfig,
    fs_ops: &FsOps,
) -> std::io::Result<CacheKey> {
    let source = fs::read(&problem.code_file)?;
    let mut hasher = DefaultHasher::new();
    source.hash(&mut hasher);
    Ok(CacheKey {
        file: problem.code_file.clone(),
        source_hash: hasher.finish(),
        language: config.language,
        flags: config.active_flags().to_vec(),
        binary: fs_ops.binary_dir.join(&problem.name),
    })
}

fn compile(
    problem: &ActiveProblem,
    config: &UserConfig,
    fs_ops: &FsOps,
    cache: &mut HashMap<CacheKey, CacheEntry>,
    cancel: &AtomicBool,
) -> Result<Compile, String> {
    if cancel.load(Ordering::SeqCst) {
        return Ok(Compile::Cancelled);
    }
    if !problem.code_file.exists() {
        return Err(format!("source file not found: {}", problem.code_file.display()));
    }

    let binary_path = fs_ops.binary_dir.join(&problem.name);
    let Some((program, args)) =
        config.language.compile_cmd(&problem.code_file, &binary_path, config.active_flags())
    else {
        return Ok(Compile::NotNeeded);
    };
    let artifact_path = artifact_path(problem, config.language, &binary_path);

    let key = cache_key(problem, config, fs_ops)
        .map_err(|e| format!("cannot read {}: {e}", problem.code_file.display()))?;
    if let Some(entry) = cache.get(&key) {
        let current = artifact_path.as_deref().and_then(artifact_stamp);
        if entry.artifact.is_some() && entry.artifact == current {
            return Ok(Compile::Skipped);
        }
        cache.remove(&key);
    }

    let start = Instant::now();
    let Some(output) = wait_for_command(&program, &args, cancel)? else {
        return Ok(Compile::Cancelled);
    };
    if !output.status.success() {
        let mut error = String::from_utf8_lossy(&output.stderr).to_string();
        if error.trim().is_empty() {
            error = String::from_utf8_lossy(&output.stdout).to_string();
        }
        return Err(error.trim_end().to_string());
    }

    let artifact = artifact_path.as_deref().and_then(artifact_stamp);
    if artifact.is_some() {
        if cache.len() >= MAX_CACHE_ENTRIES {
            cache.clear();
        }
        cache.insert(key, CacheEntry { artifact });
    }
    Ok(Compile::Compiled(start.elapsed()))
}

fn artifact_path(
    problem: &ActiveProblem,
    language: Language,
    binary_path: &Path,
) -> Option<PathBuf> {
    match language {
        Language::Cpp => Some(binary_path.to_path_buf()),
        Language::Java => Some(problem.code_file.with_extension("class")),
        Language::Python => None,
    }
}

fn artifact_stamp(path: &Path) -> Option<ArtifactStamp> {
    let metadata = fs::metadata(path).ok()?;
    Some(ArtifactStamp {
        modified: metadata.modified().ok()?,
        len: metadata.len(),
    })
}

/// Start a child in its own process group on Unix, so cancelling can take
/// down compiler drivers and their children instead of only the direct child.
fn command(program: &str, args: &[String]) -> duct::Expression {
    let expression = duct::cmd(program, args);
    #[cfg(unix)]
    let expression = {
        use std::os::unix::process::CommandExt;
        expression.before_spawn(|cmd| {
            cmd.process_group(0);
            Ok(())
        })
    };
    expression
}

fn stop(handle: &duct::Handle) {
    #[cfg(unix)]
    if let Some(pid) = handle.pids().first() {
        let _ = std::process::Command::new("kill")
            .args(["-KILL", &format!("-{pid}")])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }
    let _ = handle.kill();
    let _ = handle.wait();
}

/// Run a blocking compiler while still honouring the cancel flag.
fn wait_for_command(
    program: &str,
    args: &[String],
    cancel: &AtomicBool,
) -> Result<Option<Output>, String> {
    let handle = command(program, args)
        .stdout_capture()
        .stderr_capture()
        .unchecked()
        .start()
        .map_err(|e| format!("failed to start {program}: {e}"))?;

    loop {
        match handle.try_wait() {
            Ok(Some(output)) => return Ok(Some(output.clone())),
            Ok(None) => {
                if cancel.load(Ordering::SeqCst) {
                    stop(&handle);
                    return Ok(None);
                }
                thread::sleep(POLL_INTERVAL);
            }
            Err(e) => return Err(format!("failed to read {program} status: {e}")),
        }
    }
}

fn run_single(
    program: &str,
    args: &[String],
    input: &str,
    time_limit: Duration,
    cancel: &AtomicBool,
) -> Option<Verdict> {
    if cancel.load(Ordering::SeqCst) {
        return None;
    }

    let mut stdin = input.to_string();
    if !stdin.is_empty() && !stdin.ends_with('\n') {
        stdin.push('\n');
    }

    let start = Instant::now();
    let handle = match command(program, args)
        .stdin_bytes(stdin.into_bytes())
        .stdout_capture()
        .stderr_capture()
        .unchecked()
        .start()
    {
        Ok(handle) => handle,
        Err(e) => {
            return Some(Verdict::RuntimeError {
                error: format!("failed to start {program}: {e}"),
            });
        }
    };

    loop {
        match handle.try_wait() {
            Ok(Some(output)) => {
                let time = start.elapsed().as_millis();
                return Some(if output.status.success() {
                    Verdict::Success { output: output.clone(), time }
                } else {
                    let stderr = String::from_utf8_lossy(&output.stderr).trim_end().to_string();
                    let error = if stderr.is_empty() {
                        format!("exited with {}", output.status)
                    } else {
                        stderr
                    };
                    Verdict::RuntimeError { error }
                });
            }
            Ok(None) => {
                if cancel.load(Ordering::SeqCst) {
                    stop(&handle);
                    return None;
                }
                if start.elapsed() >= time_limit {
                    stop(&handle);
                    return Some(Verdict::TimeLimitExceeded);
                }
                thread::sleep(POLL_INTERVAL);
            }
            Err(e) => {
                return Some(Verdict::RuntimeError {
                    error: format!("failed to read process status: {e}"),
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{Language, PassingCommand, ReceivingTestCase};
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{Receiver, channel},
    };
    use std::time::Instant;

    fn collect_run(
        main_rx: &Receiver<PassingCommand>,
        run_id: u64,
        timeout: Duration,
    ) -> (Vec<String>, Vec<Verdict>) {
        let mut phases = Vec::new();
        let mut verdicts = Vec::new();
        while let Ok(message) = main_rx.recv_timeout(timeout) {
            if let PassingCommand::ToHelper(command) = message {
                match command {
                    HelperCommand::RunPhase { run_id: id, message } if id == run_id => {
                        phases.push(message);
                    }
                    HelperCommand::CaseResult { run_id: id, verdict, .. } if id == run_id => {
                        verdicts.push(verdict);
                    }
                    HelperCommand::RunFinished { run_id: id, .. } if id == run_id => break,
                    _ => {}
                }
            }
        }
        (phases, verdicts)
    }

    #[test]
    fn compile_cache_key_tracks_source_flags_and_language() {
        let root = std::env::temp_dir().join(format!("zcph_runner_cache_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let file = root.join("A.cpp");
        std::fs::write(&file, "int main() {}").unwrap();
        let fs_ops = FsOps::with_root(root.clone());

        let problem = ActiveProblem {
            name: "A".into(),
            url: String::new(),
            time_limit: 1000,
            test_cases: Vec::new(),
            code_file: file.clone(),
        };
        let cpp = UserConfig { language: Language::Cpp, extra_flags: vec!["-O2".into()], ..Default::default() };
        let first = cache_key(&problem, &cpp, &fs_ops).unwrap();
        assert_eq!(first, cache_key(&problem, &cpp, &fs_ops).unwrap());

        let mut other_flags = cpp.clone();
        other_flags.extra_flags.push("-std=c++20".into());
        assert_ne!(first, cache_key(&problem, &other_flags, &fs_ops).unwrap());

        std::fs::write(&file, "int main() { return 0; }").unwrap();
        assert_ne!(first, cache_key(&problem, &cpp, &fs_ops).unwrap());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn second_run_reuses_the_compiled_binary() {
        if std::process::Command::new("g++").arg("--version").output().is_err() {
            return;
        }

        let root = std::env::temp_dir().join(format!("zcph_runner_compile_cache_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let fs_ops = FsOps::with_root(root.clone());
        assert!(fs_ops.save_config(&UserConfig { language: Language::Cpp, extra_flags: vec![], ..Default::default() }));

        let code_file = root.join("Add.cpp");
        std::fs::write(
            &code_file,
            "#include <iostream>\nint main() { long long a, b; std::cin >> a >> b; std::cout << a + b << '\\n'; }\n",
        )
        .unwrap();

        let problem = ActiveProblem {
            name: "Add".into(),
            url: String::new(),
            time_limit: 3000,
            test_cases: vec![ReceivingTestCase { input: "1 2".into(), expected_output: "3".into() }],
            code_file,
        };

        let (main_tx, main_rx) = channel();
        let runner_tx = spawn_runner(main_tx, fs_ops);

        let collect = |run_id: u64| {
            runner_tx
                .send(RunnerCommand::RunCode {
                    problem: problem.clone(),
                    run_id,
                    cancel: Arc::new(AtomicBool::new(false)),
                })
                .unwrap();
            collect_run(&main_rx, run_id, Duration::from_secs(20))
        };

        let (phases, verdicts) = collect(1);
        assert!(phases.iter().any(|phase| phase.contains("compiled in")), "{phases:?}");
        assert!(matches!(verdicts[0], Verdict::Success { .. }));

        let (phases, verdicts) = collect(2);
        assert!(phases.iter().any(|phase| phase.contains("compile skipped")), "{phases:?}");
        assert!(matches!(verdicts[0], Verdict::Success { .. }));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn streams_results_before_the_run_finishes() {
        let root = std::env::temp_dir().join(format!("zcph_runner_stream_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let fs_ops = FsOps::with_root(root.clone());
        assert!(fs_ops.save_config(&UserConfig { language: Language::Python, extra_flags: vec![], ..Default::default() }));

        let code_file = root.join("Slow.py");
        std::fs::write(
            &code_file,
            "import time\nx = int(input())\nif x == 2:\n    time.sleep(0.8)\nprint(x)\n",
        )
        .unwrap();

        let problem = ActiveProblem {
            name: "Slow".into(),
            url: String::new(),
            time_limit: 3000,
            test_cases: vec![
                ReceivingTestCase { input: "1".into(), expected_output: "1".into() },
                ReceivingTestCase { input: "2".into(), expected_output: "2".into() },
            ],
            code_file,
        };

        let (main_tx, main_rx) = channel();
        let runner_tx = spawn_runner(main_tx, fs_ops);
        runner_tx
            .send(RunnerCommand::RunCode {
                problem,
                run_id: 7,
                cancel: Arc::new(AtomicBool::new(false)),
            })
            .unwrap();

        let start = Instant::now();
        let mut first_case_at = None;
        let mut finished_at = None;
        while let Ok(message) = main_rx.recv_timeout(Duration::from_secs(5)) {
            if let PassingCommand::ToHelper(command) = message {
                match command {
                    HelperCommand::CaseResult { run_id: 7, index: 0, .. } => {
                        first_case_at = Some(start.elapsed());
                    }
                    HelperCommand::RunFinished { run_id: 7, .. } => {
                        finished_at = Some(start.elapsed());
                        break;
                    }
                    _ => {}
                }
            }
        }

        let first = first_case_at.expect("first case should stream");
        let finished = finished_at.expect("run should finish");
        assert!(first < Duration::from_millis(500), "first result took {first:?}");
        assert!(finished > Duration::from_millis(700), "run finished too early: {finished:?}");

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn java_recompiles_after_source_changes() {
        if std::process::Command::new("javac").arg("-version").output().is_err()
            || std::process::Command::new("java").arg("-version").output().is_err()
        {
            return;
        }

        let root = std::env::temp_dir().join(format!("zcph_runner_java_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let fs_ops = FsOps::with_root(root.clone());
        assert!(fs_ops.save_config(&UserConfig { language: Language::Java, extra_flags: vec![], ..Default::default() }));

        let code_file = root.join("JavaCache.java");
        let write = |body: &str| {
            std::fs::write(
                &code_file,
                format!(
                    "public class JavaCache {{ public static void main(String[] args) {{ {body} }} }}"
                ),
            )
            .unwrap();
        };
        write("System.out.println(1);");

        let problem = ActiveProblem {
            name: "JavaCache".into(),
            url: String::new(),
            time_limit: 3000,
            test_cases: vec![ReceivingTestCase { input: String::new(), expected_output: "1".into() }],
            code_file: code_file.clone(),
        };

        let (main_tx, main_rx) = channel();
        let runner_tx = spawn_runner(main_tx, fs_ops);
        let run = |run_id: u64| {
            runner_tx
                .send(RunnerCommand::RunCode {
                    problem: problem.clone(),
                    run_id,
                    cancel: Arc::new(AtomicBool::new(false)),
                })
                .unwrap();
            collect_run(&main_rx, run_id, Duration::from_secs(20))
        };

        let (phases, _) = run(1);
        assert!(phases.iter().any(|phase| phase.contains("compiled in")), "{phases:?}");

        let (phases, _) = run(2);
        assert!(phases.iter().any(|phase| phase.contains("compile skipped")), "{phases:?}");

        write("System.out.println(222222);");
        let (phases, _) = run(3);
        assert!(phases.iter().any(|phase| phase.contains("compiled in")), "{phases:?}");

        // Restoring the old source must not reuse the class from run 3.
        write("System.out.println(1);");
        let (phases, verdicts) = run(4);
        assert!(phases.iter().any(|phase| phase.contains("compiled in")), "{phases:?}");
        match verdicts.first() {
            Some(Verdict::Success { output, .. }) => {
                assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "1");
            }
            other => panic!("expected a successful run, got {other:?}"),
        }

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn cancellation_kills_a_hung_case() {
        if std::process::Command::new("python3").arg("--version").output().is_err() {
            return;
        }

        let root = std::env::temp_dir().join(format!("zcph_runner_cancel_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let fs_ops = FsOps::with_root(root.clone());
        assert!(fs_ops.save_config(&UserConfig { language: Language::Python, extra_flags: vec![], ..Default::default() }));

        let code_file = root.join("Hang.py");
        std::fs::write(&code_file, "while True:\n    pass\n").unwrap();
        let problem = ActiveProblem {
            name: "Hang".into(),
            url: String::new(),
            time_limit: 10_000,
            test_cases: vec![ReceivingTestCase { input: String::new(), expected_output: String::new() }],
            code_file,
        };

        let (main_tx, main_rx) = channel();
        let runner_tx = spawn_runner(main_tx, fs_ops);
        let cancel = Arc::new(AtomicBool::new(false));
        runner_tx
            .send(RunnerCommand::RunCode {
                problem,
                run_id: 9,
                cancel: cancel.clone(),
            })
            .unwrap();

        thread::sleep(Duration::from_millis(150));
        let stopped_at = Instant::now();
        cancel.store(true, Ordering::SeqCst);

        let mut finished = false;
        while let Ok(message) = main_rx.recv_timeout(Duration::from_secs(2)) {
            if let PassingCommand::ToHelper(HelperCommand::RunFinished { run_id: 9, .. }) = message {
                finished = true;
                break;
            }
        }
        assert!(finished, "the runner should acknowledge cancellation");
        assert!(
            stopped_at.elapsed() < Duration::from_secs(1),
            "cancellation took {:?}",
            stopped_at.elapsed()
        );

        let _ = std::fs::remove_dir_all(&root);
    }
}
