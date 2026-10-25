use std::{
    env,
    io::stdout,
    path::Path,
    process::{Command, Stdio},
    sync::mpsc::channel,
    time::{Duration, Instant},
};

use ratatui::{
    DefaultTerminal,
    crossterm::{
        event::{self, DisableMouseCapture, EnableMouseCapture},
        execute,
    },
};

use crate::{
    core::{
        HelperCommand,
        PassingCommand::{self, ToHelper, ToRunner, ToSubmitter},
        fs_ops::FsOps,
        receiver::spawn_server,
        runner::spawn_runner,
        submitter::spawn_submitter,
    },
    helper::Helper,
};

mod components;
mod core;
mod error;
mod helper;

fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

    let args: Vec<String> = env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("send") => {
            let action = args.get(1).map(String::as_str).unwrap_or_default();
            let file = args.get(2).map(String::as_str);
            if !matches!(action, "r" | "a" | "e" | "s") {
                if !action.is_empty() {
                    eprintln!("invalid action: {action}\n");
                }
                print_help();
                std::process::exit(2);
            }
            return send_command(action, file);
        }
        Some("-h" | "--help" | "help") => {
            print_help();
            return Ok(());
        }
        Some(other) => {
            eprintln!("unknown argument: {other}\n");
            print_help();
            std::process::exit(2);
        }
        None => {}
    }

    run_tui()
}

fn print_help() {
    println!(
        "zch — competitive programming helper\n\n\
         USAGE:\n\
         \x20 zch                          start the TUI (listens on 127.0.0.1:10043,\n\
         \x20                              saves received problems to the cwd)\n\
         \x20 zch send <action> [file]     talk to a running instance\n\
         \x20                              actions: r=run · e=edit · a=add · s=submit\n\
         \x20                              (submissions are confirmed and tracked in the TUI)\n\n\
         ENV:\n\
         \x20 ZCH_PORT   override the listener port (default 10043)\n"
    );
}

/// Client mode: used by Zed tasks to drive a running TUI, e.g.
/// `zch send r "${ZED_FILE}"`.
fn send_command(action: &str, file: Option<&str>) -> color_eyre::Result<()> {
    use std::io::{Read, Write};

    let port = env::var("ZCH_PORT").ok().and_then(|p| p.parse::<u16>().ok()).unwrap_or(10043);
    let mut path = format!("/command?action={}", core::receiver::url_encode(action));
    if let Some(file) = file
        && !file.is_empty()
    {
        path.push_str(&format!("&file={}", core::receiver::url_encode(file)));
    }

    let address = format!("127.0.0.1:{port}");
    let mut stream = std::net::TcpStream::connect(&address).map_err(|e| {
        color_eyre::eyre::eyre!("listener not running on {address} ({e}) — start zch first")
    })?;
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    stream.write_all(format!("GET {path} HTTP/1.0\r\nHost: {address}\r\n\r\n").as_bytes())?;

    let mut response = String::new();
    let _ = stream.read_to_string(&mut response);

    let (head, body) = response.split_once("\r\n\r\n").unwrap_or((response.as_str(), ""));
    let status = head.lines().next().unwrap_or_default();
    let body = body.trim();
    if status.contains(" 200 ") {
        if !body.is_empty() {
            println!("{body}");
        }
        Ok(())
    } else if body.is_empty() {
        color_eyre::eyre::bail!("listener rejected the command ({status})")
    } else {
        color_eyre::eyre::bail!("{body}")
    }
}

fn run_tui() -> color_eyre::Result<()> {
    let fs_ops = FsOps::new();
    fs_ops.prune_binaries();
    let initial_dir = env::current_dir().unwrap_or_default();

    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = execute!(stdout(), DisableMouseCapture);
        ratatui::restore();
        default_hook(info);
    }));

    let mut terminal = ratatui::init();
    let mut helper = Helper::new(initial_dir, &fs_ops);

    let _ = execute!(stdout(), EnableMouseCapture);
    let result = run_app(&mut terminal, &mut helper);
    let _ = execute!(stdout(), DisableMouseCapture);

    ratatui::restore();
    result
}

/*
 * Threads:
 *   main     — UI + event loop (owns all state)
 *   listener — Competitive Companion HTTP server (:10043)
 *   runner   — compiles + runs test cases
 *   submitter— Safari submission automation (Codeforces)
 *
 * Workers never talk to each other; everything flows through `main_tx`.
 */
fn run_app(terminal: &mut DefaultTerminal, helper: &mut Helper) -> color_eyre::Result<()> {
    let (main_tx, main_rx) = channel::<PassingCommand>();
    let runner_tx = spawn_runner(main_tx.clone(), helper.fs_ops.clone());
    let submitter_tx = spawn_submitter(main_tx.clone());
    spawn_server(main_tx.clone());

    let mut last_tick = Instant::now();
    loop {
        let mut dirty = helper.dirty;
        helper.dirty = false;

        while let Ok(message) = main_rx.try_recv() {
            dirty = true;
            match message {
                ToRunner(command) => {
                    let _ = runner_tx.send(command);
                }
                ToSubmitter(command) => {
                    let _ = submitter_tx.send(command);
                }
                ToHelper(command) => match command {
                    HelperCommand::New(problem) => {
                        if let Some(path) = helper.new_problem(problem) {
                            open_in_zed(&path);
                        }
                    }
                    HelperCommand::Command { file, action, reply } => {
                        let (command, outcome) = helper.external_command(file, action);
                        let _ = reply.send(outcome);
                        if let Some(command) = command {
                            let _ = main_tx.send(command);
                        }
                    }
                    HelperCommand::RunPhase { run_id, message } => helper.run_phase(run_id, message),
                    HelperCommand::CaseResult { run_id, index, verdict } => {
                        helper.apply_case_result(run_id, index, verdict)
                    }
                    HelperCommand::RunFinished { run_id, elapsed_ms } => {
                        helper.run_finished(run_id, elapsed_ms)
                    }
                    HelperCommand::SubmitStatus { id, message } => helper.submit_status(id, message),
                    HelperCommand::SubmitFinished { id, result } => helper.submit_finished(id, result),
                    HelperCommand::Error(error) => helper.set_error(error),
                    HelperCommand::Quit => return Ok(()),
                },
            }
        }

        if (helper.busy() || helper.tc_list.testcases.is_empty())
            && last_tick.elapsed() >= Duration::from_millis(110)
        {
            helper.tick();
            last_tick = Instant::now();
            dirty = true;
        }

        if event::poll(Duration::from_millis(16))? {
            while event::poll(Duration::from_millis(0))? {
                let event = event::read()?;
                if let Some(reply) = helper.handle_event(event) {
                    if matches!(reply, ToHelper(HelperCommand::Quit)) {
                        return Ok(());
                    }
                    let _ = main_tx.send(reply);
                }
                dirty = true;
            }
        }

        if dirty {
            terminal.draw(|frame| helper.draw(frame))?;
        }
    }
}

fn open_in_zed(path: &Path) {
    for bin in ["zed", "/usr/local/bin/zed", "/opt/homebrew/bin/zed"] {
        if Command::new(bin)
            .arg("-a")
            .arg(path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .is_ok()
        {
            return;
        }
    }
}
