# Graph Report - zch  (2026-10-26)

## Corpus Check
- Corpus is ~34,970 words - fits in a single context window. You may not need a graph.

## Summary
- 837 nodes · 2120 edges · 42 communities (40 shown, 2 thin omitted)
- Extraction: 99% EXTRACTED · 1% INFERRED · 0% AMBIGUOUS · INFERRED: 31 edges (avg confidence: 0.85)
- Token cost: 0 input · 0 output

## Community Hubs (Navigation)
- Helper UI Tests
- Directory Console
- Helper Controller
- Config Flag Editor
- Theme And Scrollbars
- Case Dialog Limits
- Case List Viewport
- Product Overview
- Codeforces Submitter
- Test Runner Engine
- Case Layout And Diff
- Verdict Color Chips
- Companion HTTP Listener
- Splash Logo
- Config Persistence
- Terminal Event Loop
- Output Retention
- Output Highlighting
- Problem File Storage
- Submit Plan And Errors
- Release Job Graph
- Release Workflow Steps
- Runner Process Control
- Language Commands
- Language Boilerplate
- Helper Command Channel
- Release Publish Outputs
- Release Environment
- Local Artifact Matrix
- Checkout Action Inputs
- Homebrew Formula Publish
- Paged Output Spill
- Input Modes
- Run Verdict Outcomes
- Release Announcement
- Runner Docs
- Submitter Channel
- Compile Phase States
- Submission Classification
- Workflow Triggers
- Project Root
- WA Screenshot

## God Nodes (most connected - your core abstractions)
1. `Helper` - 63 edges
2. `temp_root()` - 45 edges
3. `TestCase` - 41 edges
4. `sample_problem()` - 39 edges
5. `key()` - 32 edges
6. `DirTerminal` - 30 edges
7. `ConfigMenu` - 25 edges
8. `FsOps` - 25 edges
9. `Language` - 24 edges
10. `PassingCommand` - 22 edges

## Surprising Connections (you probably didn't know these)
- `spawn_server()` --references--> `ToHelper`  [INFERRED]
  src/core/receiver.rs → src/core/mod.rs
- `run_app()` --calls--> `spawn_server()`  [INFERRED]
  src/main.rs → src/core/receiver.rs
- `run_app()` --calls--> `spawn_runner()`  [INFERRED]
  src/main.rs → src/core/runner.rs
- `run_app()` --calls--> `spawn_submitter()`  [INFERRED]
  src/main.rs → src/core/submitter.rs
- `ConfigMenu` --references--> `ScrollbarDrag`  [EXTRACTED]
  src/components/config.rs → src/components/mod.rs

## Import Cycles
- 1-file cycle: `src/components/mod.rs -> src/components/mod.rs`

## Hyperedges (group relationships)
- **UI thread plus receiver, runner, and submitter share one PassingCommand channel** — readme_main_rs, readme_receiver, readme_runner, readme_submitter, readme_passing_command [EXTRACTED 1.00]

## Communities (42 total, 2 thin omitted)

### Community 0 - "Helper UI Tests"
Cohesion: 0.13
Nodes (56): ToSubmitter, add_edit_and_persist_custom_case(), adding_a_case_keeps_tab_inside_that_case(), anonymous_problems_with_the_same_name_do_not_overwrite(), clicking_a_pane_in_edit_mode_moves_the_focus(), config_menu_cycles_language_and_saves(), console_does_not_repeat_footer_mode_hints(), delete_selects_and_asks_for_confirmation() (+48 more)

### Community 1 - "Directory Console"
Cohesion: 0.08
Nodes (24): common_extension(), completes_common_prefix_and_lists_ambiguous_matches(), completes_from_home_prefix(), completes_nested_directories_level_by_level(), Completion, CONSOLE_HEIGHT, console_height_is_one_line_and_stable_for_wrapped_messages(), console_renders_the_active_message_on_one_line() (+16 more)

### Community 2 - "Helper Controller"
Cohesion: 0.07
Nodes (14): block(), DialogContent, Lines, Paged, dim_area(), draw_hairline(), Helper, PaneDialog (+6 more)

### Community 3 - "Config Flag Editor"
Cohesion: 0.11
Nodes (30): accepts_a_value_after_a_flag_that_takes_one(), adds_flag_tags_and_derives_the_cf_language_id(), backspace_only_edits_the_draft(), ConfigMenu, DeleteStage, Confirm, Off, Select (+22 more)

### Community 4 - "Theme And Scrollbars"
Cohesion: 0.08
Nodes (30): a_small_drag_on_a_long_scrollbar_is_a_small_step(), ACCENT(), apply(), BORDER(), CYAN(), dragging_a_scrollbar_thumb_moves_relative_to_the_grab_point(), FILE, FOLDER (+22 more)

