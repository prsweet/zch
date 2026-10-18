use std::{
    sync::mpsc::{Sender, channel},
    thread,
    time::{Duration, Instant},
};

use crate::core::{
    ActiveProblem, HelperCommand, PassingCommand, RunnerCommand, UserConfig, Verdict,
    fs_ops::FsOps,
};

const POLL_INTERVAL: Duration = Duration::from_millis(1);

pub fn spawn_runner(main_tx: Sender<PassingCommand>, fs_ops: FsOps) -> Sender<RunnerCommand> {
    let (tx, rx) = channel::<RunnerCommand>();
    thread::spawn(move || {
        while let Ok(RunnerCommand::RunCode { problem, run_id }) = rx.recv() {
            run_streaming(&problem, run_id, &fs_ops, &main_tx);
        }
    });
    tx
}

/// Compiles once, then streams every verdict to the UI as soon as the case finishes.
fn run_streaming(
    problem: &ActiveProblem,
    run_id: u64,
    fs_ops: &FsOps,
    tx: &Sender<PassingCommand>,
) {
    let (config, _) = fs_ops.load_config();

    if let Err(error) = compile(problem, &config, fs_ops) {
        let count = problem.test_cases.len().max(1);
        for index in 0..count {
            let sent = tx.send(PassingCommand::ToHelper(HelperCommand::CaseResult {
                run_id,
                index,
                verdict: Verdict::CompilationError { error: error.clone() },
            }));
            if sent.is_err() {
                return;
            }
        }
        let _ = tx.send(PassingCommand::ToHelper(HelperCommand::RunFinished { run_id }));
        return;
    }

    let binary_path = fs_ops.binary_dir.join(&problem.name);
    let (program, args) = config.language.run_cmd(&problem.code_file, &binary_path);
    let time_limit = Duration::from_millis(problem.effective_time_limit());

    for (index, tc) in problem.test_cases.iter().enumerate() {
        let verdict = run_single(&program, &args, &tc.input, time_limit);
        let sent = tx.send(PassingCommand::ToHelper(HelperCommand::CaseResult {
            run_id,
            index,
            verdict,
        }));
        if sent.is_err() {
            return;
        }
    }

    let _ = tx.send(PassingCommand::ToHelper(HelperCommand::RunFinished { run_id }));
}

fn compile(problem: &ActiveProblem, config: &UserConfig, fs_ops: &FsOps) -> Result<(), String> {
    if !problem.code_file.exists() {
        return Err(format!("source file not found: {}", problem.code_file.display()));
    }

    let binary_path = fs_ops.binary_dir.join(&problem.name);
    let Some((program, args)) =
        config.language.compile_cmd(&problem.code_file, &binary_path, &config.extra_flags)
    else {
        return Ok(());
    };

    match duct::cmd(&program, &args)
        .stdout_capture()
        .stderr_capture()
        .unchecked()
        .run()
    {
        Err(e) => Err(format!("failed to start {program}: {e}")),
        Ok(output) if !output.status.success() => {
            let mut error = String::from_utf8_lossy(&output.stderr).to_string();
            if error.trim().is_empty() {
                error = String::from_utf8_lossy(&output.stdout).to_string();
            }
            Err(error.trim_end().to_string())
        }
        Ok(_) => Ok(()),
    }
}

fn run_single(program: &str, args: &[String], input: &str, time_limit: Duration) -> Verdict {
    let mut stdin = input.to_string();
    if !stdin.ends_with('\n') {
        stdin.push('\n');
    }

    let start = Instant::now();
    let handle = match duct::cmd(program, args)
        .stdin_bytes(stdin.into_bytes())
        .stdout_capture()
        .stderr_capture()
        .unchecked()
        .start()
    {
        Ok(handle) => handle,
        Err(e) => {
            return Verdict::RuntimeError {
                error: format!("failed to start {program}: {e}"),
            };
        }
    };

    loop {
        match handle.try_wait() {
            Ok(Some(output)) => {
                let time = start.elapsed().as_millis();
                return if output.status.success() {
                    Verdict::Success { output: output.clone(), time }
                } else {
                    let stderr = String::from_utf8_lossy(&output.stderr).trim_end().to_string();
                    let error = if stderr.is_empty() {
                        format!("exited with {}", output.status)
                    } else {
                        stderr
                    };
                    Verdict::RuntimeError { error }
                };
            }
            Ok(None) => {
                if start.elapsed() >= time_limit {
                    let _ = handle.kill();
                    return Verdict::TimeLimitExceeded;
                }
                thread::sleep(POLL_INTERVAL);
            }
            Err(e) => {
                return Verdict::RuntimeError {
                    error: format!("failed to read process status: {e}"),
                };
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{Language, PassingCommand, ReceivingTestCase};
    use std::sync::mpsc::channel;
    use std::time::Instant;

    #[test]
    fn streams_results_before_the_run_finishes() {
        let root = std::env::temp_dir().join(format!("zcph_runner_stream_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let fs_ops = FsOps::with_root(root.clone());
        assert!(fs_ops.save_config(&UserConfig { language: Language::Python, extra_flags: vec![] }));

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
        runner_tx.send(RunnerCommand::RunCode { problem, run_id: 7 }).unwrap();

        let start = Instant::now();
        let mut first_case_at = None;
        let mut finished_at = None;
        while let Ok(message) = main_rx.recv_timeout(Duration::from_secs(5)) {
            if let PassingCommand::ToHelper(command) = message {
                match command {
                    HelperCommand::CaseResult { run_id: 7, index: 0, .. } => {
                        first_case_at = Some(start.elapsed());
                    }
                    HelperCommand::RunFinished { run_id: 7 } => {
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
}
