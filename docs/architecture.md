# Architecture

Back to the [README](../README.md).

```
src/
  main.rs              — event loop: one channel, four actors
  error.rs             — shared error type
  helper/              — all UI state, owned by the main thread
    run.rs             — running cases, verdicts, the run summary
    cases.rs           — adding, editing, deleting and saving cases
    submit.rs          — the submit sheet and each submission's progress
    problem.rs         — the loaded problem, `zch send` replies
    session.rs         — interactive programs for problems without samples
    dialog.rs          — the viewer: double-click to read anything cut short
    selection.rs       — mouse selection and the clipboard
    input.rs           — keyboard and mouse dispatch
    view/              — everything drawn (chrome, sheets, transcript, motion)
  components/          — ratatui widgets
    theme.rs           — the four palettes, as semantic colour roles
    ui.rs              — shared primitives: rows, keycaps, sheets, cut-text registry
    tc_list.rs         — the scrolling case list
    testcase.rs        — one case: header, input / expected / got boxes, diff
    config.rs          — language, flag tags, theme
    dir_terminal.rs    — the cd / ls / pwd console
    status.rs          — the console line and the message vocabulary
    motion.rs          — eased, time-based transitions
    splash.rs          — the empty-state screen
  core/
    receiver.rs        — Competitive Companion listener (:10043) and the command API
    runner.rs          — compile cache, runs with timing and memory
    checker.rs         — token / line / float / custom output checking
    submitter.rs       — Safari AppleScript automation, 403 / CAPTCHA handling
    fs_ops.rs          — boilerplate, test database, config, binary pruning
```

Everything runs on one channel. The UI thread owns all state; the receiver, runner and submitter workers only exchange `PassingCommand` messages with it, and no worker holds a handle to another.

Rendering is immediate-mode: each frame is drawn from the current state, every colour is a role on the active theme, and animation is a pure function of how long ago something started, so a stalled loop never leaves a half-played effect behind.