### Community 5 - "Case Dialog Limits"
Cohesion: 0.06
Nodes (29): COLLAPSED_HEIGHT, DIALOG_SAFE_MAX_BYTES, DIALOG_SAFE_MAX_LINES, diff_does_not_shift_identical_lines(), diff_ignores_trailing_whitespace(), diff_is_positional_across_lines(), diff_marks_got_chars_out_of_place(), diff_marks_the_shifted_tail_after_a_missing_newline() (+21 more)

### Community 6 - "Case List Viewport"
Cohesion: 0.07
Nodes (17): list_only_reserves_a_column_when_the_scrollbar_is_needed(), mouse(), scrollbar_drag_scrolls_the_case_list(), tall_cases_scroll_through_their_full_height(), TestCaseList, ActiveBox, Expected, Input (+9 more)

### Community 7 - "Product Overview"
Cohesion: 0.08
Nodes (27): CAPTCHA Handling, One-Key Codeforces Submission, Codeforces, Competitive Companion, Live Directory Console, Validated Compile-Flag Tags, fs_ops.rs Storage, helper.rs UI State Machine (+19 more)

### Community 8 - "Codeforces Submitter"
Cohesion: 0.12
Nodes (19): APP_NAME, APPLESCRIPT_TEMPLATE, build_applescript(), CF_FILL_JS, CF_RESULT_JS, derives_tracking_urls(), detect_platform(), detects_codeforces_contest() (+11 more)

### Community 9 - "Test Runner Engine"
Cohesion: 0.16
Nodes (17): ActiveProblem, ReceivingTestCase, artifact_path(), artifact_stamp(), ArtifactStamp, cache_key(), CacheEntry, CacheKey (+9 more)

### Community 10 - "Case Layout And Diff"
Cohesion: 0.14
Nodes (3): diff_fits(), editing_caps_a_tall_case_to_the_viewport(), TestCase

### Community 11 - "Verdict Color Chips"
Cohesion: 0.12
Nodes (11): CaseVerdict, AC, CE, Pending, RE, Tle, WA, clip_line() (+3 more)

### Community 12 - "Companion HTTP Listener"
Cohesion: 0.13
Nodes (12): RunAction, Add, Edit, Run, Submit, COMMAND_TIMEOUT, handle_command_url(), listen_port() (+4 more)

### Community 13 - "Splash Logo"
Cohesion: 0.14
Nodes (7): cell_t(), gradient(), LOGO, logo_lines(), render(), splash_renders_on_narrow_and_wide_terminals(), STOPS

### Community 14 - "Config Persistence"
Cohesion: 0.12
Nodes (5): legacy_fallback_flags_become_the_current_language_flags(), normalize_prefers_the_selected_language_flags(), std_from_flag(), std_from_flags(), UserConfig

### Community 15 - "Terminal Event Loop"
Cohesion: 0.13
Nodes (7): ToRunner, main(), open_in_zed(), print_help(), run_app(), run_tui(), send_command()

### Community 16 - "Output Retention"
Cohesion: 0.18
Nodes (13): a_single_enter_grows_the_input_pane(), case_title_is_plain_before_a_run(), delete_focus_marks_the_title_and_keeps_the_verdict_color(), editing_uses_accent_focus(), empty_or_stderr_only_huge_output_does_not_warn(), height_tracks_content_and_collapse(), huge_wa_output_is_cut_on_arrival_and_never_diffed(), one_e7_output_stays_in_memory_with_a_full_dialog_diff() (+5 more)

### Community 17 - "Output Highlighting"
Cohesion: 0.18
Nodes (12): Capped, capped_reports_hidden_lines_and_cut_characters(), contents(), highlight_got_full(), highlight_got_inner(), line_from_runs(), line_runs(), muted() (+4 more)

### Community 18 - "Problem File Storage"
Cohesion: 0.18
Nodes (4): FsOps, migrate_legacy_root(), short_hash(), source_url()

### Community 19 - "Submit Plan And Errors"
Cohesion: 0.17
Nodes (13): SubmitConfig, describe(), plan(), safari_installed(), SubmissionInfo, submit(), url_from_file(), AppError (+5 more)

### Community 20 - "Release Job Graph"
Cohesion: 0.17
Nodes (11): if, needs, runs-on, needs, runs-on, jobs, announce, build-global-artifacts (+3 more)

### Community 21 - "Release Workflow Steps"
Cohesion: 0.38
Nodes (12): steps, steps, steps, steps, steps, steps, id, if (+4 more)

### Community 22 - "Runner Process Control"
Cohesion: 0.21
Nodes (9): PassingCommand, RunnerCommand, RunCode, cancellation_kills_a_hung_case(), collect_run(), java_recompiles_after_source_changes(), second_run_reuses_the_compiled_binary(), spawn_runner() (+1 more)

### Community 23 - "Language Commands"
Cohesion: 0.17
Nodes (6): cpp_compiles_without_implicit_flags(), Language, .ALL, Cpp, Java, Python

