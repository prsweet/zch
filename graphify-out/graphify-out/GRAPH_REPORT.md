# Graph Report - graphify-out  (2026-10-24)

## Corpus Check
- cluster-only mode — file stats not available

## Summary
- 589 nodes · 1498 edges · 25 communities (23 shown, 2 thin omitted)
- Extraction: 98% EXTRACTED · 2% INFERRED · 0% AMBIGUOUS · INFERRED: 27 edges (avg confidence: 0.85)
- Token cost: 0 input · 0 output

## Graph Freshness
- Built from commit: `b637f386`
- Run `git rev-parse HEAD` and compare to check if the graph is stale.
- Run `graphify update .` after code changes (no API cost).

## Community Hubs (Navigation)
- helper.rs
- tc_list.rs
- config.rs
- dir_terminal.rs
- runner.rs
- FsOps
- components/mod.rs
- submitter.rs
- receiver.rs
- testcase.rs
- main.rs
- TestCase
- .got_view
- core/mod.rs
- Language
- .new
- ActiveProblem
- plan
- AppError
- CaseVerdict
- HelperCommand
- .draw_tc
- InputMode
- Outcome
- zch

## God Nodes (most connected - your core abstractions)
1. `Helper` - 57 edges
2. `TestCase` - 34 edges
3. `temp_root()` - 29 edges
4. `FsOps` - 25 edges
5. `sample_problem()` - 25 edges
6. `key()` - 24 edges
7. `Language` - 23 edges
8. `PassingCommand` - 22 edges
9. `ActiveProblem` - 20 edges
10. `HelperCommand` - 20 edges

## Surprising Connections (you probably didn't know these)
- `spawn_server()` --references--> `ToHelper`  [INFERRED]
  src/core/receiver.rs → src/core/mod.rs
- `run_app()` --calls--> `spawn_server()`  [INFERRED]
  src/main.rs → src/core/receiver.rs
- `run_app()` --calls--> `spawn_runner()`  [INFERRED]
  src/main.rs → src/core/runner.rs
- `spawn_server()` --references--> `PassingCommand`  [EXTRACTED]
  src/core/receiver.rs → src/core/mod.rs
- `collect_run()` --references--> `PassingCommand`  [EXTRACTED]
  src/core/runner.rs → src/core/mod.rs

## Import Cycles
- 1-file cycle: `src/components/mod.rs -> src/components/mod.rs`

## Communities (25 total, 2 thin omitted)

### Community 0 - "helper.rs"
Cohesion: 0.07
Nodes (50): block(), PassingCommand, ToRunner, ToSubmitter, active_working_directory_uses_surface_background(), add_edit_and_persist_custom_case(), anonymous_problems_with_the_same_name_do_not_overwrite(), clicking_a_pane_in_edit_mode_moves_the_focus() (+42 more)

### Community 1 - "tc_list.rs"
Cohesion: 0.05
Nodes (21): cell_t(), gradient(), LOGO, logo_lines(), render(), splash_renders_on_narrow_and_wide_terminals(), STOPS, tall_cases_scroll_through_their_full_height() (+13 more)

### Community 2 - "config.rs"
Cohesion: 0.12
Nodes (26): accepts_a_value_after_a_flag_that_takes_one(), adds_flag_tags_and_derives_the_cf_language_id(), backspace_only_edits_the_draft(), ConfigMenu, DeleteStage, Confirm, Off, Select (+18 more)

### Community 3 - "dir_terminal.rs"
Cohesion: 0.11
Nodes (14): common_extension(), completes_common_prefix_and_lists_ambiguous_matches(), completes_from_home_prefix(), completes_nested_directories_level_by_level(), Completion, DirTerminal, HISTORY_LIMIT, key() (+6 more)

### Community 4 - "runner.rs"
Cohesion: 0.13
Nodes (16): artifact_stamp(), ArtifactStamp, CacheEntry, cancellation_kills_a_hung_case(), collect_run(), command(), java_recompiles_after_source_changes(), MAX_CACHE_ENTRIES (+8 more)

### Community 5 - "FsOps"
Cohesion: 0.11
Nodes (8): DEFAULT_CPP_BOILERPLATE, DEFAULT_JAVA_BOILERPLATE, DEFAULT_PY_BOILERPLATE, FsOps, migrate_legacy_root(), short_hash(), source_url(), UserConfig

### Community 6 - "components/mod.rs"
Cohesion: 0.13
Nodes (15): ACCENT, BORDER, CYAN, FILE, FOLDER, GREEN, Message, MUTED (+7 more)

### Community 7 - "submitter.rs"
Cohesion: 0.13
Nodes (17): APP_NAME, APPLESCRIPT_TEMPLATE, build_applescript(), CF_FILL_JS, CF_RESULT_JS, derives_tracking_urls(), detect_platform(), detects_codeforces_contest() (+9 more)

### Community 8 - "receiver.rs"
Cohesion: 0.12
Nodes (12): RunAction, Add, Edit, Run, Submit, COMMAND_TIMEOUT, handle_command_url(), listen_port() (+4 more)

