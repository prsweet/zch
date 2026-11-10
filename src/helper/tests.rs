use super::*;
use crate::core::{Language, ReceivingTestCase, UserConfig};
use ratatui::{
    Terminal,
    backend::TestBackend,
    crossterm::event::{KeyEvent, KeyModifiers, MouseEvent, MouseEventKind},
};
use std::process::{Command, Output};

fn temp_root(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("zcph_test_{tag}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn sample_problem() -> ActiveProblem {
    ActiveProblem {
        name: "E Prime Destruction".into(),
        url: "https://codeforces.com/contest/2266/problem/E".into(),
        time_limit: 2000,
        test_cases: vec![
            ReceivingTestCase {
                input: "1 2".into(),
                expected_output: "3".into(),
            },
            ReceivingTestCase {
                input: "5".into(),
                expected_output: "10".into(),
            },
        ],
        code_file: PathBuf::new(),
        checker: None,
    }
}

fn ok_output(stdout: &str) -> Output {
    Output {
        status: Command::new("true").status().unwrap(),
        stdout: stdout.as_bytes().to_vec(),
        stderr: Vec::new(),
    }
}

/// A clean exit, already judged by the runner's checker.
fn success(stdout: &str, time: u128, accepted: bool) -> Verdict {
    Verdict::Success {
        output: ok_output(stdout),
        time,
        memory_kb: None,
        accepted,
        note: None,
    }
}

fn key(code: KeyCode) -> Event {
    Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
}

fn type_text(helper: &mut Helper, text: &str) {
    for c in text.chars() {
        helper.handle_event(key(KeyCode::Char(c)));
    }
}

fn stream(helper: &mut Helper, results: Vec<Verdict>) {
    for (index, verdict) in results.into_iter().enumerate() {
        helper.apply_case_result(helper.run_id, index, verdict);
    }
    helper.run_finished(helper.run_id, 0);
}

#[test]
fn same_name_problems_keep_separate_state() {
    let root = temp_root("identity");
    let fs_ops = FsOps::with_root(root.clone());
    let work = root.join("problems");
    std::fs::create_dir_all(&work).unwrap();
    let mut helper = Helper::new(work, &fs_ops);

    let mut first = sample_problem();
    first.name = "A. Shared".into();
    first.url = "https://codeforces.com/contest/1111/problem/A".into();
    let first_path = helper.new_problem(first).unwrap();

    let mut second = sample_problem();
    second.name = "A. Shared".into();
    second.url = "https://codeforces.com/contest/2222/problem/A".into();
    second.test_cases = vec![ReceivingTestCase {
        input: "9".into(),
        expected_output: "9".into(),
    }];
    let second_path = helper.new_problem(second).unwrap();

    assert_eq!(
        first_path.file_stem().unwrap().to_string_lossy(),
        "A__Shared"
    );
    let second_stem = second_path
        .file_stem()
        .unwrap()
        .to_string_lossy()
        .to_string();
    assert!(second_stem.starts_with("A__Shared__"), "{second_stem}");
    assert_ne!(first_path, second_path);

    let first_saved = fs_ops.load_problem_by_stem("A__Shared").unwrap();
    assert_eq!(first_saved.test_cases[0].input, "1 2");
    let second_saved = fs_ops.load_problem_by_stem(&second_stem).unwrap();
    assert_eq!(second_saved.test_cases[0].input, "9");
    assert_eq!(
        second_saved.url,
        "https://codeforces.com/contest/2222/problem/A"
    );
    assert_ne!(
        fs_ops.binary_dir.join(&first_saved.name),
        fs_ops.binary_dir.join(&second_saved.name)
    );

    // Re-receiving the same URL must reuse its suffix, not add another one.
    let mut again = sample_problem();
    again.name = "A. Shared".into();
    again.url = "https://codeforces.com/contest/2222/problem/A".into();
    assert_eq!(helper.new_problem(again).unwrap(), second_path);

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn anonymous_problems_with_the_same_name_do_not_overwrite() {
    let root = temp_root("anonymous-identity");
    let fs_ops = FsOps::with_root(root.clone());
    let work = root.join("problems");
    std::fs::create_dir_all(&work).unwrap();
    let mut helper = Helper::new(work, &fs_ops);

    let mut first = sample_problem();
    first.name = "A. Anonymous".into();
    first.url.clear();
    let first_path = helper.new_problem(first).unwrap();

    let mut second = sample_problem();
    second.name = "A. Anonymous".into();
    second.url.clear();
    second.test_cases = vec![ReceivingTestCase {
        input: "9".into(),
        expected_output: "9".into(),
    }];
    let second_path = helper.new_problem(second).unwrap();

    assert_eq!(
        first_path.file_stem().unwrap().to_string_lossy(),
        "A__Anonymous"
    );
    assert_eq!(
        second_path.file_stem().unwrap().to_string_lossy(),
        "A__Anonymous__2"
    );
    assert_eq!(
        fs_ops
            .load_problem_by_stem("A__Anonymous")
            .unwrap()
            .test_cases[0]
            .input,
        "1 2"
    );
    assert_eq!(
        fs_ops
            .load_problem_by_stem("A__Anonymous__2")
            .unwrap()
            .test_cases[0]
            .input,
        "9"
    );

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn renders_problem_and_results() {
    let root = temp_root("render");
    let fs_ops = FsOps::with_root(root.clone());
    let work = root.join("problems");
    std::fs::create_dir_all(&work).unwrap();

    let mut helper = Helper::new(work.clone(), &fs_ops);
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    terminal.draw(|f| helper.draw(f)).unwrap();

    let code_path = helper.new_problem(sample_problem()).unwrap();
    assert!(code_path.exists());
    let content = std::fs::read_to_string(&code_path).unwrap();
    assert!(content.starts_with("// URL: https://codeforces.com/contest/2266/problem/E"));

    let saved = fs_ops.load_problem_by_stem("E_Prime_Destruction").unwrap();
    assert_eq!(saved.test_cases.len(), 2);
    assert_eq!(helper.tc_list.testcases.len(), 2);

    let results = [success("3\n", 4, true), success("0\n", 9, false)];
    // Results are applied one at a time, exactly as they arrive from the runner.
    helper.apply_case_result(helper.run_id, 0, results[0].clone());
    assert!(matches!(
        helper.tc_list.testcases[0].verdict,
        CaseVerdict::AC { ms: 4 }
    ));
    assert!(matches!(
        helper.tc_list.testcases[1].verdict,
        CaseVerdict::Pending
    ));

    helper.apply_case_result(helper.run_id, 1, results[1].clone());
    helper.run_finished(helper.run_id, 0);
    terminal.draw(|f| helper.draw(f)).unwrap();

    assert!(matches!(
        helper.tc_list.testcases[1].verdict,
        CaseVerdict::WA { ms: 9 }
    ));
    assert!(helper.tc_list.testcases[0].collapsed);
    assert!(!helper.running);

    let failures = vec![
        Verdict::TimeLimitExceeded {
            time: 2400,
            limit: 2000,
            killed: false,
            stdout: String::new(),
        },
        Verdict::CompilationError {
            error: "boom".into(),
        },
    ];
    stream(&mut helper, failures);
    terminal.draw(|f| helper.draw(f)).unwrap();
    assert!(matches!(
        helper.tc_list.testcases[0].verdict,
        CaseVerdict::Tle
    ));
    assert!(matches!(
        helper.tc_list.testcases[1].verdict,
        CaseVerdict::CE
    ));

    dump_screen(&terminal);

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn cancelling_a_modal_does_not_echo_into_the_console() {
    let root = temp_root("cancel_quiet");
    let fs_ops = FsOps::with_root(root.clone());
    let work = root.join("problems");
    std::fs::create_dir_all(&work).unwrap();

    let mut helper = Helper::new(work, &fs_ops);
    helper.new_problem(sample_problem());
    helper.status.success("2 of 2 passed");

    helper.handle_event(key(KeyCode::Char('x')));
    assert!(matches!(helper.input_mode, InputMode::DeleteSelect));
    helper.handle_event(key(KeyCode::Esc));
    assert!(matches!(helper.input_mode, InputMode::Normal));
    assert_eq!(
        console_text(&helper),
        "2 of 2 passed",
        "cancelling a delete keeps the last result"
    );

    helper.handle_event(key(KeyCode::Char('s')));
    assert!(matches!(helper.input_mode, InputMode::ConfirmSubmit));
    helper.handle_event(key(KeyCode::Backspace));
    assert!(matches!(helper.input_mode, InputMode::Normal));
    assert_eq!(
        console_text(&helper),
        "2 of 2 passed",
        "cancelling a submission keeps the last result"
    );

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_new_problem_replaces_the_previous_result_with_its_own() {
    let root = temp_root("attach_console");
    let fs_ops = FsOps::with_root(root.clone());
    let work = root.join("problems");
    std::fs::create_dir_all(&work).unwrap();

    let mut helper = Helper::new(work, &fs_ops);
    helper.new_problem(sample_problem());
    helper.status.success("2 of 2 passed");

    let mut next = sample_problem();
    next.name = "Another Problem".into();
    next.url = "https://codeforces.com/contest/1/problem/A".into();
    helper.new_problem(next);
    assert_eq!(
        console_text(&helper),
        "received · 2 samples",
        "the previous problem's result must not leak into the new one"
    );
    assert_eq!(helper.status.tone(), Some(Tone::Success));

    let _ = std::fs::remove_dir_all(&root);
}

/// `ZCH_DUMP=1 cargo test renders_problem -- --nocapture` to eyeball the UI.
fn dump_screen(terminal: &Terminal<TestBackend>) {
    if std::env::var("ZCH_DUMP").is_err() {
        return;
    }
    let buffer = terminal.backend().buffer();
    let area = buffer.area;
    for y in 0..area.height {
        let mut line = String::new();
        for x in 0..area.width {
            line.push_str(buffer[(x, y)].symbol());
        }
        println!("|{line}|");
    }
}

#[test]
fn a_drag_selects_inside_one_pane_and_copies_on_release() {
    let root = temp_root("text_select");
    let fs_ops = FsOps::with_root(root.clone());
    let work = root.join("problems");
    std::fs::create_dir_all(&work).unwrap();

    let mut helper = Helper::new(work, &fs_ops);
    helper.new_problem(sample_problem());
    stream(
        &mut helper,
        vec![success("3\n", 1, false), success("7\n", 1, false)],
    );
    let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
    terminal.draw(|f| helper.draw(f)).unwrap();
    let mouse = |kind, column, row| {
        Event::Mouse(MouseEvent {
            kind,
            column,
            row,
            modifiers: KeyModifiers::NONE,
        })
    };
    let [input, _, got] = helper.tc_list.testcases[0].text_regions();

    // A click is still a click. (On another pane: two quick clicks on
    // the same one are a double-click, which opens the large viewer.)
    helper.handle_event(mouse(
        MouseEventKind::Down(MouseButton::Left),
        input.x,
        input.y,
    ));
    helper.handle_event(mouse(
        MouseEventKind::Up(MouseButton::Left),
        input.x,
        input.y,
    ));
    assert!(
        helper.text_selection.is_none(),
        "a click is not a selection"
    );

    // A drag from the got pane far past it stays inside the got pane.
    helper.handle_event(mouse(MouseEventKind::Down(MouseButton::Left), got.x, got.y));
    helper.handle_event(mouse(MouseEventKind::Drag(MouseButton::Left), 99, 29));
    let selection = helper.text_selection.expect("a drag selects");
    assert_eq!(selection.bounds, got);
    assert_eq!(selected_text(&helper.screen_rows, selection), "3");

    // While held, the cells get a light wash; the input pane does not.
    terminal.draw(|f| helper.draw(f)).unwrap();
    let buffer = terminal.backend().buffer();
    // The got pane is a tinted well; the wash lightens that.
    let washed = theme::blend(theme::got_rgb(), theme::p().text, 0.3);
    assert_eq!(buffer[(got.x, got.y)].bg, washed);
    assert_ne!(buffer[(input.x, input.y)].bg, washed);

    // Releasing copies and clears the selection.
    helper.handle_event(mouse(MouseEventKind::Up(MouseButton::Left), 99, 29));
    assert!(
        helper.text_selection.is_none(),
        "the selection goes on release"
    );
    assert_eq!(console_text(&helper), "copied 1 character");

    let _ = std::fs::remove_dir_all(&root);
}

/// The status line's text, without its glyph.
fn console_text(helper: &Helper) -> String {
    helper.status.text().unwrap_or_default().to_string()
}

fn screen_text(terminal: &Terminal<TestBackend>) -> String {
    let buffer = terminal.backend().buffer();
    let area = buffer.area;
    (0..area.height)
        .map(|y| {
            (0..area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn row_text(terminal: &Terminal<TestBackend>, y: u16) -> String {
    let buffer = terminal.backend().buffer();
    (0..buffer.area.width)
        .map(|x| buffer[(x, y)].symbol())
        .collect()
}

#[test]
fn add_edit_and_persist_custom_case() {
    let root = temp_root("edit");
    let fs_ops = FsOps::with_root(root.clone());
    let work = root.join("problems");
    std::fs::create_dir_all(&work).unwrap();

    let mut helper = Helper::new(work, &fs_ops);
    helper.new_problem(sample_problem());

    helper.handle_event(key(KeyCode::Char('a')));
    assert!(matches!(helper.input_mode, InputMode::Editing));
    assert_eq!(helper.tc_list.testcases.len(), 3);

    type_text(&mut helper, "7 8");
    helper.handle_event(key(KeyCode::Tab));
    type_text(&mut helper, "15");
    helper.handle_event(key(KeyCode::Esc));
    assert!(matches!(helper.input_mode, InputMode::Normal));

    let saved = fs_ops.load_problem_by_stem("E_Prime_Destruction").unwrap();
    assert_eq!(saved.test_cases.len(), 3);
    assert_eq!(saved.test_cases[2].input, "7 8");
    assert_eq!(saved.test_cases[2].expected_output, "15");

    // An added case left completely empty is dropped on save.
    helper.handle_event(key(KeyCode::Char('a')));
    helper.handle_event(key(KeyCode::Esc));
    assert_eq!(helper.tc_list.testcases.len(), 3);

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn adding_a_case_keeps_tab_inside_that_case() {
    let root = temp_root("add_stay");
    let fs_ops = FsOps::with_root(root.clone());
    let work = root.join("problems");
    std::fs::create_dir_all(&work).unwrap();

    let mut helper = Helper::new(work, &fs_ops);
    helper.new_problem(sample_problem());
    helper.handle_event(key(KeyCode::Char('a')));
    let new_case = helper.tc_list.active_area;
    assert_eq!(new_case, 2);
    assert_eq!(
        helper.tc_list.active_box,
        crate::components::testcase::ActiveBox::Input
    );

    helper.handle_event(key(KeyCode::Tab));
    assert_eq!(helper.tc_list.active_area, new_case);
    assert_eq!(
        helper.tc_list.active_box,
        crate::components::testcase::ActiveBox::Expected
    );

    helper.handle_event(key(KeyCode::Tab));
    assert_eq!(helper.tc_list.active_area, new_case);
    assert_eq!(
        helper.tc_list.active_box,
        crate::components::testcase::ActiveBox::Input
    );

    helper.handle_event(Event::Key(KeyEvent::new(
        KeyCode::BackTab,
        KeyModifiers::SHIFT,
    )));
    assert_eq!(helper.tc_list.active_area, new_case);
    assert_eq!(
        helper.tc_list.active_box,
        crate::components::testcase::ActiveBox::Expected
    );

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn edit_mode_still_walks_cases() {
    let root = temp_root("edit_walk");
    let fs_ops = FsOps::with_root(root.clone());
    let work = root.join("problems");
    std::fs::create_dir_all(&work).unwrap();

    let mut helper = Helper::new(work, &fs_ops);
    helper.new_problem(sample_problem());
    helper.handle_event(key(KeyCode::Char('e')));
    assert_eq!(helper.tc_list.active_area, 0);

    helper.handle_event(key(KeyCode::Tab));
    assert_eq!(helper.tc_list.active_area, 0);
    helper.handle_event(key(KeyCode::Tab));
    assert_eq!(helper.tc_list.active_area, 1);

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn delete_selects_and_asks_for_confirmation() {
    let root = temp_root("delete");
    let fs_ops = FsOps::with_root(root.clone());
    let work = root.join("problems");
    std::fs::create_dir_all(&work).unwrap();

    let mut helper = Helper::new(work, &fs_ops);
    helper.new_problem(sample_problem());
    helper.running = true;

    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();

    helper.tc_list.testcases[0].collapsed = true;
    helper.tc_list.testcases[1].collapsed = true;
    helper.tc_list.active_area = 0;
    helper.handle_event(key(KeyCode::Char('x')));
    assert!(matches!(helper.input_mode, InputMode::DeleteSelect));
    assert_eq!(helper.tc_list.testcases.len(), 2, "x only starts the flow");
    assert!(
        helper.tc_list.testcases.iter().all(|tc| !tc.collapsed),
        "delete selection must show every case open"
    );
    assert!(
        !helper.running,
        "an in-flight run is retired before the list shifts"
    );
    terminal.draw(|f| helper.draw(f)).unwrap();
    dump_screen(&terminal);

    // Tab picks the next case; the selection wraps in both directions.
    helper.handle_event(key(KeyCode::Tab));
    assert_eq!(helper.tc_list.active_area, 1);
    helper.handle_event(key(KeyCode::Tab));
    assert_eq!(helper.tc_list.active_area, 0, "selection wraps around");
    helper.handle_event(key(KeyCode::BackTab));
    assert_eq!(helper.tc_list.active_area, 1);

    // Enter opens the confirmation; Esc there goes back to the selection.
    helper.handle_event(key(KeyCode::Enter));
    assert!(matches!(helper.input_mode, InputMode::ConfirmDelete));
    assert_eq!(
        helper.tc_list.testcases.len(),
        2,
        "confirmation must not delete yet"
    );
    terminal.draw(|f| helper.draw(f)).unwrap();
    dump_screen(&terminal);
    helper.handle_event(key(KeyCode::Esc));
    assert!(matches!(helper.input_mode, InputMode::DeleteSelect));

    // Esc leaves the flow entirely; nothing is deleted.
    helper.handle_event(key(KeyCode::Esc));
    assert!(matches!(helper.input_mode, InputMode::Normal));
    assert_eq!(helper.tc_list.testcases.len(), 2);

    // x -> Enter -> Enter deletes the selected case and persists the rest.
    helper.handle_event(key(KeyCode::Char('x')));
    helper.handle_event(key(KeyCode::Enter));
    helper.handle_event(key(KeyCode::Enter));
    assert!(matches!(helper.input_mode, InputMode::Normal));
    assert_eq!(helper.tc_list.testcases.len(), 1);
    assert_eq!(helper.tc_list.active_area, 0);
    let saved = fs_ops.load_problem_by_stem("E_Prime_Destruction").unwrap();
    assert_eq!(saved.test_cases.len(), 1);
    assert_eq!(saved.test_cases[0].input, "1 2");

    // Deleting the last case leaves an empty list without panicking.
    helper.handle_event(key(KeyCode::Char('x')));
    helper.handle_event(key(KeyCode::Enter));
    helper.handle_event(key(KeyCode::Enter));
    assert!(helper.tc_list.testcases.is_empty());
    let saved = fs_ops.load_problem_by_stem("E_Prime_Destruction").unwrap();
    assert!(saved.test_cases.is_empty());

    // With an empty list x just reports it and stays in Normal mode.
    helper.handle_event(key(KeyCode::Char('x')));
    assert!(matches!(helper.input_mode, InputMode::Normal));

    // In Editing mode `x` is a character, not the delete command.
    helper.handle_event(key(KeyCode::Char('a')));
    helper.handle_event(key(KeyCode::Char('x')));
    assert_eq!(helper.tc_list.testcases.len(), 1);
    helper.handle_event(key(KeyCode::Esc));
    assert_eq!(helper.tc_list.testcases.len(), 1);
    assert_eq!(
        helper.tc_list.testcases[0].inp_content.lines().join("\n"),
        "x"
    );

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn config_menu_cycles_language_and_saves() {
    let root = temp_root("config");
    let fs_ops = FsOps::with_root(root.clone());
    let work = root.join("problems");
    std::fs::create_dir_all(&work).unwrap();

    let mut helper = Helper::new(work, &fs_ops);
    let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();

    helper.handle_event(key(KeyCode::Char('c')));
    assert!(matches!(helper.input_mode, InputMode::Config));
    terminal.draw(|f| helper.draw(f)).unwrap();

    helper.handle_event(key(KeyCode::Tab));
    type_text(&mut helper, "-O2");
    helper.handle_event(key(KeyCode::Enter));
    helper.handle_event(key(KeyCode::BackTab));
    helper.handle_event(key(KeyCode::Right));
    assert_eq!(helper.config_menu.language, Language::Python);
    assert!(helper.config_menu.flags.is_empty());

    helper.handle_event(key(KeyCode::Tab));
    type_text(&mut helper, "-u");
    helper.handle_event(key(KeyCode::Enter));
    terminal.draw(|f| helper.draw(f)).unwrap();
    dump_screen(&terminal);

    helper.handle_event(key(KeyCode::Esc));
    assert!(matches!(helper.input_mode, InputMode::Normal));

    let (config, loaded) = fs_ops.load_config();
    assert!(loaded);
    assert_eq!(config.language, Language::Python);
    assert_eq!(config.extra_flags, ["-u"]);
    assert_eq!(config.flags_by_language[&Language::Cpp], ["-O2"]);
    assert_eq!(config.flags_by_language[&Language::Python], ["-u"]);

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn submit_waits_for_confirmation() {
    let root = temp_root("submit");
    let fs_ops = FsOps::with_root(root.clone());
    let work = root.join("problems");
    std::fs::create_dir_all(&work).unwrap();

    let mut helper = Helper::new(work, &fs_ops);
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    helper.new_problem(sample_problem());

    assert!(helper.handle_event(key(KeyCode::Char('s'))).is_none());
    assert!(matches!(helper.input_mode, InputMode::ConfirmSubmit));
    let pending = helper.pending_submit.as_ref().expect("pending submission");
    assert_eq!(pending.info.platform, "Codeforces");
    assert_eq!(pending.info.problem_code, "E");
    assert_eq!(
        pending.info.submit_url,
        "https://codeforces.com/contest/2266/submit"
    );
    assert_eq!(pending.info.language_id, Language::Cpp.cf_id(&[]));
    assert_eq!(pending.tests, LocalTests::NotRun { total: 2 });
    terminal.draw(|f| helper.draw(f)).unwrap();
    dump_screen(&terminal);
    let screen = screen_text(&terminal);
    assert!(screen.contains("Submit"), "{screen}");
    assert!(screen.contains("Codeforces · E"), "{screen}");
    assert!(screen.contains("not run yet · 2 cases"), "{screen}");
    assert!(
        row_text(&terminal, 39).contains("[Enter] submit"),
        "the hint row says how to confirm"
    );

    helper.handle_event(key(KeyCode::Char('x')));
    assert!(
        helper.pending_submit.is_some(),
        "other keys must not dismiss the modal"
    );

    helper.handle_event(key(KeyCode::Backspace));
    assert!(matches!(helper.input_mode, InputMode::Normal));
    assert!(helper.pending_submit.is_none());

    helper.handle_event(key(KeyCode::Char('s')));
    let command = helper.handle_event(key(KeyCode::Enter));
    assert!(matches!(
        command,
        Some(PassingCommand::ToSubmitter(SubmitterCommand::SubmitCode {
            id: 1,
            ..
        }))
    ));
    assert_eq!(helper.submissions.len(), 1);
    assert!(!helper.submissions[0].done());
    assert!(matches!(helper.input_mode, InputMode::Normal));
    assert!(helper.pending_submit.is_none());

    helper.submit_status(1, "Running on test 4".into());
    assert_eq!(helper.submissions[0].message, "running on test 4");
    assert_eq!(helper.submissions[0].tone, Tone::Progress);
    helper.submit_status(1, "captcha".into());
    assert_eq!(helper.submissions[0].tone, Tone::Warning);
    helper.submit_finished(1, "RESULT: ACCEPTED".into());
    assert!(helper.submissions[0].done());
    assert_eq!(helper.submissions[0].tone, Tone::Success);
    assert_eq!(helper.submissions[0].message, "accepted");
    assert_eq!(
        console_text(&helper),
        "received · 2 samples",
        "the verdict shows in the submissions panel, not again in the console"
    );

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn submissions_track_their_own_status_in_parallel() {
    let root = temp_root("submit_queue");
    let fs_ops = FsOps::with_root(root.clone());
    let work = root.join("problems");
    std::fs::create_dir_all(&work).unwrap();

    let mut helper = Helper::new(work, &fs_ops);
    helper.new_problem(sample_problem());

    for _ in 0..2 {
        helper.handle_event(key(KeyCode::Char('s')));
        let command = helper.handle_event(key(KeyCode::Enter));
        assert!(matches!(
            command,
            Some(PassingCommand::ToSubmitter(
                SubmitterCommand::SubmitCode { .. }
            ))
        ));
    }
    assert_eq!(helper.submissions.len(), 2);
    assert_eq!(helper.submissions[0].id, 1);
    assert_eq!(helper.submissions[1].id, 2);

    helper.submit_status(2, "judging…".into());
    assert_eq!(helper.submissions[1].message, "judging…");
    assert_eq!(helper.submissions[0].message, "opening Codeforces E");

    helper.submit_finished(1, "RESULT: ACCEPTED".into());
    assert!(helper.submissions[0].done() && !helper.submissions[1].done());
    assert_eq!(helper.submissions[1].message, "judging…");

    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    terminal.draw(|f| helper.draw(f)).unwrap();
    dump_screen(&terminal);

    for _ in 0..MAX_SUBMISSIONS {
        helper.handle_event(key(KeyCode::Char('s')));
        helper.handle_event(key(KeyCode::Enter));
    }
    assert_eq!(helper.submissions.len(), MAX_SUBMISSIONS);
    assert_eq!(helper.submissions.first().unwrap().id, 3);
    assert_eq!(
        helper.submissions.last().unwrap().id,
        2 + MAX_SUBMISSIONS as u64
    );

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn test_case_list_reserves_a_scroll_viewport() {
    let root = temp_root("list_edge");
    let fs_ops = FsOps::with_root(root.clone());
    let work = root.join("problems");
    std::fs::create_dir_all(&work).unwrap();

    let mut helper = Helper::new(work, &fs_ops);
    helper.new_problem(sample_problem());
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    terminal.draw(|f| helper.draw(f)).unwrap();

    assert_eq!(
        helper.tc_list.testcases[0].area.rect.x,
        crate::components::ui::GUTTER,
        "cases keep the screen's gutter on both sides"
    );
    assert_eq!(
        helper.tc_list.testcases[0].area.rect.y, 7,
        "the first case sits under the padded header, the rule and one quiet row"
    );

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn submissions_panel_scrolls_internally() {
    let root = temp_root("submit_scroll");
    let fs_ops = FsOps::with_root(root.clone());
    let work = root.join("problems");
    std::fs::create_dir_all(&work).unwrap();

    let mut helper = Helper::new(work, &fs_ops);
    helper.new_problem(sample_problem());
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();

    for _ in 0..(SUBMISSION_VIEWPORT + 3) {
        helper.handle_event(key(KeyCode::Char('s')));
        helper.handle_event(key(KeyCode::Enter));
    }
    assert_eq!(helper.submissions.len(), SUBMISSION_VIEWPORT + 3);

    terminal.draw(|f| helper.draw(f)).unwrap();
    let area = helper.submissions_area;
    assert_eq!(
        area.height as usize,
        SUBMISSION_VIEWPORT + 1,
        "a title row, then at most a few submissions"
    );
    let wheel = |kind| {
        Event::Mouse(MouseEvent {
            kind,
            column: area.x + 1,
            row: area.y + 1,
            modifiers: KeyModifiers::NONE,
        })
    };

    helper.handle_event(wheel(MouseEventKind::ScrollDown));
    assert_eq!(helper.submission_scroll, 1);
    helper.handle_event(wheel(MouseEventKind::ScrollDown));
    assert_eq!(helper.submission_scroll, 2);
    helper.handle_event(wheel(MouseEventKind::ScrollUp));
    assert_eq!(helper.submission_scroll, 1);

    for _ in 0..20 {
        helper.handle_event(wheel(MouseEventKind::ScrollDown));
    }
    assert_eq!(helper.submission_scroll, 3, "scroll clamps to overflow");

    helper.submission_scroll = 0;
    terminal.draw(|f| helper.draw(f)).unwrap();
    let scrollbar = helper.submissions_scrollbar_area;
    assert!(
        scrollbar.height > 2,
        "the submissions panel must expose a scrollbar"
    );
    let mouse = |kind, column, row| {
        Event::Mouse(MouseEvent {
            kind,
            column,
            row,
            modifiers: KeyModifiers::NONE,
        })
    };
    let bottom = scrollbar.y + scrollbar.height - 1;
    helper.handle_event(mouse(
        MouseEventKind::Down(MouseButton::Left),
        scrollbar.x,
        bottom,
    ));
    assert!(
        helper.submission_scroll > 0,
        "clicking the track must seek down"
    );

    helper.handle_event(mouse(
        MouseEventKind::Drag(MouseButton::Left),
        scrollbar.x,
        scrollbar.y + 1,
    ));
    assert_eq!(
        helper.submission_scroll, 0,
        "dragging to the top must seek back"
    );
    helper.handle_event(mouse(
        MouseEventKind::Up(MouseButton::Left),
        scrollbar.x,
        scrollbar.y + 1,
    ));
    assert!(helper.submissions_scrollbar_drag.is_none());

    helper.handle_event(key(KeyCode::Char('s')));
    helper.handle_event(key(KeyCode::Enter));
    assert_eq!(
        helper.submission_scroll, 0,
        "new submission snaps back to newest"
    );

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn successful_cases_do_not_churn_the_console() {
    let root = temp_root("console_lanes");
    let fs_ops = FsOps::with_root(root.clone());
    let work = root.join("problems");
    std::fs::create_dir_all(&work).unwrap();

    let mut helper = Helper::new(work, &fs_ops);
    helper.new_problem(sample_problem());
    assert!(helper.cmd_run().is_some());
    helper.run_phase(helper.run_id, "compiling…".to_string());
    assert_eq!(console_text(&helper), "compiling");
    helper.run_phase(helper.run_id, "compiled in 412ms".to_string());
    helper.run_phase(helper.run_id, "running case 1/2…".to_string());
    assert_eq!(console_text(&helper), "running 1/2");
    assert_eq!(helper.status.tone(), Some(Tone::Progress));
    helper.apply_case_result(helper.run_id, 0, success("3\n", 1, true));
    assert_eq!(
        console_text(&helper),
        "running 1/2",
        "a finished case does not churn the status line"
    );

    helper.run_phase(helper.run_id, "running case 2/2…".to_string());
    helper.apply_case_result(helper.run_id, 1, success("0\n", 2, false));
    assert_eq!(
        console_text(&helper),
        "running 2/2",
        "the card shows the failure; the status line keeps counting"
    );

    helper.run_finished(helper.run_id, 1240);
    assert_eq!(console_text(&helper), "1 of 2 failed · 1.24 s");
    assert_eq!(helper.status.tone(), Some(Tone::Error));
    assert_eq!(
        helper.tc_list.active_area, 1,
        "the first failure is brought into view"
    );

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn run_summary_uses_the_runner_elapsed_time() {
    let root = temp_root("run_elapsed");
    let fs_ops = FsOps::with_root(root.clone());
    let work = root.join("problems");
    std::fs::create_dir_all(&work).unwrap();

    let mut helper = Helper::new(work, &fs_ops);
    helper.new_problem(sample_problem());
    helper.running = true;
    helper.run_started = Some(Instant::now() - Duration::from_secs(5));
    helper.apply_case_result(helper.run_id, 0, success("3\n", 123, true));
    helper.apply_case_result(helper.run_id, 1, success("10\n", 456, true));
    helper.run_finished(helper.run_id, 397);

    let message = console_text(&helper);
    assert_eq!(
        message, "all 2 passed · 397 ms",
        "the runner's elapsed time is the total, not the per-case sum or the UI's clock"
    );

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn failures_explain_themselves() {
    let root = temp_root("failure_details");
    let fs_ops = FsOps::with_root(root.clone());
    let work = root.join("problems");
    std::fs::create_dir_all(&work).unwrap();

    let mut helper = Helper::new(work, &fs_ops);
    let mut problem = sample_problem();
    problem.test_cases.push(ReceivingTestCase {
        input: "7".into(),
        expected_output: "14".into(),
    });
    helper.new_problem(problem);

    stream(
        &mut helper,
        vec![
            Verdict::TimeLimitExceeded {
                time: 4000,
                limit: 2000,
                killed: true,
                stdout: "partial".into(),
            },
            Verdict::RuntimeError {
                error: "killed by SIGSEGV — invalid memory access".into(),
                stdout: "1 2".into(),
            },
            Verdict::OutputLimitExceeded {
                limit: 64 * 1024 * 1024,
            },
        ],
    );
    let cases = &helper.tc_list.testcases;
    assert!(matches!(cases[0].verdict, CaseVerdict::Tle));
    let got = cases[0].got_content.as_deref().unwrap();
    assert!(got.starts_with("partial"), "{got}");
    assert!(got.contains("still running after 4 s"), "{got}");

    assert!(matches!(cases[1].verdict, CaseVerdict::RE));
    let got = cases[1].got_content.as_deref().unwrap();
    assert!(got.starts_with("1 2"), "partial output is kept: {got}");
    assert!(got.contains("SIGSEGV"), "{got}");

    assert!(matches!(cases[2].verdict, CaseVerdict::Ole));
    assert!(
        cases[2]
            .got_content
            .as_deref()
            .unwrap()
            .contains("more than 64 MB")
    );

    // The checker's verdict and note come from the runner.
    stream(
        &mut helper,
        vec![Verdict::Success {
            output: ok_output("3\n"),
            time: 5,
            memory_kb: Some(3 * 1024),
            accepted: false,
            note: Some("checker: wrong answer 1st numbers differ".into()),
        }],
    );
    let case = &helper.tc_list.testcases[0];
    assert!(matches!(case.verdict, CaseVerdict::WA { ms: 5 }));
    assert_eq!(case.memory_kb, Some(3 * 1024));
    assert!(
        case.got_content
            .as_deref()
            .unwrap()
            .contains("checker: wrong answer")
    );

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn entering_edit_mode_expands_collapsed_cases() {
    let root = temp_root("edit_expand");
    let fs_ops = FsOps::with_root(root.clone());
    let work = root.join("problems");
    std::fs::create_dir_all(&work).unwrap();

    let mut helper = Helper::new(work, &fs_ops);
    helper.new_problem(sample_problem());
    stream(
        &mut helper,
        vec![success("3\n", 1, true), success("10\n", 1, true)],
    );
    assert!(
        helper.tc_list.testcases.iter().all(|tc| tc.collapsed),
        "AC cases auto-collapse"
    );

    helper.handle_event(key(KeyCode::Char('e')));
    assert!(matches!(helper.input_mode, InputMode::Editing));
    assert!(
        helper.tc_list.testcases.iter().all(|tc| !tc.collapsed),
        "every case must be visible while editing"
    );

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn shift_tab_walks_backwards_while_editing() {
    let root = temp_root("edit-back");
    let fs_ops = FsOps::with_root(root.clone());
    let work = root.join("problems");
    std::fs::create_dir_all(&work).unwrap();

    let mut helper = Helper::new(work, &fs_ops);
    helper.new_problem(sample_problem());
    helper.handle_event(key(KeyCode::Char('e')));
    assert!(matches!(helper.input_mode, InputMode::Editing));
    assert_eq!(
        helper.tc_list.active_box,
        crate::components::testcase::ActiveBox::Input
    );

    helper.handle_event(key(KeyCode::Tab));
    assert_eq!(
        helper.tc_list.active_box,
        crate::components::testcase::ActiveBox::Expected
    );
    helper.handle_event(Event::Key(KeyEvent::new(
        KeyCode::BackTab,
        KeyModifiers::SHIFT,
    )));
    assert_eq!(
        helper.tc_list.active_box,
        crate::components::testcase::ActiveBox::Input
    );
    helper.handle_event(Event::Key(KeyEvent::new(KeyCode::Tab, KeyModifiers::SHIFT)));
    assert_eq!(
        helper.tc_list.active_box,
        crate::components::testcase::ActiveBox::Expected,
        "a terminal that reports Shift+Tab as Tab must still go back"
    );

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn editing_keeps_the_cursor_line_visible() {
    let root = temp_root("edit_scroll");
    let fs_ops = FsOps::with_root(root.clone());
    let work = root.join("problems");
    std::fs::create_dir_all(&work).unwrap();

    let mut helper = Helper::new(work, &fs_ops);
    let mut problem = sample_problem();
    problem.test_cases[0].input = (0..100)
        .map(|i| format!("line-{i:03}"))
        .collect::<Vec<_>>()
        .join("\n");
    helper.new_problem(problem);

    helper.handle_event(key(KeyCode::Char('e')));
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal.draw(|f| helper.draw(f)).unwrap();
    dump_screen(&terminal);

    let buffer = terminal.backend().buffer();
    let screen = (0..buffer.area.height)
        .map(|y| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        screen.contains("line-099"),
        "the cursor line must stay on screen:\n{screen}"
    );

    // Adding more lines must not shift the list under the cursor.
    helper.handle_event(key(KeyCode::Enter));
    type_text(&mut helper, "fresh-line");
    terminal.draw(|f| helper.draw(f)).unwrap();
    let buffer = terminal.backend().buffer();
    let screen = (0..buffer.area.height)
        .map(|y| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        screen.contains("fresh-line"),
        "new text must stay on screen:\n{screen}"
    );

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn switching_to_expected_does_not_shrink_input() {
    let root = temp_root("edit_stable_panes");
    let fs_ops = FsOps::with_root(root.clone());
    let work = root.join("problems");
    std::fs::create_dir_all(&work).unwrap();

    let mut helper = Helper::new(work, &fs_ops);
    let mut problem = sample_problem();
    problem.test_cases[0].input = (0..40)
        .map(|i| format!("line-{i:03}"))
        .collect::<Vec<_>>()
        .join("\n");
    helper.new_problem(problem);
    helper.handle_event(key(KeyCode::Char('e')));

    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal.draw(|f| helper.draw(f)).unwrap();
    let input_height = helper.tc_list.testcases[0].input_box.rect.height;

    helper.handle_event(key(KeyCode::Tab));
    terminal.draw(|f| helper.draw(f)).unwrap();
    dump_screen(&terminal);
    assert_eq!(
        helper.tc_list.testcases[0].input_box.rect.height, input_height,
        "focusing Expected must not shrink the Input pane"
    );

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn mouse_wheel_scrolls_the_editing_textarea() {
    let root = temp_root("edit_wheel");
    let fs_ops = FsOps::with_root(root.clone());
    let work = root.join("problems");
    std::fs::create_dir_all(&work).unwrap();

    let mut helper = Helper::new(work, &fs_ops);
    let mut problem = sample_problem();
    problem.test_cases[0].input = (0..40)
        .map(|i| format!("line-{i:03}"))
        .collect::<Vec<_>>()
        .join("\n");
    helper.new_problem(problem);
    helper.handle_event(key(KeyCode::Char('e')));

    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal.draw(|f| helper.draw(f)).unwrap();

    let rect = helper.tc_list.testcases[0].input_box.rect;
    let wheel = |kind| {
        Event::Mouse(MouseEvent {
            kind,
            column: rect.x + 2,
            row: rect.y + 2,
            modifiers: KeyModifiers::NONE,
        })
    };
    let before = helper.tc_list.testcases[0].inp_content.cursor().0;
    let list_before = helper.tc_list.scroll_row;
    helper.handle_event(wheel(MouseEventKind::ScrollUp));
    assert_eq!(
        helper.tc_list.testcases[0].inp_content.cursor().0,
        before - 1
    );
    assert_eq!(
        helper.tc_list.scroll_row, list_before,
        "the list must not move"
    );
    helper.handle_event(wheel(MouseEventKind::ScrollDown));
    assert!(helper.tc_list.testcases[0].inp_content.cursor().0 <= before);
    assert_eq!(
        helper.tc_list.scroll_row, list_before,
        "the list must not move"
    );

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn manual_list_scroll_survives_editing_redraws() {
    let root = temp_root("edit_list_wheel");
    let fs_ops = FsOps::with_root(root.clone());
    let work = root.join("problems");
    std::fs::create_dir_all(&work).unwrap();

    let mut helper = Helper::new(work, &fs_ops);
    let mut problem = sample_problem();
    let body = (0..30)
        .map(|i| format!("line-{i:03}"))
        .collect::<Vec<_>>()
        .join("\n");
    problem.test_cases = (0..4)
        .map(|_| ReceivingTestCase {
            input: body.clone(),
            expected_output: "ok".into(),
        })
        .collect();
    helper.new_problem(problem);
    helper.handle_event(key(KeyCode::Char('e')));

    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal.draw(|f| helper.draw(f)).unwrap();

    let wheel = Event::Mouse(MouseEvent {
        kind: MouseEventKind::ScrollDown,
        column: 1,
        row: 1,
        modifiers: KeyModifiers::NONE,
    });
    helper.handle_event(wheel);
    let scrolled = helper.tc_list.scroll_row;
    assert!(scrolled > 0);
    terminal.draw(|f| helper.draw(f)).unwrap();
    assert_eq!(
        helper.tc_list.scroll_row, scrolled,
        "redraw must not snap back"
    );

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn mouse_wheel_scrolls_the_delete_list() {
    let root = temp_root("delete_list_wheel");
    let fs_ops = FsOps::with_root(root.clone());
    let work = root.join("problems");
    std::fs::create_dir_all(&work).unwrap();

    let mut helper = Helper::new(work, &fs_ops);
    let mut problem = sample_problem();
    let body = (0..30)
        .map(|i| format!("line-{i:03}"))
        .collect::<Vec<_>>()
        .join("\n");
    problem.test_cases = (0..4)
        .map(|_| ReceivingTestCase {
            input: body.clone(),
            expected_output: "ok".into(),
        })
        .collect();
    helper.new_problem(problem);
    helper.handle_event(key(KeyCode::Char('x')));
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal.draw(|f| helper.draw(f)).unwrap();

    let wheel = Event::Mouse(MouseEvent {
        kind: MouseEventKind::ScrollDown,
        column: 1,
        row: 1,
        modifiers: KeyModifiers::NONE,
    });
    helper.handle_event(wheel);
    let scrolled = helper.tc_list.scroll_row;
    assert!(scrolled > 0);
    terminal.draw(|f| helper.draw(f)).unwrap();
    assert_eq!(
        helper.tc_list.scroll_row, scrolled,
        "delete mode keeps manual scroll"
    );

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn clicking_a_pane_in_edit_mode_moves_the_focus() {
    let root = temp_root("click_focus");
    let fs_ops = FsOps::with_root(root.clone());
    let work = root.join("problems");
    std::fs::create_dir_all(&work).unwrap();

    let mut helper = Helper::new(work, &fs_ops);
    helper.new_problem(sample_problem());
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();

    helper.handle_event(key(KeyCode::Char('e')));
    terminal.draw(|f| helper.draw(f)).unwrap();

    let click = |rect: ratatui::layout::Rect| {
        Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(ratatui::crossterm::event::MouseButton::Left),
            column: rect.x + 1,
            row: rect.y,
            modifiers: KeyModifiers::NONE,
        })
    };

    let expected_rect = helper.tc_list.testcases[0].expected_box.rect;
    helper.handle_event(click(expected_rect));
    assert!(matches!(
        helper.tc_list.active_box,
        crate::components::testcase::ActiveBox::Expected
    ));
    assert_eq!(helper.tc_list.active_area, 0);

    let input_rect = helper.tc_list.testcases[0].input_box.rect;
    helper.handle_event(click(input_rect));
    assert!(matches!(
        helper.tc_list.active_box,
        crate::components::testcase::ActiveBox::Input
    ));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn double_click_opens_a_cut_pane_only() {
    let root = temp_root("pane_dialog");
    let fs_ops = FsOps::with_root(root.clone());
    let work = root.join("problems");
    std::fs::create_dir_all(&work).unwrap();

    let mut helper = Helper::new(work, &fs_ops);
    helper.new_problem(sample_problem());
    let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
    terminal.draw(|f| helper.draw(f)).unwrap();

    // Everything in this box is already on screen: nothing to open.
    let rect = helper.tc_list.testcases[0].input_box.rect;
    let fits = Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: rect.x + 2,
        row: rect.y,
        modifiers: KeyModifiers::NONE,
    });
    helper.handle_event(fits.clone());
    helper.handle_event(fits);
    assert!(helper.pane_dialog.is_none(), "text that fits opens nothing");

    let wide = format!("1 2 {}", "9 ".repeat(80));
    helper.tc_list.testcases[0] = TestCase::new(&wide, "3");
    terminal.draw(|f| helper.draw(f)).unwrap();

    let rect = helper.tc_list.testcases[0].input_box.rect;
    let click = || {
        Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: rect.x + 2,
            row: rect.y,
            modifiers: KeyModifiers::NONE,
        })
    };

    helper.handle_event(click());
    assert!(
        helper.pane_dialog.is_none(),
        "one click must not open the dialog"
    );
    helper.handle_event(click());
    let dialog = helper
        .pane_dialog
        .as_ref()
        .expect("double click opens the dialog");
    assert!(dialog.title.starts_with("Case 1 · input"));
    assert!(
        dialog
            .visible_lines()
            .iter()
            .any(|line| line.to_string() == wide)
    );

    terminal.draw(|f| helper.draw(f)).unwrap();
    dump_screen(&terminal);

    helper.handle_event(key(KeyCode::Esc));
    assert!(helper.pane_dialog.is_none());

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn pane_dialog_pans_horizontally() {
    let root = temp_root("pane_dialog_hscroll");
    let fs_ops = FsOps::with_root(root.clone());
    let work = root.join("problems");
    std::fs::create_dir_all(&work).unwrap();

    let mut helper = Helper::new(work, &fs_ops);
    let mut problem = sample_problem();
    problem.test_cases[0].input = format!("START{}END", "x".repeat(300));
    helper.new_problem(problem);
    helper.open_pane_dialog(0, PaneKind::Input);

    let mut terminal = Terminal::new(TestBackend::new(60, 24)).unwrap();
    terminal.draw(|f| helper.draw(f)).unwrap();
    dump_screen(&terminal);
    let screen = |terminal: &Terminal<TestBackend>| {
        let buffer = terminal.backend().buffer();
        (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    };

    let dialog = helper.pane_dialog.as_ref().expect("dialog opened");
    assert_eq!(dialog.h_scroll, 0);
    assert!(dialog.viewport_width < 300);
    assert!(screen(&terminal).contains("START"));

    helper.handle_event(key(KeyCode::Right));
    terminal.draw(|f| helper.draw(f)).unwrap();
    assert_eq!(helper.pane_dialog.as_ref().unwrap().h_scroll, 1);
    assert!(!screen(&terminal).contains("START"));

    helper.handle_event(key(KeyCode::Left));
    terminal.draw(|f| helper.draw(f)).unwrap();
    assert_eq!(helper.pane_dialog.as_ref().unwrap().h_scroll, 0);
    assert!(screen(&terminal).contains("START"));

    let area = helper.pane_dialog.as_ref().unwrap().h_scroll_area;
    assert!(area.width > 1, "the horizontal scrollbar must be hittable");
    let buffer = terminal.backend().buffer();
    let gap_row = (0..area.width)
        .map(|x| buffer[(area.x + x, area.y - 1)].symbol())
        .collect::<String>();
    assert!(
        gap_row.chars().all(|c| c == ' '),
        "the horizontal scrollbar keeps a blank row above it: {gap_row:?}"
    );
    let mouse = |kind, column, row| {
        Event::Mouse(MouseEvent {
            kind,
            column,
            row,
            modifiers: KeyModifiers::NONE,
        })
    };

    helper.handle_event(mouse(
        MouseEventKind::Down(MouseButton::Left),
        area.x + area.width - 1,
        area.y,
    ));
    let dialog = helper.pane_dialog.as_ref().unwrap();
    let max = dialog
        .content
        .max_width()
        .saturating_sub(dialog.viewport_width as usize);
    assert!(max > 0);
    assert_eq!(dialog.h_scroll, max, "clicking the track seeks to the end");

    helper.handle_event(mouse(
        MouseEventKind::Drag(MouseButton::Left),
        area.x,
        area.y,
    ));
    assert_eq!(
        helper.pane_dialog.as_ref().unwrap().h_scroll,
        0,
        "dragging the scrollbar seeks back"
    );

    helper.handle_event(mouse(MouseEventKind::Up(MouseButton::Left), area.x, area.y));
    assert!(helper.pane_dialog.as_ref().unwrap().h_scroll_drag.is_none());

    helper.handle_event(mouse(MouseEventKind::ScrollRight, area.x + 1, area.y));
    assert_eq!(helper.pane_dialog.as_ref().unwrap().h_scroll, 1);
    helper.handle_event(mouse(MouseEventKind::ScrollLeft, area.x + 1, area.y));
    assert_eq!(helper.pane_dialog.as_ref().unwrap().h_scroll, 0);

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn pane_dialog_vertical_scrollbar_drags() {
    let root = temp_root("pane_dialog_vscroll");
    let fs_ops = FsOps::with_root(root.clone());
    let work = root.join("problems");
    std::fs::create_dir_all(&work).unwrap();

    let mut helper = Helper::new(work, &fs_ops);
    let input = (0..100)
        .map(|index| format!("line-{index:03}"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut problem = sample_problem();
    problem.test_cases = vec![ReceivingTestCase {
        input,
        expected_output: String::new(),
    }];
    helper.new_problem(problem);
    helper.open_pane_dialog(0, PaneKind::Input);

    let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
    terminal.draw(|f| helper.draw(f)).unwrap();
    dump_screen(&terminal);
    let area = helper.pane_dialog.as_ref().unwrap().v_scroll_area;
    assert!(
        area.height > 2,
        "the pane dialog must expose a vertical scrollbar"
    );
    let buffer = terminal.backend().buffer();
    for row in area.y..area.y + area.height {
        assert_eq!(
            buffer[(area.x - 1, row)].symbol(),
            " ",
            "the vertical scrollbar keeps a blank column beside it"
        );
    }

    let mouse = |kind, column, row| {
        Event::Mouse(MouseEvent {
            kind,
            column,
            row,
            modifiers: KeyModifiers::NONE,
        })
    };
    let bottom = area.y + area.height - 2;
    helper.handle_event(mouse(
        MouseEventKind::Down(MouseButton::Left),
        area.x,
        bottom,
    ));
    assert!(
        helper.pane_dialog.as_ref().unwrap().scroll > 0,
        "clicking the track must seek down"
    );

    helper.handle_event(mouse(
        MouseEventKind::Drag(MouseButton::Left),
        area.x,
        area.y + 1,
    ));
    assert_eq!(
        helper.pane_dialog.as_ref().unwrap().scroll,
        0,
        "dragging to the top must seek back"
    );
    helper.handle_event(mouse(
        MouseEventKind::Up(MouseButton::Left),
        area.x,
        area.y + 1,
    ));
    assert!(helper.pane_dialog.as_ref().unwrap().v_scroll_drag.is_none());

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn pane_dialog_only_closes_on_esc() {
    let root = temp_root("pane_dialog_close");
    let fs_ops = FsOps::with_root(root.clone());
    let work = root.join("problems");
    std::fs::create_dir_all(&work).unwrap();

    let mut helper = Helper::new(work, &fs_ops);
    helper.new_problem(sample_problem());
    helper.open_pane_dialog(0, PaneKind::Input);

    let click = |column, row| {
        Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column,
            row,
            modifiers: KeyModifiers::NONE,
        })
    };

    helper.handle_event(click(2, 2));
    assert!(
        helper.pane_dialog.is_some(),
        "a click inside must not close the dialog"
    );
    helper.handle_event(click(90, 28));
    assert!(
        helper.pane_dialog.is_some(),
        "a click outside must not close the dialog"
    );
    helper.handle_event(key(KeyCode::Enter));
    assert!(
        helper.pane_dialog.is_some(),
        "Enter must not close the dialog"
    );
    helper.handle_event(key(KeyCode::Char('q')));
    assert!(helper.pane_dialog.is_some(), "q must not close the dialog");

    helper.handle_event(key(KeyCode::Esc));
    assert!(helper.pane_dialog.is_none());

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn paged_got_dialog_keeps_the_diff_and_reads_pages() {
    let root = temp_root("got_dialog_paged");
    let fs_ops = FsOps::with_root(root.clone());
    let work = root.join("problems");
    std::fs::create_dir_all(&work).unwrap();

    let mut helper = Helper::new(work, &fs_ops);
    helper.new_problem(sample_problem());
    let output = (0..20_001)
        .map(|index| format!("line-{index:05}"))
        .collect::<Vec<_>>()
        .join("\n");
    helper.tc_list.testcases[0].set_result(CaseVerdict::WA { ms: 1 }, output);
    helper.open_pane_dialog(0, PaneKind::Got);

    let dialog = helper.pane_dialog.as_ref().expect("dialog opened");
    assert!(matches!(dialog.content, DialogContent::Paged { .. }));
    assert_eq!(
        dialog.total_lines(),
        20_002,
        "one warning row plus the output"
    );
    assert_eq!(dialog.visible_lines().len(), GOT_PAGE_LINES);
    assert!(
        dialog.visible_lines()[0]
            .spans
            .iter()
            .any(|span| span.style.fg == Some(theme::error())),
        "paged WA output still gets its diff"
    );

    let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
    terminal.draw(|f| helper.draw(f)).unwrap();
    let buffer = terminal.backend().buffer();
    let first_screen = (0..buffer.area.height)
        .map(|y| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        first_screen.contains("paged view"),
        "the first page must explain that it is paged:\n{first_screen}"
    );
    dump_screen(&terminal);

    helper.set_pane_dialog_scroll(1_000);
    terminal.draw(|f| helper.draw(f)).unwrap();
    let dialog = helper.pane_dialog.as_ref().expect("dialog still open");
    assert_eq!(
        dialog.visible_lines()[487].to_string(),
        "line-00999",
        "the second page must start at line 512 and line up with the scroll"
    );
    dump_screen(&terminal);

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn safe_wa_output_keeps_its_diff_in_the_dialog() {
    let root = temp_root("got_dialog_diff");
    let fs_ops = FsOps::with_root(root.clone());
    let work = root.join("problems");
    std::fs::create_dir_all(&work).unwrap();

    let mut helper = Helper::new(work, &fs_ops);
    helper.new_problem(sample_problem());
    // Wider than the box, so it is cut and opens.
    helper.tc_list.testcases[0].set_result(CaseVerdict::WA { ms: 1 }, "4 ".repeat(60));
    let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
    terminal.draw(|f| helper.draw(f)).unwrap();

    let rect = helper.tc_list.testcases[0].got_box.rect;
    let click = || {
        Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: rect.x + 2,
            row: rect.y,
            modifiers: KeyModifiers::NONE,
        })
    };
    helper.handle_event(click());
    helper.handle_event(click());

    let dialog = helper
        .pane_dialog
        .as_ref()
        .expect("double click opens the Got dialog");
    assert!(dialog.title.starts_with("Case 1 · got"));
    assert!(
        dialog
            .visible_lines()
            .iter()
            .flat_map(|line| &line.spans)
            .any(|span| span.style.fg == Some(theme::error())),
        "safe WA output keeps its red diff in the dialog"
    );
    assert!(
        dialog
            .visible_lines()
            .iter()
            .all(|line| !line.to_string().contains("too large"))
    );

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn double_click_does_not_open_a_dialog_in_delete_mode() {
    let root = temp_root("pane_dialog_delete");
    let fs_ops = FsOps::with_root(root.clone());
    let work = root.join("problems");
    std::fs::create_dir_all(&work).unwrap();

    let mut helper = Helper::new(work, &fs_ops);
    helper.new_problem(sample_problem());
    helper.handle_event(key(KeyCode::Char('x')));
    let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
    terminal.draw(|f| helper.draw(f)).unwrap();

    let rect = helper.tc_list.testcases[0].input_box.rect;
    let click = || {
        Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: rect.x + 2,
            row: rect.y,
            modifiers: KeyModifiers::NONE,
        })
    };
    helper.handle_event(click());
    helper.handle_event(click());
    assert!(
        helper.pane_dialog.is_none(),
        "delete mode must keep its own modal"
    );

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn directory_console_changes_dir() {
    let root = temp_root("dir");
    let fs_ops = FsOps::with_root(root.clone());
    let work = root.join("problems");
    std::fs::create_dir_all(work.join("cses")).unwrap();

    let mut helper = Helper::new(work.clone(), &fs_ops);
    helper.handle_event(key(KeyCode::Char('d')));
    assert!(matches!(helper.input_mode, InputMode::Directory));

    type_text(&mut helper, "cd cses");
    helper.handle_event(key(KeyCode::Enter));

    assert_eq!(
        helper.dir_terminal.cur_dir,
        work.join("cses").canonicalize().unwrap()
    );

    helper.handle_event(key(KeyCode::Esc));
    assert!(matches!(helper.input_mode, InputMode::Normal));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn external_command_loads_file_by_stem() {
    let root = temp_root("external");
    let fs_ops = FsOps::with_root(root.clone());
    let work = root.join("problems");
    std::fs::create_dir_all(&work).unwrap();

    let mut helper = Helper::new(work.clone(), &fs_ops);
    let code_path = helper.new_problem(sample_problem()).unwrap();

    let mut fresh = Helper::new(work, &fs_ops);
    let (_, reply) = fresh.external_command(Some(code_path.clone()), RunAction::Add);
    assert!(reply.is_ok());
    assert!(matches!(fresh.input_mode, InputMode::Editing));
    assert_eq!(fresh.tc_list.testcases.len(), 3);
    assert!(fresh.active_problem.is_some());

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn run_without_stored_cases_starts_an_interactive_session() {
    let root = temp_root("interactive_start");
    let fs_ops = FsOps::with_root(root.clone());
    let work = root.join("problems");
    std::fs::create_dir_all(&work).unwrap();
    assert!(fs_ops.save_config(&UserConfig {
        language: Language::Python,
        extra_flags: vec![],
        ..Default::default()
    }));

    let code_path = work.join("Solo.py");
    std::fs::write(&code_path, "print('hi')\n").unwrap();

    let mut helper = Helper::new(work, &fs_ops);
    let (command, reply) = helper.external_command(Some(code_path), RunAction::Run);
    assert!(reply.is_ok());
    assert!(matches!(
        command,
        Some(PassingCommand::ToRunner(RunnerCommand::Interact { .. }))
    ));
    assert!(
        helper.tc_list.testcases.is_empty(),
        "a session must not invent a smoke case"
    );
    assert!(!helper.running);
    assert!(matches!(helper.input_mode, InputMode::Session));
    let first = helper
        .session
        .as_ref()
        .expect("the session should exist immediately");
    assert!(first.alive);
    assert!(!first.stopping);
    let first_id = first.id;

    // Running again retires the first session and starts a fresh one.
    let (command, reply) = helper.external_command(None, RunAction::Run);
    assert!(reply.is_ok());
    assert!(matches!(
        command,
        Some(PassingCommand::ToRunner(RunnerCommand::Interact { .. }))
    ));
    let second = helper.session.as_ref().expect("a session should exist");
    assert_eq!(second.id, first_id + 1);
    assert!(second.alive);

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn session_output_keeps_prompts_before_the_echo() {
    let (input_tx, _input_rx) = mpsc::channel();
    let mut session = InteractiveSession::new(
        1,
        Arc::new(AtomicBool::new(false)),
        Arc::new(AtomicI32::new(0)),
        input_tx,
    );
    session.push_stream(TranscriptKind::Out, "n = ");
    session.push_you("5".to_string());
    session.push_stream(TranscriptKind::Out, "5\n");
    session.push_stream(TranscriptKind::Status, "exited\n");

    let rows: Vec<(TranscriptKind, &str)> = session
        .lines
        .iter()
        .map(|line| (line.kind, line.text.as_str()))
        .collect();
    assert_eq!(
        rows,
        vec![
            (TranscriptKind::Out, "n = "),
            (TranscriptKind::You, "5"),
            (TranscriptKind::Out, "5"),
            (TranscriptKind::Status, "exited"),
        ]
    );
    assert!(session.partial.is_empty());
}

#[test]
fn session_status_never_merges_into_stdout() {
    let (input_tx, _input_rx) = mpsc::channel();
    let mut session = InteractiveSession::new(
        1,
        Arc::new(AtomicBool::new(false)),
        Arc::new(AtomicI32::new(0)),
        input_tx,
    );
    session.push_stream(TranscriptKind::Out, "prompt: ");
    session.push_stream(TranscriptKind::Status, "running");
    assert_eq!(session.lines.len(), 2);
    assert_eq!(session.lines[0].kind, TranscriptKind::Out);
    assert_eq!(session.lines[0].text, "prompt: ");
    assert_eq!(session.lines[1].kind, TranscriptKind::Status);
    assert_eq!(session.lines[1].text, "running");
    assert!(session.partial.is_empty());
}

#[test]
fn completing_a_partial_does_not_move_a_scrolled_transcript() {
    let (input_tx, _input_rx) = mpsc::channel();
    let mut session = InteractiveSession::new(
        1,
        Arc::new(AtomicBool::new(false)),
        Arc::new(AtomicI32::new(0)),
        input_tx,
    );
    for index in 0..50 {
        session.push_stream(TranscriptKind::Out, &format!("line {index}\n"));
    }
    session.push_stream(TranscriptKind::Out, "prompt: ");
    session.scroll_back(5);
    session.push_stream(TranscriptKind::Out, "answer\n");

    assert_eq!(session.lines.len(), 51);
    assert_eq!(
        session.scroll, 5,
        "finishing the visible partial must not move the view"
    );

    // A genuinely new line is appended below the view, so the distance
    // from the tail grows and the same content stays on screen.
    session.push_stream(TranscriptKind::Out, "next\n");
    assert_eq!(session.scroll, 6);
}

#[test]
fn interactive_session_renders_the_transcript_and_input() {
    let root = temp_root("interactive_render");
    let fs_ops = FsOps::with_root(root.clone());
    let work = root.join("problems");
    std::fs::create_dir_all(&work).unwrap();
    assert!(fs_ops.save_config(&UserConfig {
        language: Language::Python,
        extra_flags: vec![],
        ..Default::default()
    }));
    let code_path = work.join("Solo.py");
    std::fs::write(&code_path, "print(input())\n").unwrap();

    let mut helper = Helper::new(work, &fs_ops);
    let (command, _) = helper.external_command(Some(code_path), RunAction::Run);
    assert!(matches!(
        command,
        Some(PassingCommand::ToRunner(RunnerCommand::Interact { .. }))
    ));
    let session_id = helper.session.as_ref().expect("a session").id;
    helper.session_chunk(session_id, SessionStream::Out, "n = ".to_string());
    type_text(&mut helper, "5");
    helper.handle_event(key(KeyCode::Enter));

    let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
    terminal.draw(|f| helper.draw(f)).unwrap();
    let buffer = terminal.backend().buffer();
    let row = |y| {
        (0..buffer.area.width)
            .map(|x| buffer[(x, y)].symbol())
            .collect::<String>()
    };
    let screen = (0..buffer.area.height)
        .map(row)
        .collect::<Vec<_>>()
        .join("\n");

    assert!(screen.contains("interactive"), "{screen}");
    assert!(screen.contains("n = "), "{screen}");
    assert!(screen.contains("❯ 5"), "{screen}");
    assert!(screen.contains("SESSION"), "{screen}");

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn ctrl_d_and_ctrl_c_are_sent_to_the_session() {
    let root = temp_root("interactive_keys");
    let fs_ops = FsOps::with_root(root.clone());
    let work = root.join("problems");
    std::fs::create_dir_all(&work).unwrap();
    assert!(fs_ops.save_config(&UserConfig {
        language: Language::Python,
        extra_flags: vec![],
        ..Default::default()
    }));
    let code_path = work.join("Solo.py");
    std::fs::write(&code_path, "input()\n").unwrap();

    let mut helper = Helper::new(work, &fs_ops);
    let (command, _) = helper.external_command(Some(code_path), RunAction::Run);
    let Some(PassingCommand::ToRunner(RunnerCommand::Interact { input, .. })) = command else {
        panic!("expected an interactive command");
    };

    helper.handle_event(Event::Key(KeyEvent::new(
        KeyCode::Char('d'),
        KeyModifiers::CONTROL,
    )));
    assert_eq!(
        input.recv_timeout(Duration::from_millis(100)).unwrap(),
        SessionInput::Eof
    );
    helper.handle_event(Event::Key(KeyEvent::new(
        KeyCode::Char('c'),
        KeyModifiers::CONTROL,
    )));
    assert_eq!(
        input.recv_timeout(Duration::from_millis(100)).unwrap(),
        SessionInput::Interrupt
    );

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn external_command_reports_missing_files() {
    let root = temp_root("external_missing");
    let fs_ops = FsOps::with_root(root.clone());
    let work = root.join("problems");
    std::fs::create_dir_all(&work).unwrap();

    let mut helper = Helper::new(work, &fs_ops);
    let (command, reply) = helper.external_command(Some(root.join("nope.cpp")), RunAction::Run);
    assert!(command.is_none());
    assert!(reply.unwrap_err().contains("file not found"));

    let _ = std::fs::remove_dir_all(&root);
}

/// Every main state renders cleanly at the minimum size and wide: the
/// header and console are in place and the help key is always on screen.
/// `ZCH_DUMP=1 cargo test screens_render -- --nocapture` prints them.
#[test]
fn screens_render_at_the_floor_and_wide() {
    let root = temp_root("screens");
    let fs_ops = FsOps::with_root(root.clone());
    let work = root.join("problems");
    std::fs::create_dir_all(&work).unwrap();
    std::fs::create_dir_all(work.join("round912")).unwrap();
    std::fs::write(work.join("A.cpp"), "").unwrap();

    let sizes = [(crate::components::splash::MIN_WIDTH, 24u16), (100, 30)];
    let check = |helper: &mut Helper, label: &str| {
        for (width, height) in sizes {
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal.draw(|f| helper.draw(f)).unwrap();
            let screen = screen_text(&terminal);
            if std::env::var("ZCH_DUMP").is_ok() {
                println!("── {label} {width}x{height}\n{screen}");
            }
            if label != "help" {
                assert!(
                    row_text(&terminal, 1).contains("zch"),
                    "{label}: header\n{screen}"
                );
            }
            let console = row_text(&terminal, height - 1);
            assert!(
                console.contains("[Ctrl-H] help") || console.contains("[Esc] close"),
                "{label} {width}x{height}: help (or the way out of it) in the console: {console}"
            );
        }
    };

    let mut helper = Helper::new(work.clone(), &fs_ops);
    check(&mut helper, "splash");

    helper.new_problem(sample_problem());
    stream(
        &mut helper,
        vec![
            success("3\n", 4, true),
            Verdict::RuntimeError {
                error: "killed by SIGSEGV — invalid memory access".into(),
                stdout: "1".into(),
            },
        ],
    );
    for _ in 0..2 {
        helper.handle_event(key(KeyCode::Char('s')));
        helper.handle_event(key(KeyCode::Enter));
    }
    helper.submit_finished(1, "RESULT: Wrong answer on test 3".into());
    check(&mut helper, "cases");

    helper.handle_event(key(KeyCode::Char('d')));
    type_text(&mut helper, "ls");
    helper.handle_event(key(KeyCode::Enter));
    check(&mut helper, "directory");
    helper.handle_event(key(KeyCode::Esc));

    helper.handle_event(Event::Key(KeyEvent::new(
        KeyCode::Char('h'),
        KeyModifiers::ALT,
    )));
    check(&mut helper, "help");

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn option_h_opens_help_for_the_mode_you_are_in() {
    let root = temp_root("help_keys");
    let fs_ops = FsOps::with_root(root.clone());
    let work = root.join("problems");
    std::fs::create_dir_all(&work).unwrap();

    let mut helper = Helper::new(work, &fs_ops);
    helper.new_problem(sample_problem());
    let alt_h = Event::Key(KeyEvent::new(KeyCode::Char('h'), KeyModifiers::ALT));

    // A plain `h` is not help.
    helper.handle_event(key(KeyCode::Char('h')));
    assert!(!helper.show_help);

    // While editing, `h` is a letter and Ctrl-H is still help.
    helper.handle_event(key(KeyCode::Char('e')));
    helper.handle_event(key(KeyCode::Char('h')));
    assert!(!helper.show_help, "h types an h while editing");
    helper.handle_event(alt_h.clone());
    assert!(helper.show_help);

    let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
    terminal.draw(|f| helper.draw(f)).unwrap();
    let screen = screen_text(&terminal);
    assert!(screen.contains("Keys · edit"), "{screen}");
    assert!(
        screen.contains("[Esc]") && screen.contains("save"),
        "{screen}"
    );
    assert!(
        !screen.contains("submit"),
        "only this mode's keys:\n{screen}"
    );

    helper.handle_event(key(KeyCode::Esc));
    assert!(!helper.show_help);
    assert_eq!(
        helper.input_mode,
        InputMode::Editing,
        "help closes back to editing"
    );

    // Ctrl-H and the `˙` that Option-H sends without Meta work too.
    helper.handle_event(Event::Key(KeyEvent::new(
        KeyCode::Char('h'),
        KeyModifiers::CONTROL,
    )));
    assert!(helper.show_help);
    helper.handle_event(key(KeyCode::Esc));
    helper.handle_event(key(KeyCode::Char('˙')));
    assert!(helper.show_help);

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_cut_error_opens_whole_on_a_double_click() {
    let root = temp_root("reveal_error");
    let fs_ops = FsOps::with_root(root.clone());
    let work = root.join("problems");
    std::fs::create_dir_all(&work).unwrap();

    let mut helper = Helper::new(work, &fs_ops);
    let long = format!("could not save: {}", "the disk said no ".repeat(10));
    helper.config_menu.error = Some(long.clone());
    helper.input_mode = InputMode::Config;
    let mut terminal = Terminal::new(TestBackend::new(80, 30)).unwrap();
    terminal.draw(|f| helper.draw(f)).unwrap();

    let screen = terminal.backend().buffer().clone();
    let (x, y) = (0..screen.area.height)
        .flat_map(|y| (0..screen.area.width).map(move |x| (x, y)))
        .find(|&(x, y)| screen[(x, y)].symbol() == "✗")
        .expect("the error is drawn");
    let click = Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: x + 4,
        row: y,
        modifiers: KeyModifiers::NONE,
    });
    helper.handle_event(click.clone());
    assert!(helper.pane_dialog.is_none(), "one click opens nothing");
    helper.handle_event(click);
    let dialog = helper.pane_dialog.as_ref().expect("double click opens it");
    assert_eq!(dialog.title, "Error");
    assert_eq!(dialog.visible_lines()[0].to_string(), long);

    // The viewer closes back to the config sheet.
    helper.handle_event(key(KeyCode::Esc));
    assert!(helper.pane_dialog.is_none());
    assert_eq!(helper.input_mode, InputMode::Config);

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_long_output_draws_only_the_rows_on_screen() {
    let root = temp_root("long_dialog");
    let fs_ops = FsOps::with_root(root.clone());
    let work = root.join("problems");
    std::fs::create_dir_all(&work).unwrap();

    let mut helper = Helper::new(work, &fs_ops);
    let text: Vec<String> = (0..20_000).map(|n| format!("line {n}")).collect();
    helper.open_text_dialog("Output", &text.join("\n"));
    let mut terminal = Terminal::new(TestBackend::new(80, 30)).unwrap();
    terminal.draw(|f| helper.draw(f)).unwrap();

    let dialog = helper.pane_dialog.as_mut().unwrap();
    let rows = dialog.viewport as usize;
    let shown = dialog.visible(12_345, rows);
    assert_eq!(shown.len(), rows, "exactly one screenful");
    assert_eq!(shown[0].to_string(), "line 12345");

    // Scrolled to the end, the last line is on screen.
    helper.set_pane_dialog_scroll(usize::MAX);
    terminal.draw(|f| helper.draw(f)).unwrap();
    let screen = screen_text(&terminal);
    assert!(screen.contains("line 19999"), "{screen}");
    assert!(!screen.contains("line 0\n"));

    let _ = std::fs::remove_dir_all(&root);
}
