# Graph Report - zch  (2026-11-10)

## Corpus Check
- 38 files · ~61,118 words
- Verdict: corpus is large enough that graph structure adds value.
- Unclassified: 2 file(s) not represented in the graph (top: (none) 1, .toml 1)

## Summary
- 1369 nodes · 3295 edges · 76 communities (67 shown, 9 thin omitted)
- Extraction: 95% EXTRACTED · 5% INFERRED · 0% AMBIGUOUS · INFERRED: 158 edges (avg confidence: 0.73)
- Token cost: 96,630 input · 0 output

## Community Hubs (Navigation)
- Config Sheet and Flags
- Helper Integration Tests
- Status Line and Notices
- Interactive Session UI
- Theme Palettes
- Got Output Storage
- Directory Console
- Safari Submitter
- Chrome and Cut-Text Reveals
- Pane Viewer Dialog
- Splash Screen
- Case Card Rendering
- Companion Listener
- Output Checkers
- Shared UI Primitives
- Compile Cache and PTY
- Path Breadcrumb Drawing
- README Features
- Case Pane Hit Areas
- Case List Layout
- Program Execution
- Output Capping
- Icons and Scrollbars
- Component Imports
- Motion Timing
- Time Limits and Cache Keys
- Mouse Text Selection
- View Motion State
- Helper Imports
- Filesystem Operations
- Terminal Lifecycle
- Core Config Types
- Runner Collection Tests
- Atomic File Writes
- Release Workflow Jobs
- Case Box Drawing
- Helper Commands
- Run Dispatch
- Case Editing Commands
- Release Workflow Steps
- Case Verdicts
- Submit Planning
- Test Case List
- WA Diff Tests
- Error Types
- PTY Session
- Input and Problem Glue
- Input Modes
- Release Build Matrix
- Release Artifact Upload
- Scrollbar Drag
- Process Group Control
- Homebrew Publish Job
- Screenshot: Case Card
- Escape Sequence Filter
- Problem Attachment
- Release Host Job
- Release Host Outputs
- Release Plan Job
- Paged Got File
- Runner Verdicts
- Console Key Fitting
- Run Result Handling
- Runner Channel Commands
- Release Announcement
- Session Streams
- Compile Outcomes
- Submission Outcomes
- Splash Wordmark
- Active Box
- Case Focus
- Graphify Rules
- Time Limit Multiplier
- Legacy Flag Migration
- C++ Compile Command
- Cargo Package

## God Nodes (most connected - your core abstractions)
1. `TestCase` - 50 edges
2. `temp_root()` - 43 edges
3. `p()` - 38 edges
4. `sample_problem()` - 37 edges
5. `FsOps` - 30 edges
6. `key()` - 30 edges
7. `ConfigMenu` - 28 edges
8. `paint()` - 28 edges
9. `Helper` - 28 edges
10. `Language` - 26 edges

## Surprising Connections (you probably didn't know these)
- `PaneDialog` --references--> `ScrollbarDrag`  [INFERRED]
  src/helper/dialog.rs → src/components/mod.rs
- `Motion` --references--> `Tone`  [INFERRED]
  src/helper/view/motion.rs → src/components/status.rs
- `sync_paged_page()` --references--> `TestCaseList`  [INFERRED]
  src/helper/dialog.rs → src/components/tc_list.rs
- `sheet_with()` --references--> `Rgb`  [INFERRED]
  src/components/ui.rs → src/components/theme.rs
- `regions()` --calls--> `row()`  [INFERRED]
  src/helper/view/mod.rs → src/components/ui.rs

## Import Cycles
- 1-file cycle: `src/components/mod.rs -> src/components/mod.rs`

## Hyperedges (group relationships)
- **Receiver / Runner / Submitter actors on one channel** — readme_single_channel_architecture, readme_problem_listener, readme_compile_cache, readme_safari_automation [EXTRACTED 1.00]
- **Settings persisted in config.json** — readme_config_json, readme_themes, readme_checker_modes, readme_compile_flag_tags [EXTRACTED 1.00]
- **Test result card layout: header, input, expected/actual diff** — sample_test_case_header, sample_input_panel, sample_expected_vs_actual_diff, sample_diff_line_highlight [EXTRACTED 1.00]

## Communities (76 total, 9 thin omitted)

### Community 0 - "Config Sheet and Flags"
Cohesion: 0.06
Nodes (47): a_value_is_refused_after_the_wrong_flag_and_says_why(), accepts_a_value_after_a_flag_that_takes_one(), adds_flag_tags_and_derives_the_cf_language_id(), backspace_only_edits_the_draft(), ConfigMenu, DeleteStage, Confirm, Off (+39 more)

