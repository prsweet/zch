use std::{
    collections::HashMap,
    fs,
    hash::{DefaultHasher, Hash, Hasher},
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, ExitStatus, Output, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicI32, Ordering},
        mpsc::{Receiver, RecvTimeoutError, Sender, channel},
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use portable_pty::{Child as PtyChild, CommandBuilder, MasterPty, PtySize, native_pty_system};

use crate::core::{
    ActiveProblem, Checker, HelperCommand, Language, PassingCommand, RunnerCommand, SessionInput,
    SessionStream, Toolchain, Verdict, checker, fs_ops::FsOps,
};

const POLL_INTERVAL: Duration = Duration::from_millis(1);
/// How often a running case checks for cancel / output limit / hard cap.
const WATCH_INTERVAL: Duration = Duration::from_millis(5);
const SESSION_POLL_INTERVAL: Duration = Duration::from_millis(15);
const SESSION_DRAIN_TIMEOUT: Duration = Duration::from_millis(750);
/// A case printing more than this is stopped with "output limit exceeded".
pub const OUTPUT_LIMIT_BYTES: usize = 64 * 1024 * 1024;
/// stderr is for debugging; anything past this is read and dropped.
const STDERR_LIMIT_BYTES: usize = 1024 * 1024;
/// A TLE case keeps running up to this multiple of the limit, so the report
/// can say how slow it really was instead of only "over the limit".
const HARD_LIMIT_FACTOR: u32 = 2;

/// The process group of the live interactive child, if any. This lets the UI
/// kill the child synchronously on quit or panic instead of hoping the runner
/// thread wakes up before the process exits.
static ACTIVE_SESSION_PGID: AtomicI32 = AtomicI32::new(0);