### Community 24 - "Language Boilerplate"
Cohesion: 0.18
Nodes (3): DEFAULT_CPP_BOILERPLATE, DEFAULT_JAVA_BOILERPLATE, DEFAULT_PY_BOILERPLATE

### Community 25 - "Helper Command Channel"
Cohesion: 0.18
Nodes (10): HelperCommand, CaseResult, Command, Error, New, Quit, RunFinished, RunPhase (+2 more)

### Community 26 - "Release Publish Outputs"
Cohesion: 0.20
Nodes (10): if, needs, outputs, runs-on, host, publishing, tag, tag-flag (+2 more)

### Community 27 - "Release Environment"
Cohesion: 0.25
Nodes (9): env, env, env, BUILD_MANIFEST_NAME, GH_TOKEN, env, plan, env (+1 more)

### Community 28 - "Local Artifact Matrix"
Cohesion: 0.22
Nodes (9): container, if, name, needs, runs-on, strategy, build-local-artifacts, fail-fast (+1 more)

### Community 29 - "Checkout Action Inputs"
Cohesion: 0.22
Nodes (9): with, merge-multiple, name, path, pattern, persist-credentials, repository, submodules (+1 more)

### Community 30 - "Homebrew Formula Publish"
Cohesion: 0.25
Nodes (8): GITHUB_EMAIL, GITHUB_USER, PLAN, publish-homebrew-formula, env, if, needs, runs-on

### Community 31 - "Paged Output Spill"
Cohesion: 0.32
Nodes (4): GotFile, PageLine, read_line_limited(), skip_line()

### Community 32 - "Input Modes"
Cohesion: 0.25
Nodes (8): InputMode, Config, ConfirmDelete, ConfirmSubmit, DeleteSelect, Directory, Editing, Normal

### Community 33 - "Run Verdict Outcomes"
Cohesion: 0.33
Nodes (5): Verdict, CompilationError, RuntimeError, Success, TimeLimitExceeded

### Community 34 - "Release Announcement"
Cohesion: 0.40
Nodes (5): ANNOUNCEMENT_BODY, ANNOUNCEMENT_TITLE, PRERELEASE_FLAG, RELEASE_COMMIT, env

### Community 35 - "Runner Docs"
Cohesion: 0.40
Nodes (4): Content-Hash Compile Cache, runner.rs Test Runner, sample.png 1ms Run Screenshot, In-Memory Smoke Case

### Community 36 - "Submitter Channel"
Cohesion: 0.40
Nodes (4): ToHelper, SubmitterCommand, SubmitCode, spawn_submitter()

### Community 37 - "Compile Phase States"
Cohesion: 0.40
Nodes (5): Compile, Cancelled, Compiled, NotNeeded, Skipped

### Community 38 - "Submission Classification"
Cohesion: 0.40
Nodes (5): classify(), Outcome, Accepted, Failed, Rejected

### Community 39 - "Workflow Triggers"
Cohesion: 0.50
Nodes (4): tags, True, pull_request, push

## Knowledge Gaps
- **81 isolated node(s):** `zch`, `Off`, `Select`, `Confirm`, `Hint` (+76 more)
  These have ≤1 connection - possible missing edges or undocumented components. (Counts symbols only; 232 node(s) total have ≤1 connection when file, concept and rationale nodes are included.)
- **2 thin communities (<3 nodes) omitted from report** — run `graphify query` to explore isolated nodes.

## Suggested Questions
_Questions this graph is uniquely positioned to answer:_

- **Why does `Helper` connect `Helper Controller` to `Helper UI Tests`, `Directory Console`, `Input Modes`, `Config Flag Editor`, `Theme And Scrollbars`, `Case List Viewport`, `Test Runner Engine`, `Terminal Event Loop`, `Problem File Storage`, `Submit Plan And Errors`?**
  _High betweenness centrality (0.086) - this node is a cross-community bridge._
- **What connects `zch`, `Off`, `Select` to the rest of the system?**
  _81 weakly-connected nodes found - possible documentation gaps or missing edges._
- **Should `Helper UI Tests` be split into smaller, more focused modules?**
  _Cohesion score 0.13476490188818957 - nodes in this community are weakly interconnected._
- **Why does `TestCase` connect `Case Layout And Diff` to `Helper UI Tests`, `Theme And Scrollbars`, `Case Dialog Limits`, `Case List Viewport`, `Verdict Color Chips`, `Output Retention`, `Output Highlighting`, `Paged Output Spill`?**
  _High betweenness centrality (0.067) - this node is a cross-community bridge._
- **Should `Directory Console` be split into smaller, more focused modules?**
  _Cohesion score 0.08143839238498149 - nodes in this community are weakly interconnected._
- **Why does `DirTerminal` connect `Directory Console` to `Helper UI Tests`, `Helper Controller`?**
  _High betweenness centrality (0.053) - this node is a cross-community bridge._
- **Should `Helper Controller` be split into smaller, more focused modules?**
  _Cohesion score 0.06885245901639345 - nodes in this community are weakly interconnected._