### Community 1 - "Helper Integration Tests"
Cohesion: 0.11
Nodes (53): a_cut_error_opens_whole_on_a_double_click(), a_drag_selects_inside_one_pane_and_copies_on_release(), a_new_problem_replaces_the_previous_result_with_its_own(), add_edit_and_persist_custom_case(), adding_a_case_keeps_tab_inside_that_case(), anonymous_problems_with_the_same_name_do_not_overwrite(), cancelling_a_modal_does_not_echo_into_the_console(), clicking_a_pane_in_edit_mode_moves_the_focus() (+45 more)

### Community 2 - "Status Line and Notices"
Cohesion: 0.06
Nodes (29): errors_outrank_progress_and_results_answer_errors(), esc_dismisses_an_alert(), every_notice_is_logged(), LOG_LIMIT, Notice, progress_keeps_its_clock_across_phases(), Status, text_is_tidied_into_one_line() (+21 more)

### Community 3 - "Interactive Session UI"
Cohesion: 0.08
Nodes (27): SessionInput, Eof, Interrupt, fit_input(), floor_char_boundary(), Helper, InteractiveSession, SESSION_INPUT_BYTE_CAP (+19 more)

### Community 4 - "Theme Palettes"
Cohesion: 0.10
Nodes (40): file_icon(), draw_mark(), extra_style(), active(), ACTIVE, bg(), blend(), border() (+32 more)

### Community 5 - "Got Output Storage"
Cohesion: 0.06
Nodes (26): BOX_GAP, BOX_PAD_X, BOX_PAD_Y, CARD_CHROME_ROWS, CARD_PAD_RIGHT, COLLAPSED_HEIGHT, COLUMN_GAP, CONTENT_INDENT (+18 more)

### Community 6 - "Directory Console"
Cohesion: 0.14
Nodes (17): cd_moves_and_errors_say_what_went_wrong(), common_extension(), completes_common_prefix_and_lists_ambiguous_matches(), completes_from_home_prefix(), completes_nested_directories_level_by_level(), Completion, DirOutput, DirTerminal (+9 more)

### Community 7 - "Safari Submitter"
Cohesion: 0.10
Nodes (29): APP_NAME, APPLESCRIPT_TEMPLATE, build_applescript(), CF_BASELINE_JS, CF_FILL_JS, CF_RESULT_JS, classify(), derives_tracking_urls() (+21 more)

### Community 8 - "Chrome and Cut-Text Reveals"
Cohesion: 0.14
Nodes (7): format_elapsed(), reveal(), truncate(), truncation_never_exceeds_display_cells(), Helper, inset(), Helper

### Community 9 - "Pane Viewer Dialog"
Cohesion: 0.11
Nodes (16): PaneKind, Expected, Got, Input, Line, ClickTarget, Pane, Text (+8 more)

### Community 10 - "Splash Screen"
Cohesion: 0.09
Nodes (23): a_dead_listener_says_so(), BREATH, C, H, LETTER_GAP, Listener, Down, Up (+15 more)

### Community 11 - "Case Card Rendering"
Cohesion: 0.13
Nodes (24): a_fresh_verdict_colours_in(), a_single_enter_grows_the_input_pane(), CaseView, draw(), each_verdict_is_one_word_in_its_colour(), editing_caps_a_tall_case_to_the_viewport(), editing_marks_the_active_pane(), empty_or_stderr_only_huge_output_does_not_warn() (+16 more)

### Community 12 - "Companion Listener"
Cohesion: 0.10
Nodes (23): ToHelper, RunAction, Add, Edit, Run, Submit, COMMAND_TIMEOUT, decodes_encoded_query_values() (+15 more)

### Community 13 - "Output Checkers"
Cohesion: 0.12
Nodes (24): case_insensitive_tokens(), check(), CheckOutcome, CUSTOM_CHECKER_TIMEOUT, custom_checker_uses_exit_code_and_message(), failed(), float_eq(), float_tokens_use_absolute_or_relative_error() (+16 more)

### Community 14 - "Shared UI Primitives"
Cohesion: 0.16
Nodes (18): canvas(), clear_reveals(), dim(), field(), fill(), fill_buf(), GUTTER, hairline() (+10 more)