### Community 9 - "testcase.rs"
Cohesion: 0.13
Nodes (16): COLLAPSED_HEIGHT, diff_ignores_trailing_whitespace(), diff_is_positional_across_lines(), diff_marks_got_chars_out_of_place(), diff_marks_the_shifted_tail_after_a_missing_newline(), DIFF_MAX_CHARS, DIFF_MAX_LINES, diff_never_invents_missing_characters() (+8 more)

### Community 10 - "main.rs"
Cohesion: 0.13
Nodes (8): ToHelper, spawn_submitter(), main(), open_in_zed(), print_help(), run_app(), run_tui(), send_command()

### Community 12 - ".got_view"
Cohesion: 0.18
Nodes (10): Capped, capped_reports_hidden_lines_and_cut_characters(), contents(), diff_fits(), muted(), pane_lines(), pane_lines_add_a_note_only_when_something_is_cut(), red_spans() (+2 more)

### Community 13 - "core/mod.rs"
Cohesion: 0.12
Nodes (7): RunnerCommand, RunCode, Verdict, CompilationError, RuntimeError, Success, TimeLimitExceeded

### Community 14 - "Language"
Cohesion: 0.14
Nodes (7): cpp_compiles_without_implicit_flags(), Language, .ALL, Cpp, Java, Python, std_from_flags()

### Community 15 - ".new"
Cohesion: 0.19
Nodes (8): a_single_enter_grows_the_input_pane(), delete_focus_turns_every_pane_border_red(), giant_output_is_sized_to_the_capped_pane(), height_tracks_content_and_collapse(), huge_wa_output_is_cut_on_arrival_and_never_diffed(), retain_output_preview(), screen_of(), wa_previews_over_the_diff_budget_are_plain_not_diffed()

### Community 16 - "ActiveProblem"
Cohesion: 0.21
Nodes (12): ActiveProblem, ReceivingTestCase, artifact_path(), cache_key(), CacheKey, Compile, compile(), compile_cache_key_tracks_source_flags_and_language() (+4 more)

### Community 17 - "plan"
Cohesion: 0.23
Nodes (12): SubmitConfig, SubmitterCommand, SubmitCode, describe(), plan(), safari_installed(), strip_metadata(), strips_url_and_test_blocks() (+4 more)

### Community 18 - "AppError"
Cohesion: 0.18
Nodes (5): AppError, FileNotFound, Io, ParseError, UnsupportedPlatform

### Community 19 - "CaseVerdict"
Cohesion: 0.18
Nodes (7): CaseVerdict, AC, CE, Pending, RE, Tle, WA

### Community 20 - "HelperCommand"
Cohesion: 0.18
Nodes (10): HelperCommand, CaseResult, Command, Error, New, Quit, RunFinished, RunPhase (+2 more)

### Community 21 - ".draw_tc"
Cohesion: 0.31
Nodes (4): clip_line(), clip_line_marks_what_does_not_fit(), inner_width(), offset_rect()

### Community 22 - "InputMode"
Cohesion: 0.25
Nodes (8): InputMode, Config, ConfirmDelete, ConfirmSubmit, DeleteSelect, Directory, Editing, Normal

### Community 23 - "Outcome"
Cohesion: 0.40
Nodes (5): classify(), Outcome, Accepted, Failed, Rejected

## Knowledge Gaps
- **60 isolated node(s):** `AlreadyRunning`, `NoProblem`, `Expected`, `Input`, `None` (+55 more)
  These have ≤1 connection - possible missing edges. (Counts symbols only; 153 node(s) total have ≤1 connection when file, concept and rationale nodes are included.)
- **2 thin communities (<3 nodes) omitted from report** — run `graphify query` to explore isolated nodes.

## Suggested Questions
_Questions this graph is uniquely positioned to answer:_

- **Why does `Helper` connect `helper.rs` to `tc_list.rs`, `config.rs`, `dir_terminal.rs`, `FsOps`, `main.rs`, `ActiveProblem`, `plan`, `InputMode`?**
  _High betweenness centrality (0.120) - this node is a cross-community bridge._
- **What connects `AlreadyRunning`, `NoProblem`, `Expected` to the rest of the system?**
  _60 weakly-connected nodes found - possible documentation gaps or missing edges._
- **Should `helper.rs` be split into smaller, more focused modules?**
  _Cohesion score 0.0698289543290425 - nodes in this community are weakly interconnected._
- **Why does `TestCase` connect `TestCase` to `helper.rs`, `tc_list.rs`, `components/mod.rs`, `testcase.rs`, `.got_view`, `.new`, `CaseVerdict`, `.draw_tc`?**
  _High betweenness centrality (0.089) - this node is a cross-community bridge._
- **Should `tc_list.rs` be split into smaller, more focused modules?**
  _Cohesion score 0.05084745762711865 - nodes in this community are weakly interconnected._
- **Why does `Language` connect `Language` to `helper.rs`, `config.rs`, `runner.rs`, `FsOps`, `core/mod.rs`, `ActiveProblem`, `plan`?**
  _High betweenness centrality (0.057) - this node is a cross-community bridge._
- **Should `config.rs` be split into smaller, more focused modules?**
  _Cohesion score 0.11818181818181818 - nodes in this community are weakly interconnected._