/// Kill the process group of the live interactive session, if one is running.
/// Safe to call from any thread and from the panic hook.
pub fn kill_live_session() {
    let pgid = ACTIVE_SESSION_PGID.swap(0, Ordering::SeqCst);
    if pgid > 0 {
        #[cfg(unix)]
        unsafe {
            libc::kill(-pgid, libc::SIGKILL);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ArtifactStamp {
    modified: SystemTime,
    len: u64,
}

impl ArtifactStamp {
    fn encode(&self) -> String {
        let nanos = self
            .modified
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        format!("{nanos} {}", self.len)
    }
}

/// State the runner keeps between runs.
#[derive(Default)]
struct RunnerState {
    /// `<compiler> --version` output, so a compiler upgrade invalidates the
    /// cached binaries. Asked once per compiler per session.
    compiler_ids: HashMap<String, String>,
}

impl RunnerState {
    fn compiler_id(&mut self, program: &str) -> &str {
        self.compiler_ids
            .entry(program.to_string())
            .or_insert_with(|| {
                Command::new(program)
                    .arg("--version")
                    .stdin(Stdio::null())
                    .output()
                    .map(|out| {
                        let mut id = String::from_utf8_lossy(&out.stdout).into_owned();
                        id.push_str(&String::from_utf8_lossy(&out.stderr));
                        id
                    })
                    .unwrap_or_default()
            })
    }
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
        let mut cache = RunnerState::default();
        while let Ok(command) = rx.recv() {
            match command {
                RunnerCommand::RunCode {
                    problem,
                    run_id,
                    cancel,
                } => run_streaming(&problem, run_id, &fs_ops, &main_tx, &mut cache, &cancel),
                RunnerCommand::Interact {
                    problem,
                    session_id,
                    cancel,
                    input,
                    pgid,
                } => run_interactive(
                    &problem,
                    session_id,
                    &fs_ops,
                    &main_tx,
                    &mut cache,
                    SessionChannels {
                        cancel,
                        input,
                        pgid,
                    },
                ),
            }
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
    cache: &mut RunnerState,
    cancel: &AtomicBool,
) {
    let (config, _) = fs_ops.load_config();
    let toolchain = config.toolchain_for(&problem.code_file);
    let checker = problem
        .checker
        .clone()
        .unwrap_or_else(|| config.checker.clone());
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
        let _ = tx.send(PassingCommand::ToHelper(HelperCommand::RunPhase {
            run_id,
            message,
        }));
    };

    let compiles = toolchain
        .language
        .compile_cmd(
            &problem.code_file,
            &fs_ops.binary_dir.join(&problem.name),
            &toolchain.flags,
        )
        .is_some();
    if compiles {
        phase("compiling…".to_string());
    }

    match compile(problem, &toolchain, fs_ops, cache, cancel) {
        Err(error) => {
            for index in 0..case_count {
                let sent = tx.send(PassingCommand::ToHelper(HelperCommand::CaseResult {
                    run_id,
                    index,
                    verdict: Verdict::CompilationError {
                        error: error.clone(),
                    },
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
    let (program, args) = toolchain.language.run_cmd(&problem.code_file, &binary_path);
    let time_limit = Duration::from_millis(config.time_limit_ms(problem));

    for (index, tc) in problem.test_cases.iter().enumerate() {
        if cancelled() {
            finish();
            return;
        }
        phase(format!("running case {}/{}…", index + 1, case_count));
        let Some(execution) = run_single(
            &program,
            &args,
            &tc.input,
            time_limit,
            OUTPUT_LIMIT_BYTES,
            cancel,
        ) else {
            finish();
            return;
        };
        let verdict = judge(
            execution,
            &tc.input,
            &tc.expected_output,
            &checker,
            time_limit,
            &fs_ops.binary_dir,
        );
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

/// Everything that decides what a compile produces: the source, language,
/// flags, compiler version, and where the output goes.
fn cache_key(
    problem: &ActiveProblem,
    toolchain: &Toolchain,
    compiler_id: &str,
    fs_ops: &FsOps,
) -> std::io::Result<u64> {
    let source = fs::read(&problem.code_file)?;
    let mut hasher = DefaultHasher::new();
    source.hash(&mut hasher);
    problem.code_file.hash(&mut hasher);
    toolchain.hash(&mut hasher);
    compiler_id.hash(&mut hasher);
    fs_ops.binary_dir.join(&problem.name).hash(&mut hasher);
    Ok(hasher.finish())
}

/// The cache record next to the binary: `<key> <artifact mtime> <len>`.
/// It lives on disk, so an unchanged file is not recompiled after a restart.
fn cache_record_path(problem: &ActiveProblem, fs_ops: &FsOps) -> PathBuf {
    fs_ops.binary_dir.join(format!("{}.zchkey", problem.name))
}

fn cache_record(key: u64, stamp: &ArtifactStamp) -> String {
    format!("{key:016x} {}", stamp.encode())
}

fn compile(
    problem: &ActiveProblem,
    toolchain: &Toolchain,
    fs_ops: &FsOps,
    cache: &mut RunnerState,
    cancel: &AtomicBool,
) -> Result<Compile, String> {
    if cancel.load(Ordering::SeqCst) {
        return Ok(Compile::Cancelled);
    }
    if !problem.code_file.exists() {
        return Err(format!(
            "source file not found: {}",
            problem.code_file.display()
        ));
    }

    let binary_path = fs_ops.binary_dir.join(&problem.name);
    let Some((program, args)) =
        toolchain
            .language
            .compile_cmd(&problem.code_file, &binary_path, &toolchain.flags)
    else {
        return Ok(Compile::NotNeeded);
    };
    let artifact_path = artifact_path(problem, toolchain.language, &binary_path);

    let compiler_id = cache.compiler_id(&program).to_string();
    let key = cache_key(problem, toolchain, &compiler_id, fs_ops)
        .map_err(|e| format!("cannot read {}: {e}", problem.code_file.display()))?;
    let record_path = cache_record_path(problem, fs_ops);
    if let Some(stamp) = artifact_path.as_deref().and_then(artifact_stamp)
        && fs::read_to_string(&record_path).ok().as_deref() == Some(&cache_record(key, &stamp))
    {
        return Ok(Compile::Skipped);
    }
    // The old record no longer describes the artifact, whatever happens next.
    let _ = fs::remove_file(&record_path);

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

    if let Some(stamp) = artifact_path.as_deref().and_then(artifact_stamp) {
        let _ = super::fs_ops::write_atomic(&record_path, cache_record(key, &stamp).as_bytes());
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
    if let Some(pid) = handle.pids().first() {
        kill_session_group(*pid as i32);
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

/// What happened to one execution, before it is judged.
#[derive(Debug)]
enum Execution {
    Exited {
        status: ExitStatus,
        stdout: Vec<u8>,
        stderr: Vec<u8>,
        /// CPU time capped by wall time: judges count CPU, but a JVM's GC
        /// and JIT threads can burn more CPU than wall time passes.
        time: Duration,
        memory_kb: Option<u64>,
    },
    /// Still running at the hard cap and killed; `time` is wall time.
    Killed {
        time: Duration,
        stdout: Vec<u8>,
    },
    OutputLimit {
        limit: usize,
    },
    FailedToStart(String),
}

/// Turn an execution into a verdict: limits first, then the checker.
fn judge(
    execution: Execution,
    input: &str,
    expected: &str,
    checker: &Checker,
    time_limit: Duration,
    scratch: &Path,
) -> Verdict {
    let lossy = |bytes: &[u8]| String::from_utf8_lossy(bytes).into_owned();
    match execution {
        Execution::FailedToStart(error) => Verdict::RuntimeError {
            error,
            stdout: String::new(),
        },
        Execution::OutputLimit { limit } => Verdict::OutputLimitExceeded { limit },
        Execution::Killed { time, stdout } => Verdict::TimeLimitExceeded {
            time: time.as_millis(),
            limit: time_limit.as_millis(),
            killed: true,
            stdout: lossy(&stdout),
        },
        Execution::Exited {
            status,
            stdout,
            stderr,
            time,
            memory_kb,
        } => {
            if time > time_limit {
                return Verdict::TimeLimitExceeded {
                    time: time.as_millis(),
                    limit: time_limit.as_millis(),
                    killed: false,
                    stdout: lossy(&stdout),
                };
            }
            if !status.success() {
                return Verdict::RuntimeError {
                    error: describe_failure(status, &lossy(&stderr)),
                    stdout: lossy(&stdout),
                };
            }
            let outcome = checker::check(checker, input, expected, &lossy(&stdout), scratch);
            Verdict::Success {
                output: Output {
                    status,
                    stdout,
                    stderr,
                },
                time: time.as_millis(),
                memory_kb,
                accepted: outcome.accepted,
                note: outcome.note,
            }
        }
    }
}

/// stderr plus a plain-language reading of the exit status.
fn describe_failure(status: ExitStatus, stderr: &str) -> String {
    let reason = match exit_signal(status) {
        Some(signal) => {
            let (name, hint) = signal_hint(signal);
            match hint {
                Some(hint) => format!("killed by {name} — {hint}"),
                None => format!("killed by {name}"),
            }
        }
        None => match status.code() {
            Some(code) => format!("exited with code {code}"),
            None => format!("exited with {status}"),
        },
    };
    let stderr = stderr.trim_end();
    if stderr.is_empty() {
        reason
    } else {
        format!("{stderr}\n{reason}")
    }
}

#[cfg(unix)]
fn exit_signal(status: ExitStatus) -> Option<i32> {
    use std::os::unix::process::ExitStatusExt;
    status.signal()
}

#[cfg(not(unix))]
fn exit_signal(_status: ExitStatus) -> Option<i32> {
    None
}

#[cfg(unix)]
fn signal_hint(signal: i32) -> (String, Option<&'static str>) {
    const STACK_HINT: &str = if cfg!(target_os = "macos") {
        "invalid memory access: an out-of-bounds index, a null/dangling pointer, or a stack \
         overflow from deep recursion (macOS gives the main thread only 8 MB of stack; add \
         -Wl,-stack_size,0x10000000 to your flags for 256 MB)"
    } else {
        "invalid memory access: an out-of-bounds index, a null/dangling pointer, or a stack \
         overflow from deep recursion (try `ulimit -s unlimited`)"
    };
    let (name, hint) = match signal {
        libc::SIGSEGV => ("SIGSEGV", Some(STACK_HINT)),
        libc::SIGBUS => ("SIGBUS", Some(STACK_HINT)),
        libc::SIGFPE => (
            "SIGFPE",
            Some("arithmetic error: integer division or modulo by zero (or INT_MIN / -1)"),
        ),
        libc::SIGABRT => (
            "SIGABRT",
            Some(
                "aborted: a failed assert, an uncaught exception (bad_alloc, out_of_range \
                 from .at(), …), or a sanitizer / _GLIBCXX_DEBUG check",
            ),
        ),
        libc::SIGILL => (
            "SIGILL",
            Some(
                "illegal instruction: usually undefined behaviour, e.g. a non-void function \
                 that falls off its end without returning",
            ),
        ),
        libc::SIGKILL => (
            "SIGKILL",
            Some("killed from outside, often for running out of memory"),
        ),
        libc::SIGPIPE => ("SIGPIPE", None),
        _ => return (format!("signal {signal}"), None),
    };
    (name.to_string(), hint)
}

#[cfg(not(unix))]
fn signal_hint(signal: i32) -> (String, Option<&'static str>) {
    (format!("signal {signal}"), None)
}

/// Read a pipe into memory up to `limit` bytes. With `overflow`, crossing
/// the limit raises the flag and stops reading (the case gets killed);
/// without it the rest is drained and dropped, so the child never blocks.
fn read_limited<R: Read>(mut reader: R, limit: usize, overflow: Option<&AtomicBool>) -> Vec<u8> {
    let mut out = Vec::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                let room = limit.saturating_sub(out.len());
                if n > room {
                    out.extend_from_slice(&buf[..room]);
                    if let Some(flag) = overflow {
                        flag.store(true, Ordering::SeqCst);
                        break;
                    }
                } else {
                    out.extend_from_slice(&buf[..n]);
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        }
    }
    out
}

/// Run one case. Returns `None` only when cancelled.
#[cfg(unix)]
fn run_single(
    program: &str,
    args: &[String],
    input: &str,
    time_limit: Duration,
    output_limit: usize,
    cancel: &AtomicBool,
) -> Option<Execution> {
    use std::os::unix::process::{CommandExt, ExitStatusExt};

    if cancel.load(Ordering::SeqCst) {
        return None;
    }

    let mut stdin_bytes = input.as_bytes().to_vec();
    if !stdin_bytes.is_empty() && !stdin_bytes.ends_with(b"\n") {
        stdin_bytes.push(b'\n');
    }

    let start = Instant::now();
    // Own process group, so a kill also takes down anything it spawned.
    let mut child = match Command::new(program)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .spawn()
    {
        Ok(child) => child,
        Err(e) => {
            return Some(Execution::FailedToStart(format!(
                "failed to start {program}: {e}"
            )));
        }
    };
    let pid = child.id() as i32;

    // A child that never reads stdin must not block us, so feed it from a
    // thread; the write fails with EPIPE once the child is gone.
    let stdin = child.stdin.take();
    let writer = thread::spawn(move || {
        if let Some(mut stdin) = stdin {
            let _ = stdin.write_all(&stdin_bytes);
        }
    });
    let overflow = Arc::new(AtomicBool::new(false));
    let stdout = child.stdout.take();
    let stdout_overflow = overflow.clone();
    let stdout_reader = thread::spawn(move || {
        stdout
            .map(|pipe| read_limited(pipe, output_limit, Some(&stdout_overflow)))
            .unwrap_or_default()
    });
    let stderr = child.stderr.take();
    let stderr_reader = thread::spawn(move || {
        stderr
            .map(|pipe| read_limited(pipe, STDERR_LIMIT_BYTES, None))
            .unwrap_or_default()
    });

    // wait4 blocks on its own thread and reports the exact exit moment plus
    // the child's resource usage; this thread only watches the limits.
    let (wait_tx, wait_rx) = channel();
    thread::spawn(move || {
        let result = wait_with_usage(pid);
        let _ = wait_tx.send((result, Instant::now()));
    });

    let hard_limit = time_limit * HARD_LIMIT_FACTOR;
    enum Stop {
        Exited(std::io::Result<(i32, libc::rusage)>, Instant),
        Cancelled,
        OutputLimit,
        HardLimit,
    }
    let stop = loop {
        match wait_rx.recv_timeout(WATCH_INTERVAL) {
            Ok((result, at)) => break Stop::Exited(result, at),
            Err(RecvTimeoutError::Timeout) => {
                if cancel.load(Ordering::SeqCst) {
                    break Stop::Cancelled;
                }
                if overflow.load(Ordering::SeqCst) {
                    break Stop::OutputLimit;
                }
                if start.elapsed() >= hard_limit {
                    break Stop::HardLimit;
                }
            }
            Err(RecvTimeoutError::Disconnected) => {
                break Stop::Exited(
                    Err(std::io::Error::other("lost track of the process")),
                    Instant::now(),
                );
            }
        }
    };

    // Whatever the outcome, nothing from this case may outlive it: leftover
    // group members would also keep the pipes (and the readers) open.
    kill_session_group(pid);
    if !matches!(stop, Stop::Exited(..)) {
        let _ = wait_rx.recv();
    }
    let killed_at = start.elapsed();
    let stdout = stdout_reader.join().unwrap_or_default();
    let stderr = stderr_reader.join().unwrap_or_default();
    let _ = writer.join();
    drop(child);

    Some(match stop {
        Stop::Cancelled => return None,
        Stop::OutputLimit => Execution::OutputLimit {
            limit: output_limit,
        },
        Stop::HardLimit => Execution::Killed {
            time: killed_at,
            stdout,
        },
        Stop::Exited(Err(error), _) => {
            Execution::FailedToStart(format!("failed to read process status: {error}"))
        }
        Stop::Exited(Ok((raw_status, usage)), exited_at) => {
            // A flood that ended before the watcher saw the flag is still OLE.
            if overflow.load(Ordering::SeqCst) {
                Execution::OutputLimit {
                    limit: output_limit,
                }
            } else {
                let wall = exited_at.duration_since(start);
                let cpu = timeval(usage.ru_utime) + timeval(usage.ru_stime);
                Execution::Exited {
                    status: ExitStatus::from_raw(raw_status),
                    stdout,
                    stderr,
                    time: cpu.min(wall),
                    memory_kb: max_rss_kb(&usage),
                }
            }
        }
    })
}

/// Blocking `wait4`: the exit status and the child's resource usage.
#[cfg(unix)]
fn wait_with_usage(pid: i32) -> std::io::Result<(i32, libc::rusage)> {
    loop {
        let mut status = 0;
        // SAFETY: both out-pointers are valid for the duration of the call.
        let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
        let result = unsafe { libc::wait4(pid, &mut status, 0, &mut usage) };
        if result == pid {
            return Ok((status, usage));
        }
        let error = std::io::Error::last_os_error();
        if error.kind() != std::io::ErrorKind::Interrupted {
            return Err(error);
        }
    }
}

#[cfg(unix)]
fn timeval(value: libc::timeval) -> Duration {
    Duration::from_secs(value.tv_sec.max(0) as u64)
        + Duration::from_micros(value.tv_usec.max(0) as u64)
}

/// Peak resident memory. macOS reports bytes, Linux kilobytes.
#[cfg(unix)]
fn max_rss_kb(usage: &libc::rusage) -> Option<u64> {
    let raw = u64::try_from(usage.ru_maxrss)
        .ok()
        .filter(|&value| value > 0)?;
    Some(if cfg!(target_os = "macos") {
        raw / 1024
    } else {
        raw
    })
}

/// Portable fallback: wall time only, no memory reading.
#[cfg(not(unix))]
fn run_single(
    program: &str,
    args: &[String],
    input: &str,
    time_limit: Duration,
    output_limit: usize,
    cancel: &AtomicBool,
) -> Option<Execution> {
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
            return Some(Execution::FailedToStart(format!(
                "failed to start {program}: {e}"
            )));
        }
    };
    loop {
        match handle.try_wait() {
            Ok(Some(output)) => {
                if output.stdout.len() > output_limit {
                    return Some(Execution::OutputLimit {
                        limit: output_limit,
                    });
                }
                return Some(Execution::Exited {
                    status: output.status,
                    stdout: output.stdout.clone(),
                    stderr: output.stderr.clone(),
                    time: start.elapsed(),
                    memory_kb: None,
                });
            }
            Ok(None) => {
                if cancel.load(Ordering::SeqCst) {
                    stop(&handle);
                    return None;
                }
                if start.elapsed() >= time_limit * HARD_LIMIT_FACTOR {
                    stop(&handle);
                    return Some(Execution::Killed {
                        time: start.elapsed(),
                        stdout: Vec::new(),
                    });
                }
                thread::sleep(POLL_INTERVAL);
            }
            Err(e) => {
                return Some(Execution::FailedToStart(format!(
                    "failed to read process status: {e}"
                )));
            }
        }
    }
}

/// Everything the UI and the runner share for one interactive session.
struct SessionChannels {
    cancel: Arc<AtomicBool>,
    input: Receiver<SessionInput>,
    pgid: Arc<AtomicI32>,
}

/// Compile with the shared cache, then leave the program running on a
/// pseudo-terminal. A PTY is what makes the feature actually interactive:
/// the child sees a terminal, so libc/Python do not silently block-buffer
/// prompts, stdout and stderr share one ordered stream, and the line
/// discipline handles Ctrl-C/Ctrl-D for us.
fn run_interactive(
    problem: &ActiveProblem,
    session_id: u64,
    fs_ops: &FsOps,
    tx: &Sender<PassingCommand>,
    cache: &mut RunnerState,
    channels: SessionChannels,
) {
    let SessionChannels {
        cancel,
        input,
        pgid,
    } = channels;
    let cancel = &cancel;
    let (config, _) = fs_ops.load_config();
    let send_end = |message: String| {
        let _ = tx.send(PassingCommand::ToHelper(HelperCommand::SessionEnded {
            session_id,
            message,
        }));
    };
    let send_chunk = |stream: SessionStream, text: String| {
        let _ = tx.send(PassingCommand::ToHelper(HelperCommand::SessionChunk {
            session_id,
            stream,
            text,
        }));
    };

    if cancel.load(Ordering::SeqCst) {
        send_end("stopped".to_string());
        return;
    }

    let toolchain = config.toolchain_for(&problem.code_file);
    let binary_path = fs_ops.binary_dir.join(&problem.name);
    let compiles = toolchain
        .language
        .compile_cmd(&problem.code_file, &binary_path, &toolchain.flags)
        .is_some();
    if compiles {
        send_chunk(SessionStream::Status, "compiling…\n".to_string());
    }
    match compile(problem, &toolchain, fs_ops, cache, cancel) {
        Err(error) => {
            if !error.is_empty() {
                send_chunk(SessionStream::Err, format!("{error}\n"));
            }
            send_end("compile failed".to_string());
            return;
        }
        Ok(Compile::Cancelled) => {
            send_end("stopped".to_string());
            return;
        }
        Ok(Compile::Compiled(took)) => {
            send_chunk(
                SessionStream::Status,
                format!("compiled in {}ms\n", took.as_millis()),
            );
        }
        Ok(Compile::Skipped) => {
            send_chunk(
                SessionStream::Status,
                "sources unchanged — compile skipped\n".to_string(),
            );
        }
        Ok(Compile::NotNeeded) => {}
    }

    if cancel.load(Ordering::SeqCst) {
        send_end("stopped".to_string());
        return;
    }

    let (program, args) = toolchain.language.run_cmd(&problem.code_file, &binary_path);
    let PtySession {
        mut child,
        reader,
        writer,
        pid,
    } = match spawn_session(&program, &args) {
        Ok(session) => session,
        Err(error) => {
            send_chunk(SessionStream::Err, format!("{error}\n"));
            send_end("failed to start".to_string());
            return;
        }
    };
    if let Some(pid) = pid {
        pgid.store(pid as i32, Ordering::SeqCst);
        ACTIVE_SESSION_PGID.store(pid as i32, Ordering::SeqCst);
    }

    let reader_tx = tx.clone();
    let reader_thread = thread::spawn(move || pump_session(reader, session_id, reader_tx));
    // Input gets its own thread so a child that stops reading can never wedge
    // the runner: cancelling kills the group, which unblocks the write.
    thread::spawn(move || pump_input(writer, input));
    send_chunk(SessionStream::Status, "running\n".to_string());

    let status = loop {
        if cancel.load(Ordering::SeqCst) {
            stop_session_child(&mut child);
            break None;
        }
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) => thread::sleep(SESSION_POLL_INTERVAL),
            Err(error) => {
                send_chunk(
                    SessionStream::Err,
                    format!("failed to read process status: {error}\n"),
                );
                stop_session_child(&mut child);
                break None;
            }
        }
    };

    if let Some(pid) = pid {
        let _ = pgid.compare_exchange(pid as i32, 0, Ordering::SeqCst, Ordering::SeqCst);
        let _ =
            ACTIVE_SESSION_PGID.compare_exchange(pid as i32, 0, Ordering::SeqCst, Ordering::SeqCst);
    }

    // Give the reader a moment to deliver the last bytes. A grandchild that
    // inherited the PTY must not be allowed to hang the runner forever.
    drain_reader(reader_thread, SESSION_DRAIN_TIMEOUT);

    let message = match status {
        None => "stopped".to_string(),
        Some(status) if status.success() => "exited".to_string(),
        Some(status) => match status.signal() {
            Some(signal) => format!("terminated by {signal}"),
            None => format!("exited with code {}", status.exit_code()),
        },
    };
    send_end(message);
}

struct PtySession {
    child: Box<dyn PtyChild + Send + Sync>,
    reader: Box<dyn Read + Send>,
    writer: Box<dyn Write + Send>,
    pid: Option<u32>,
}

fn spawn_session(program: &str, args: &[String]) -> Result<PtySession, String> {
    let pair = native_pty_system()
        .openpty(PtySize {
            rows: 40,
            cols: 120,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|error| format!("failed to allocate a pseudo-terminal: {error}"))?;
    configure_pty(pair.master.as_ref())?;

    let mut builder = CommandBuilder::new(program);
    builder.args(args);
    builder.env("TERM", "dumb");
    builder.env("NO_COLOR", "1");
    // Harmless for other languages, and belt-and-braces for Python even on
    // platforms where the PTY does not force line buffering.
    builder.env("PYTHONUNBUFFERED", "1");

    let child = pair
        .slave
        .spawn_command(builder)
        .map_err(|error| format!("failed to start {program}: {error}"))?;
    let pid = child.process_id();
    let reader = pair
        .master
        .try_clone_reader()
        .map_err(|error| format!("failed to read from {program}: {error}"))?;
    let writer = pair
        .master
        .take_writer()
        .map_err(|error| format!("failed to write to {program}: {error}"))?;
    drop(pair.slave);
    Ok(PtySession {
        child,
        reader,
        writer,
        pid,
    })
}

#[cfg(unix)]
fn configure_pty(master: &dyn MasterPty) -> Result<(), String> {
    let Some(fd) = master.as_raw_fd() else {
        return Ok(());
    };
    unsafe {
        let mut term: libc::termios = std::mem::zeroed();
        if libc::tcgetattr(fd, &mut term) != 0 {
            return Err(format!(
                "failed to inspect the pseudo-terminal: {}",
                std::io::Error::last_os_error()
            ));
        }
        // We echo the user's line ourselves, in the transcript, so the line
        // discipline must not echo it a second time (and must not echo ^C).
        term.c_lflag &= !(libc::ECHO | libc::ECHOE | libc::ECHOK | libc::ECHONL);
        #[cfg(any(target_os = "macos", target_os = "linux", target_os = "freebsd"))]
        {
            term.c_lflag &= !libc::ECHOCTL;
        }
        if libc::tcsetattr(fd, libc::TCSANOW, &term) != 0 {
            return Err(format!(
                "failed to configure the pseudo-terminal: {}",
                std::io::Error::last_os_error()
            ));
        }
    }
    Ok(())
}

#[cfg(not(unix))]
fn configure_pty(_master: &dyn MasterPty) -> Result<(), String> {
    Ok(())
}

fn pump_input(mut writer: Box<dyn Write + Send>, input: Receiver<SessionInput>) {
    for item in input {
        let result = match item {
            SessionInput::Line(line) => writer
                .write_all(line.as_bytes())
                .and_then(|_| writer.write_all(b"\n")),
            SessionInput::Eof => writer.write_all(&[0x04]),
            SessionInput::Interrupt => writer.write_all(&[0x03]),
        };
        if result.and_then(|_| writer.flush()).is_err() {
            break;
        }
    }
}

fn pump_session(mut reader: Box<dyn Read + Send>, session_id: u64, tx: Sender<PassingCommand>) {
    let mut filter = ControlFilter::default();
    let mut decoder = Utf8Decoder::default();
    let mut buf = [0u8; 4096];
    loop {
        match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                let filtered = filter.push(&buf[..n]);
                let text = decoder.push(&filtered);
                if !text.is_empty()
                    && tx
                        .send(PassingCommand::ToHelper(HelperCommand::SessionChunk {
                            session_id,
                            stream: SessionStream::Out,
                            text,
                        }))
                        .is_err()
                {
                    break;
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        }
    }
    let tail = decoder.finish();
    if !tail.is_empty() {
        let _ = tx.send(PassingCommand::ToHelper(HelperCommand::SessionChunk {
            session_id,
            stream: SessionStream::Out,
            text: tail,
        }));
    }
}

fn stop_session_child(child: &mut Box<dyn PtyChild + Send + Sync>) {
    if let Some(pid) = child.process_id() {
        kill_session_group(pid as i32);
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// Kill a whole process group. Public so the UI can tear a session down
/// synchronously instead of waiting for the runner's next poll.
pub fn kill_session_group(pid: i32) {
    if pid <= 0 {
        return;
    }
    #[cfg(unix)]
    unsafe {
        libc::kill(-pid, libc::SIGKILL);
    }
    #[cfg(not(unix))]
    let _ = pid;
}

fn drain_reader(handle: thread::JoinHandle<()>, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    while !handle.is_finished() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(5));
    }
    if handle.is_finished() {
        let _ = handle.join();
    }
}

/// Removes terminal control sequences and normalises CR/CRLF to LF. The UI
/// renders text, not a terminal, so escape sequences would otherwise show up
/// as garbage. State is kept between reads, so a sequence split across a
/// 4 KiB read boundary is still removed.
#[derive(Default)]
struct ControlFilter {
    state: FilterState,
    skip_lf: bool,
}

#[derive(Default, Clone, Copy, PartialEq, Eq)]
enum FilterState {
    #[default]
    Normal,
    Escape,
    Csi,
    Osc,
    OscEscape,
}

impl ControlFilter {
    fn push(&mut self, bytes: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(bytes.len());
        for &byte in bytes {
            if self.skip_lf {
                self.skip_lf = false;
                if byte == b'\n' {
                    continue;
                }
            }
            match self.state {
                FilterState::Normal => match byte {
                    0x1b => self.state = FilterState::Escape,
                    b'\r' => {
                        out.push(b'\n');
                        self.skip_lf = true;
                    }
                    b'\n' | b'\t' => out.push(byte),
                    0x00..=0x08 | 0x0b | 0x0c | 0x0e..=0x1f | 0x7f => {}
                    _ => out.push(byte),
                },
                FilterState::Escape => match byte {
                    b'[' => self.state = FilterState::Csi,
                    b']' | b'P' | b'X' | b'^' | b'_' => self.state = FilterState::Osc,
                    _ => self.state = FilterState::Normal,
                },
                FilterState::Csi => {
                    if (0x40..=0x7e).contains(&byte) {
                        self.state = FilterState::Normal;
                    }
                }
                FilterState::Osc => match byte {
                    0x07 => self.state = FilterState::Normal,
                    0x1b => self.state = FilterState::OscEscape,
                    _ => {}
                },
                FilterState::OscEscape => {
                    self.state = if byte == b'\\' {
                        FilterState::Normal
                    } else {
                        FilterState::Osc
                    };
                }
            }
        }
        out
    }
}

/// Incremental UTF-8 decoder: a multi-byte character can be split across two
/// PTY reads, and `from_utf8_lossy` per chunk would replace it with U+FFFD.
#[derive(Default)]
struct Utf8Decoder {
    pending: Vec<u8>,
}

impl Utf8Decoder {
    fn push(&mut self, bytes: &[u8]) -> String {
        self.pending.extend_from_slice(bytes);
        let mut out = String::new();
        loop {
            match std::str::from_utf8(&self.pending) {
                Ok(text) => {
                    out.push_str(text);
                    self.pending.clear();
                    break;
                }
                Err(error) => {
                    let valid = error.valid_up_to();
                    if valid > 0 {
                        out.push_str(std::str::from_utf8(&self.pending[..valid]).unwrap());
                        self.pending.drain(..valid);
                    }
                    match error.error_len() {
                        Some(len) => {
                            out.push('\u{fffd}');
                            self.pending.drain(..len);
                        }
                        None => break,
                    }
                }
            }
        }
        out
    }

    fn finish(&mut self) -> String {
        let text = String::from_utf8_lossy(&self.pending).into_owned();
        self.pending.clear();
        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{Language, PassingCommand, ReceivingTestCase, UserConfig};
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{Receiver, channel},
    };
    use std::time::Instant;

    /// The live-session process group is process-global, so interactive tests
    /// must not run at the same time as each other.
    static SESSION_TEST_LOCK: Mutex<()> = Mutex::new(());

    fn session_test_lock() -> std::sync::MutexGuard<'static, ()> {
        SESSION_TEST_LOCK
            .lock()
            .unwrap_or_else(|error| error.into_inner())
    }

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
                    HelperCommand::RunPhase {
                        run_id: id,
                        message,
                    } if id == run_id => {
                        phases.push(message);
                    }
                    HelperCommand::CaseResult {
                        run_id: id,
                        verdict,
                        ..
                    } if id == run_id => {
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
            checker: None,
        };
        let cpp = Toolchain {
            language: Language::Cpp,
            flags: vec!["-O2".into()],
        };
        let first = cache_key(&problem, &cpp, "g++ 1", &fs_ops).unwrap();
        assert_eq!(first, cache_key(&problem, &cpp, "g++ 1", &fs_ops).unwrap());

        let mut other_flags = cpp.clone();
        other_flags.flags.push("-std=c++20".into());
        assert_ne!(
            first,
            cache_key(&problem, &other_flags, "g++ 1", &fs_ops).unwrap()
        );

        // A compiler upgrade invalidates the binary too.
        assert_ne!(first, cache_key(&problem, &cpp, "g++ 2", &fs_ops).unwrap());

        std::fs::write(&file, "int main() { return 0; }").unwrap();
        assert_ne!(first, cache_key(&problem, &cpp, "g++ 1", &fs_ops).unwrap());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn second_run_reuses_the_compiled_binary() {
        if std::process::Command::new("g++")
            .arg("--version")
            .output()
            .is_err()
        {
            return;
        }

        let root =
            std::env::temp_dir().join(format!("zcph_runner_compile_cache_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let fs_ops = FsOps::with_root(root.clone());
        assert!(fs_ops.save_config(&UserConfig {
            language: Language::Cpp,
            extra_flags: vec![],
            ..Default::default()
        }));

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
            test_cases: vec![ReceivingTestCase {
                input: "1 2".into(),
                expected_output: "3".into(),
            }],
            code_file,
            checker: None,
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
        assert!(
            phases.iter().any(|phase| phase.contains("compiled in")),
            "{phases:?}"
        );
        assert!(matches!(verdicts[0], Verdict::Success { .. }));

        let (phases, verdicts) = collect(2);
        assert!(
            phases.iter().any(|phase| phase.contains("compile skipped")),
            "{phases:?}"
        );
        assert!(matches!(verdicts[0], Verdict::Success { .. }));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn streams_results_before_the_run_finishes() {
        let root = std::env::temp_dir().join(format!("zcph_runner_stream_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let fs_ops = FsOps::with_root(root.clone());
        assert!(fs_ops.save_config(&UserConfig {
            language: Language::Python,
            extra_flags: vec![],
            ..Default::default()
        }));

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
                ReceivingTestCase {
                    input: "1".into(),
                    expected_output: "1".into(),
                },
                ReceivingTestCase {
                    input: "2".into(),
                    expected_output: "2".into(),
                },
            ],
            code_file,
            checker: None,
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
                    HelperCommand::CaseResult {
                        run_id: 7,
                        index: 0,
                        ..
                    } => {
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
        assert!(
            first < Duration::from_millis(500),
            "first result took {first:?}"
        );
        assert!(
            finished > Duration::from_millis(700),
            "run finished too early: {finished:?}"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn java_recompiles_after_source_changes() {
        if std::process::Command::new("javac")
            .arg("-version")
            .output()
            .is_err()
            || std::process::Command::new("java")
                .arg("-version")
                .output()
                .is_err()
        {
            return;
        }

        let root = std::env::temp_dir().join(format!("zcph_runner_java_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let fs_ops = FsOps::with_root(root.clone());
        assert!(fs_ops.save_config(&UserConfig {
            language: Language::Java,
            extra_flags: vec![],
            ..Default::default()
        }));

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
            test_cases: vec![ReceivingTestCase {
                input: String::new(),
                expected_output: "1".into(),
            }],
            code_file: code_file.clone(),
            checker: None,
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
        assert!(
            phases.iter().any(|phase| phase.contains("compiled in")),
            "{phases:?}"
        );

        let (phases, _) = run(2);
        assert!(
            phases.iter().any(|phase| phase.contains("compile skipped")),
            "{phases:?}"
        );

        write("System.out.println(222222);");
        let (phases, _) = run(3);
        assert!(
            phases.iter().any(|phase| phase.contains("compiled in")),
            "{phases:?}"
        );

        // Restoring the old source must not reuse the class from run 3.
        write("System.out.println(1);");
        let (phases, verdicts) = run(4);
        assert!(
            phases.iter().any(|phase| phase.contains("compiled in")),
            "{phases:?}"
        );
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
        if std::process::Command::new("python3")
            .arg("--version")
            .output()
            .is_err()
        {
            return;
        }

        let root = std::env::temp_dir().join(format!("zcph_runner_cancel_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let fs_ops = FsOps::with_root(root.clone());
        assert!(fs_ops.save_config(&UserConfig {
            language: Language::Python,
            extra_flags: vec![],
            ..Default::default()
        }));

        let code_file = root.join("Hang.py");
        std::fs::write(&code_file, "while True:\n    pass\n").unwrap();
        let problem = ActiveProblem {
            name: "Hang".into(),
            url: String::new(),
            time_limit: 10_000,
            test_cases: vec![ReceivingTestCase {
                input: String::new(),
                expected_output: String::new(),
            }],
            code_file,
            checker: None,
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
            if let PassingCommand::ToHelper(HelperCommand::RunFinished { run_id: 9, .. }) = message
            {
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
    fn collect_session(
        main_rx: &Receiver<PassingCommand>,
        session_id: u64,
        timeout: Duration,
    ) -> (String, String, Option<String>) {
        let mut out = String::new();
        let mut status = String::new();
        let mut ended = None;
        while let Ok(message) = main_rx.recv_timeout(timeout) {
            if let PassingCommand::ToHelper(command) = message {
                match command {
                    HelperCommand::SessionChunk {
                        session_id: id,
                        stream,
                        text,
                    } if id == session_id => match stream {
                        SessionStream::Status => status.push_str(&text),
                        SessionStream::Out | SessionStream::Err => out.push_str(&text),
                    },
                    HelperCommand::SessionEnded {
                        session_id: id,
                        message,
                    } if id == session_id => {
                        ended = Some(message);
                        break;
                    }
                    _ => {}
                }
            }
        }
        (out, status, ended)
    }

    #[test]
    fn control_filter_strips_ansi_and_normalises_newlines() {
        let mut filter = ControlFilter::default();
        assert_eq!(
            filter.push(b"a\r\nb\x1b[31mc\x1b[0m\r\nd"),
            b"a\nbc\nd".to_vec()
        );
        // A sequence split across reads is still removed.
        assert_eq!(filter.push(b"x\x1b[3"), b"x".to_vec());
        assert_eq!(filter.push(b"1my"), b"y".to_vec());
        // OSC titles are removed too.
        assert_eq!(filter.push(b"\x1b]0;title\x07ok"), b"ok".to_vec());
    }

    #[test]
    fn utf8_decoder_handles_split_characters() {
        let mut decoder = Utf8Decoder::default();
        let bytes = "héllo".as_bytes();
        assert_eq!(decoder.push(&bytes[..2]), "h");
        assert_eq!(decoder.push(&bytes[2..]), "éllo");
        assert_eq!(decoder.finish(), "");
    }

    #[test]
    fn interactive_session_streams_input_and_eof() {
        let _guard = session_test_lock();
        if std::process::Command::new("python3")
            .arg("--version")
            .output()
            .is_err()
        {
            return;
        }
        let root = std::env::temp_dir().join(format!("zcph_runner_session_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let fs_ops = FsOps::with_root(root.clone());
        assert!(fs_ops.save_config(&UserConfig {
            language: Language::Python,
            extra_flags: vec![],
            ..Default::default()
        }));
        let code_file = root.join("Live.py");
        std::fs::write(
            &code_file,
            "import sys\nline = sys.stdin.readline()\nprint('got:' + line.strip(), flush=True)\nrest = sys.stdin.read()\nprint('eof:' + rest.strip(), flush=True)\n",
        )
        .unwrap();
        let problem = ActiveProblem {
            name: "Live".into(),
            url: String::new(),
            time_limit: 3000,
            test_cases: Vec::new(),
            code_file,
            checker: None,
        };

        let (main_tx, main_rx) = channel();
        let runner_tx = spawn_runner(main_tx, fs_ops);
        let (input_tx, input_rx) = channel();
        let session_id = 41;
        runner_tx
            .send(RunnerCommand::Interact {
                problem,
                session_id,
                cancel: Arc::new(AtomicBool::new(false)),
                input: input_rx,
                pgid: Arc::new(AtomicI32::new(0)),
            })
            .unwrap();
        input_tx.send(SessionInput::Line("42".into())).unwrap();
        input_tx.send(SessionInput::Eof).unwrap();

        let (out, status, ended) = collect_session(&main_rx, session_id, Duration::from_secs(20));
        assert!(status.contains("running"), "{status:?}");
        // The pty must not echo the line: the UI prints it, exactly once, so
        // the transcript stays ordered and styled.
        assert_eq!(out.trim(), "got:42\neof:", "unexpected transcript: {out:?}");
        assert_eq!(ended.as_deref(), Some("exited"));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn interactive_cancel_kills_the_child() {
        let _guard = session_test_lock();
        if std::process::Command::new("python3")
            .arg("--version")
            .output()
            .is_err()
        {
            return;
        }
        let root =
            std::env::temp_dir().join(format!("zcph_runner_session_cancel_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let fs_ops = FsOps::with_root(root.clone());
        assert!(fs_ops.save_config(&UserConfig {
            language: Language::Python,
            extra_flags: vec![],
            ..Default::default()
        }));
        let code_file = root.join("HangLive.py");
        std::fs::write(&code_file, "while True:\n    pass\n").unwrap();
        let problem = ActiveProblem {
            name: "HangLive".into(),
            url: String::new(),
            time_limit: 3000,
            test_cases: Vec::new(),
            code_file,
            checker: None,
        };

        let (main_tx, main_rx) = channel();
        let runner_tx = spawn_runner(main_tx, fs_ops);
        let (_input_tx, input_rx) = channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let session_id = 42;
        runner_tx
            .send(RunnerCommand::Interact {
                problem,
                session_id,
                cancel: cancel.clone(),
                input: input_rx,
                pgid: Arc::new(AtomicI32::new(0)),
            })
            .unwrap();
        thread::sleep(Duration::from_millis(200));
        let stopped_at = Instant::now();
        cancel.store(true, Ordering::SeqCst);
        let (_, _, ended) = collect_session(&main_rx, session_id, Duration::from_secs(2));
        assert_eq!(ended.as_deref(), Some("stopped"));
        assert!(
            stopped_at.elapsed() < Duration::from_secs(1),
            "cancellation took {:?}",
            stopped_at.elapsed()
        );
        kill_live_session();

        let _ = std::fs::remove_dir_all(&root);
    }
    #[test]
    fn interactive_interrupt_reaches_the_child() {
        let _guard = session_test_lock();
        if std::process::Command::new("python3")
            .arg("--version")
            .output()
            .is_err()
        {
            return;
        }
        let root =
            std::env::temp_dir().join(format!("zcph_runner_session_int_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let fs_ops = FsOps::with_root(root.clone());
        assert!(fs_ops.save_config(&UserConfig {
            language: Language::Python,
            extra_flags: vec![],
            ..Default::default()
        }));
        let code_file = root.join("InterruptLive.py");
        std::fs::write(
            &code_file,
            "import time\nprint('ready', flush=True)\nwhile True:\n    time.sleep(0.05)\n",
        )
        .unwrap();
        let problem = ActiveProblem {
            name: "InterruptLive".into(),
            url: String::new(),
            time_limit: 3000,
            test_cases: Vec::new(),
            code_file,
            checker: None,
        };

        let (main_tx, main_rx) = channel();
        let runner_tx = spawn_runner(main_tx, fs_ops);
        let (input_tx, input_rx) = channel();
        let session_id = 43;
        runner_tx
            .send(RunnerCommand::Interact {
                problem,
                session_id,
                cancel: Arc::new(AtomicBool::new(false)),
                input: input_rx,
                pgid: Arc::new(AtomicI32::new(0)),
            })
            .unwrap();

        let mut out = String::new();
        let mut sent = false;
        let mut ended = None;
        while let Ok(message) = main_rx.recv_timeout(Duration::from_secs(5)) {
            if let PassingCommand::ToHelper(command) = message {
                match command {
                    HelperCommand::SessionChunk {
                        session_id: id,
                        stream,
                        text,
                    } if id == session_id => {
                        if stream != SessionStream::Status {
                            out.push_str(&text);
                        }
                        if !sent && out.contains("ready") {
                            input_tx.send(SessionInput::Interrupt).unwrap();
                            sent = true;
                        }
                    }
                    HelperCommand::SessionEnded {
                        session_id: id,
                        message,
                    } if id == session_id => {
                        ended = Some(message);
                        break;
                    }
                    _ => {}
                }
            }
        }
        assert!(sent, "the session never became ready: {out:?}");
        assert!(
            out.contains("KeyboardInterrupt"),
            "Ctrl-C did not reach the child: {out:?}"
        );
        assert!(ended.is_some(), "the session should report an end");

        let _ = std::fs::remove_dir_all(&root);
    }
    #[test]
    fn kill_live_session_terminates_the_current_child() {
        let _guard = session_test_lock();
        if std::process::Command::new("python3")
            .arg("--version")
            .output()
            .is_err()
        {
            return;
        }
        let root =
            std::env::temp_dir().join(format!("zcph_runner_session_kill_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let fs_ops = FsOps::with_root(root.clone());
        assert!(fs_ops.save_config(&UserConfig {
            language: Language::Python,
            extra_flags: vec![],
            ..Default::default()
        }));
        let code_file = root.join("KillLive.py");
        std::fs::write(&code_file, "while True:\n    pass\n").unwrap();
        let problem = ActiveProblem {
            name: "KillLive".into(),
            url: String::new(),
            time_limit: 3000,
            test_cases: Vec::new(),
            code_file,
            checker: None,
        };

        let (main_tx, main_rx) = channel();
        let runner_tx = spawn_runner(main_tx, fs_ops);
        let (_input_tx, input_rx) = channel();
        let pgid = Arc::new(AtomicI32::new(0));
        let session_id = 44;
        runner_tx
            .send(RunnerCommand::Interact {
                problem,
                session_id,
                cancel: Arc::new(AtomicBool::new(false)),
                input: input_rx,
                pgid: pgid.clone(),
            })
            .unwrap();

        let deadline = Instant::now() + Duration::from_secs(5);
        while pgid.load(Ordering::SeqCst) == 0 && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        assert!(
            pgid.load(Ordering::SeqCst) > 0,
            "the session never became live"
        );

        kill_live_session();
        assert_eq!(ACTIVE_SESSION_PGID.load(Ordering::SeqCst), 0);
        let (_, _, ended) = collect_session(&main_rx, session_id, Duration::from_secs(2));
        assert!(ended.is_some(), "the runner should notice the killed child");

        let _ = std::fs::remove_dir_all(&root);
    }

    fn sh(script: &str) -> (String, Vec<String>) {
        ("sh".to_string(), vec!["-c".to_string(), script.to_string()])
    }

    fn exec(script: &str, time_limit_ms: u64, output_limit: usize) -> Execution {
        let (program, args) = sh(script);
        run_single(
            &program,
            &args,
            "",
            Duration::from_millis(time_limit_ms),
            output_limit,
            &AtomicBool::new(false),
        )
        .expect("not cancelled")
    }

    fn verdict_of(execution: Execution, expected: &str, time_limit_ms: u64) -> Verdict {
        judge(
            execution,
            "",
            expected,
            &Checker::Tokens,
            Duration::from_millis(time_limit_ms),
            &std::env::temp_dir(),
        )
    }

    #[cfg(unix)]
    #[test]
    fn endless_output_is_stopped_at_the_limit() {
        let started = Instant::now();
        let execution = exec("yes", 10_000, 1024 * 1024);
        assert!(
            matches!(execution, Execution::OutputLimit { limit } if limit == 1024 * 1024),
            "{execution:?}"
        );
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "must not wait for the time limit"
        );
    }

    #[cfg(unix)]
    #[test]
    fn slow_cases_report_how_slow_they_were() {
        // Finished, but over the limit: the real time is reported, and even a
        // correct answer is TLE.
        let finished = Execution::Exited {
            status: Command::new("true").status().unwrap(),
            stdout: b"42\n".to_vec(),
            stderr: Vec::new(),
            time: Duration::from_millis(1340),
            memory_kb: None,
        };
        match verdict_of(finished, "42", 1000) {
            Verdict::TimeLimitExceeded {
                time,
                limit,
                killed,
                ..
            } => {
                assert_eq!((time, limit, killed), (1340, 1000, false));
            }
            other => panic!("expected TLE, got {other:?}"),
        }

        // A hang is stopped at twice the limit.
        let started = Instant::now();
        match verdict_of(exec("sleep 30", 200, OUTPUT_LIMIT_BYTES), "", 200) {
            Verdict::TimeLimitExceeded { killed, time, .. } => {
                assert!(killed);
                assert!(time >= 400, "{time}");
            }
            other => panic!("expected TLE, got {other:?}"),
        }
        assert!(started.elapsed() < Duration::from_secs(3));
    }

    #[cfg(unix)]
    #[test]
    fn crashes_name_the_signal_and_keep_partial_output() {
        match verdict_of(
            exec("echo partial; kill -SEGV $$", 2000, OUTPUT_LIMIT_BYTES),
            "",
            2000,
        ) {
            Verdict::RuntimeError { error, stdout } => {
                assert!(error.contains("SIGSEGV"), "{error}");
                assert!(error.contains("stack"), "{error}");
                assert_eq!(stdout.trim(), "partial");
            }
            other => panic!("expected RE, got {other:?}"),
        }
        match verdict_of(
            exec("echo oops >&2; exit 3", 2000, OUTPUT_LIMIT_BYTES),
            "",
            2000,
        ) {
            Verdict::RuntimeError { error, .. } => {
                assert!(error.contains("oops"), "{error}");
                assert!(error.contains("exited with code 3"), "{error}");
            }
            other => panic!("expected RE, got {other:?}"),
        }
    }

    #[cfg(unix)]
    #[test]
    fn successful_runs_are_judged_and_measured() {
        match verdict_of(exec("echo '1   2'", 2000, OUTPUT_LIMIT_BYTES), "1 2", 2000) {
            Verdict::Success {
                accepted,
                memory_kb,
                ..
            } => {
                assert!(accepted, "token checker ignores spacing");
                assert!(memory_kb.is_some_and(|kb| kb > 0), "{memory_kb:?}");
            }
            other => panic!("expected success, got {other:?}"),
        }
        match verdict_of(exec("echo 3", 2000, OUTPUT_LIMIT_BYTES), "4", 2000) {
            Verdict::Success { accepted, .. } => assert!(!accepted),
            other => panic!("expected success, got {other:?}"),
        }
    }

    #[test]
    fn compiled_binaries_survive_a_restart() {
        if std::process::Command::new("g++")
            .arg("--version")
            .output()
            .is_err()
        {
            return;
        }
        let root = std::env::temp_dir().join(format!("zcph_runner_restart_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let fs_ops = FsOps::with_root(root.clone());
        let code_file = root.join("Restart.cpp");
        std::fs::write(&code_file, "int main() { return 0; }\n").unwrap();
        let problem = ActiveProblem {
            name: "Restart".into(),
            url: String::new(),
            time_limit: 3000,
            test_cases: vec![ReceivingTestCase {
                input: String::new(),
                expected_output: String::new(),
            }],
            code_file,
            checker: None,
        };

        let run = |run_id: u64| {
            // A fresh runner each time, as after quitting and restarting zch.
            let (main_tx, main_rx) = channel();
            let runner_tx = spawn_runner(main_tx, fs_ops.clone());
            runner_tx
                .send(RunnerCommand::RunCode {
                    problem: problem.clone(),
                    run_id,
                    cancel: Arc::new(AtomicBool::new(false)),
                })
                .unwrap();
            collect_run(&main_rx, run_id, Duration::from_secs(20)).0
        };
        let phases = run(1);
        assert!(
            phases.iter().any(|p| p.contains("compiled in")),
            "{phases:?}"
        );
        let phases = run(2);
        assert!(
            phases.iter().any(|p| p.contains("compile skipped")),
            "{phases:?}"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn python_files_run_as_python_whatever_is_selected() {
        if std::process::Command::new("python3")
            .arg("--version")
            .output()
            .is_err()
        {
            return;
        }
        let root = std::env::temp_dir().join(format!("zcph_runner_ext_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let fs_ops = FsOps::with_root(root.clone());
        // C++ is selected, but the file is Python.
        assert!(fs_ops.save_config(&UserConfig {
            language: Language::Cpp,
            extra_flags: vec!["-O2".into()],
            ..Default::default()
        }));
        let code_file = root.join("Echo.py");
        std::fs::write(&code_file, "print(int(input()) * 2)\n").unwrap();
        let problem = ActiveProblem {
            name: "Echo".into(),
            url: String::new(),
            time_limit: 3000,
            test_cases: vec![ReceivingTestCase {
                input: "21".into(),
                expected_output: "42".into(),
            }],
            code_file,
            checker: None,
        };

        let (main_tx, main_rx) = channel();
        let runner_tx = spawn_runner(main_tx, fs_ops);
        runner_tx
            .send(RunnerCommand::RunCode {
                problem,
                run_id: 1,
                cancel: Arc::new(AtomicBool::new(false)),
            })
            .unwrap();
        let (phases, verdicts) = collect_run(&main_rx, 1, Duration::from_secs(20));
        assert!(
            !phases.iter().any(|p| p.contains("compil")),
            "Python must not be compiled: {phases:?}"
        );
        assert!(
            matches!(
                verdicts.first(),
                Some(Verdict::Success { accepted: true, .. })
            ),
            "{verdicts:?}"
        );

        let _ = std::fs::remove_dir_all(&root);
    }
}