### Community 15 - "Compile Cache and PTY"
Cohesion: 0.09
Nodes (12): ACTIVE_SESSION_PGID, artifact_stamp(), ArtifactStamp, cache_record(), HARD_LIMIT_FACTOR, OUTPUT_LIMIT_BYTES, POLL_INTERVAL, SESSION_DRAIN_TIMEOUT (+4 more)

### Community 16 - "Path Breadcrumb Drawing"
Cohesion: 0.18
Nodes (13): breadcrumb(), output_line(), ease_out(), progress(), transitions_run_from_zero_to_one(), text_lines(), accent(), faint() (+5 more)

### Community 17 - "README Features"
Cohesion: 0.11
Nodes (18): CAPTCHA and 403/429 Handling, Checker Modes (tokens, lines, case_insensitive, float, custom), Codeforces, One-key Codeforces Submission, Competitive Companion, ~/.zch/config.json, Double-click Cut-text Viewer, Directory Console (cd/ls/pwd) (+10 more)

### Community 18 - "Case Pane Hit Areas"
Cohesion: 0.14
Nodes (4): diff_fits(), pane_lines(), pane_lines_add_a_note_only_when_something_is_cut(), TestCase

### Community 19 - "Case List Layout"
Cohesion: 0.14
Nodes (5): cards_keep_a_gutter_and_the_scrollbar_takes_the_last_column(), ListLook, mouse(), scrollbar_drag_scrolls_the_case_list(), tall_cases_scroll_through_their_full_height()

### Community 20 - "Program Execution"
Cohesion: 0.12
Nodes (21): crashes_name_the_signal_and_keep_partial_output(), describe_failure(), endless_output_is_stopped_at_the_limit(), exec(), Execution, Exited, FailedToStart, Killed (+13 more)

### Community 21 - "Output Capping"
Cohesion: 0.18
Nodes (12): Capped, capped_reports_hidden_lines_and_cut_characters(), has_stdout(), highlight_got_full(), highlight_got_inner(), line_from_runs(), line_runs(), muted() (+4 more)

### Community 22 - "Icons and Scrollbars"
Cohesion: 0.10
Nodes (17): COLLAPSED, CPP, DATA, EXPANDED, FILE, FOLDER, FOLDER_OPEN, JAVA (+9 more)

### Community 23 - "Component Imports"
Cohesion: 0.13
Nodes (12): PATH_GLOW, header_rows(), CHROME_ROWS, draw_too_small(), FOOTER_PAD, MIN_HEIGHT, MIN_WIDTH, Regions (+4 more)

### Community 24 - "Motion Timing"
Cohesion: 0.12
Nodes (17): active(), AMBIENT_FRAME, approach(), approach_converges(), breath(), enabled(), FLASH, FRAME (+9 more)

### Community 25 - "Time Limits and Cache Keys"
Cohesion: 0.17
Nodes (13): ActiveProblem, ReceivingTestCase, cache_key(), cache_record_path(), compile(), compile_cache_key_tracks_source_flags_and_language(), drain_reader(), pump_session() (+5 more)

### Community 26 - "Mouse Text Selection"
Cohesion: 0.22
Nodes (10): a_selection_reads_like_a_terminal_copy(), a_selection_stays_inside_its_panel(), clamp_to(), copy_to_clipboard(), Helper, pick(), rows(), selected_text() (+2 more)

### Community 27 - "View Motion State"
Cohesion: 0.12
Nodes (10): Helper, Motion, Sheet, Config, Delete, Help, Log, Pane (+2 more)

### Community 28 - "Helper Imports"
Cohesion: 0.14
Nodes (6): GOT_PAGE_LINES, PAGED_OUTPUT_NOTE, STDERR_MARKER, Helper, RunProgress, RunRecord

### Community 29 - "Filesystem Operations"
Cohesion: 0.20
Nodes (4): FsOps, short_hash(), source_url(), write_atomic()

### Community 30 - "Terminal Lifecycle"
Cohesion: 0.16
Nodes (7): ToSubmitter, main(), open_in_zed(), print_help(), run_app(), run_tui(), send_command()

### Community 31 - "Core Config Types"
Cohesion: 0.14
Nodes (3): std_from_flag(), std_from_flags(), std_parser_rejects_unknown_versions()

### Community 32 - "Runner Collection Tests"
Cohesion: 0.20
Nodes (15): cancellation_kills_a_hung_case(), collect_run(), collect_session(), compiled_binaries_survive_a_restart(), interactive_cancel_kills_the_child(), interactive_interrupt_reaches_the_child(), interactive_session_streams_input_and_eof(), java_recompiles_after_source_changes() (+7 more)

### Community 33 - "Atomic File Writes"
Cohesion: 0.18
Nodes (9): a_broken_config_is_backed_up_not_lost(), atomic_writes_leave_no_temp_files(), DEFAULT_CPP_BOILERPLATE, DEFAULT_JAVA_BOILERPLATE, DEFAULT_PY_BOILERPLATE, each_language_gets_its_own_template(), migrate_legacy_root(), temp_ops() (+1 more)

### Community 34 - "Release Workflow Jobs"
Cohesion: 0.14
Nodes (13): env, if, needs, runs-on, jobs, announce, name, on (+5 more)

### Community 35 - "Case Box Drawing"
Cohesion: 0.25
Nodes (6): clip_line(), frame_for(), gutter_label(), offset_rect(), overflows(), pane_box()

### Community 36 - "Helper Commands"
Cohesion: 0.14
Nodes (13): HelperCommand, CaseResult, Command, Error, ListenerDown, New, Quit, RunFinished (+5 more)

### Community 37 - "Run Dispatch"
Cohesion: 0.23
Nodes (7): PassingCommand, ToRunner, source_stamp(), RunRefusal, AlreadyRunning, NoProblem, Helper

### Community 39 - "Release Workflow Steps"
Cohesion: 0.38
Nodes (12): steps, steps, steps, steps, steps, steps, id, if (+4 more)

### Community 40 - "Case Verdicts"
Cohesion: 0.17
Nodes (8): CaseVerdict, AC, CE, Ole, Pending, RE, Tle, WA

### Community 41 - "Submit Planning"
Cohesion: 0.24
Nodes (11): SubmitConfig, SubmitterCommand, SubmitCode, describe(), plan(), safari_installed(), spawn_submitter(), SubmissionInfo (+3 more)

### Community 43 - "WA Diff Tests"
Cohesion: 0.36
Nodes (11): clip_line_marks_what_does_not_fit(), contents(), diff_does_not_shift_identical_lines(), diff_ignores_trailing_whitespace(), diff_is_positional_across_lines(), diff_marks_got_chars_out_of_place(), diff_marks_the_shifted_tail_after_a_missing_newline(), diff_matches_characters_within_each_line() (+3 more)

### Community 44 - "Error Types"
Cohesion: 0.20
Nodes (5): AppError, FileNotFound, Io, ParseError, UnsupportedPlatform

### Community 45 - "PTY Session"
Cohesion: 0.20
Nodes (4): configure_pty(), PtySession, pump_input(), spawn_session()

### Community 47 - "Input Modes"
Cohesion: 0.20
Nodes (10): InputMode, Config, ConfirmDelete, ConfirmQuit, ConfirmSubmit, DeleteSelect, Directory, Editing (+2 more)

### Community 48 - "Release Build Matrix"
Cohesion: 0.22
Nodes (9): container, if, name, needs, runs-on, strategy, build-local-artifacts, fail-fast (+1 more)

### Community 49 - "Release Artifact Upload"
Cohesion: 0.22
Nodes (9): with, merge-multiple, name, path, pattern, persist-credentials, repository, submodules (+1 more)

### Community 50 - "Scrollbar Drag"
Cohesion: 0.42
Nodes (6): a_small_drag_on_a_long_scrollbar_is_a_small_step(), dragging_a_scrollbar_thumb_moves_relative_to_the_grab_point(), round_div(), scroll_for(), ScrollbarDrag, thumb_geometry()

### Community 51 - "Process Group Control"
Cohesion: 0.25
Nodes (5): command(), kill_session_group(), stop(), stop_session_child(), wait_for_command()

### Community 52 - "Homebrew Publish Job"
Cohesion: 0.25
Nodes (8): GITHUB_EMAIL, GITHUB_USER, PLAN, publish-homebrew-formula, env, if, needs, runs-on

### Community 53 - "Screenshot: Case Card"
Cohesion: 0.29
Nodes (5): Input Panel (inp), Runtime and Memory Usage Metrics, Test Case Header (index, verdict, time and memory), Test Case Result Card (TUI screenshot), Wrong Answer Verdict

### Community 54 - "Escape Sequence Filter"
Cohesion: 0.25
Nodes (7): ControlFilter, FilterState, Csi, Escape, Normal, Osc, OscEscape

### Community 56 - "Release Host Job"
Cohesion: 0.33
Nodes (7): env, needs, runs-on, env, BUILD_MANIFEST_NAME, GH_TOKEN, build-global-artifacts

### Community 57 - "Release Host Outputs"
Cohesion: 0.29
Nodes (7): env, if, needs, outputs, runs-on, host, val

### Community 58 - "Release Plan Job"
Cohesion: 0.29
Nodes (7): plan, publishing, tag, tag-flag, env, outputs, runs-on

### Community 59 - "Paged Got File"
Cohesion: 0.38
Nodes (4): GotFile, PageLine, read_line_limited(), skip_line()

### Community 60 - "Runner Verdicts"
Cohesion: 0.29
Nodes (6): Verdict, CompilationError, OutputLimitExceeded, RuntimeError, Success, TimeLimitExceeded

### Community 61 - "Console Key Fitting"
Cohesion: 0.33
Nodes (6): fit_keys(), HEADER_GAP, HEADER_PAD, HELP_KEY, keys_drop_from_the_right_and_can_vanish(), SEGMENT_END

### Community 62 - "Run Result Handling"
Cohesion: 0.47
Nodes (3): format_ms(), units_are_spelled_one_way(), Helper

### Community 63 - "Runner Channel Commands"
Cohesion: 0.33
Nodes (3): RunnerCommand, Interact, RunCode

### Community 64 - "Release Announcement"
Cohesion: 0.40
Nodes (5): ANNOUNCEMENT_BODY, ANNOUNCEMENT_TITLE, PRERELEASE_FLAG, RELEASE_COMMIT, env

### Community 65 - "Session Streams"
Cohesion: 0.40
Nodes (4): SessionStream, Err, Out, Status

### Community 66 - "Compile Outcomes"
Cohesion: 0.40
Nodes (5): Compile, Cancelled, Compiled, NotNeeded, Skipped

### Community 67 - "Submission Outcomes"
Cohesion: 0.40
Nodes (5): Outcome, Accepted, Failed, PartiallyAccepted, Rejected

### Community 69 - "Active Box"
Cohesion: 0.50
Nodes (4): ActiveBox, Expected, Input, None

### Community 70 - "Case Focus"
Cohesion: 0.50
Nodes (4): CaseFocus, Delete, Edit, None

### Community 71 - "Graphify Rules"
Cohesion: 1.00
Nodes (3): graphify-out Knowledge Graph, Graphify Usage Rules, graphify update (AST-only refresh)

## Knowledge Gaps
- **133 isolated node(s):** `zch`, `Off`, `Select`, `Confirm`, `Progress` (+128 more)
  These have ≤1 connection - possible missing edges or undocumented components. (Counts symbols only; 395 node(s) total have ≤1 connection when file, concept and rationale nodes are included.)
- **9 thin communities (<3 nodes) omitted from report** — run `graphify query` to explore isolated nodes.

## Suggested Questions
_Questions this graph is uniquely positioned to answer:_

- **Why does `run` connect `Release Workflow Steps` to `Helper Imports`?**
  _High betweenness centrality (0.099) - this node is a cross-community bridge._
- **What connects `zch`, `Off`, `Select` to the rest of the system?**
  _133 weakly-connected nodes found - possible documentation gaps or missing edges._
- **Should `Config Sheet and Flags` be split into smaller, more focused modules?**
  _Cohesion score 0.05730238025271819 - nodes in this community are weakly interconnected._
- **Why does `TestCase` connect `Case Pane Hit Areas` to `Case Box Drawing`, `Got Output Storage`, `Case Verdicts`, `Pane Viewer Dialog`, `Test Case List`, `Case Card Rendering`, `Path Breadcrumb Drawing`, `Case List Layout`, `Output Capping`, `Icons and Scrollbars`, `Motion Timing`, `Paged Got File`, `Helper Imports`?**
  _High betweenness centrality (0.049) - this node is a cross-community bridge._
- **Should `Helper Integration Tests` be split into smaller, more focused modules?**
  _Cohesion score 0.10957501280081926 - nodes in this community are weakly interconnected._
- **Why does `Helper` connect `Helper Imports` to `Config Sheet and Flags`, `Helper Integration Tests`, `Status Line and Notices`, `Interactive Session UI`, `Directory Console`, `Pane Viewer Dialog`, `Splash Screen`, `Test Case List`, `Companion Listener`, `Submit Planning`, `Input Modes`, `Scrollbar Drag`, `Time Limits and Cache Keys`, `Mouse Text Selection`, `View Motion State`, `Filesystem Operations`, `Terminal Lifecycle`?**
  _High betweenness centrality (0.044) - this node is a cross-community bridge._
- **Should `Status Line and Notices` be split into smaller, more focused modules?**
  _Cohesion score 0.05573770491803279 - nodes in this community are weakly interconnected._