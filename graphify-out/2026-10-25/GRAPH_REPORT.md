# Graph Report - zch  (2026-10-24)

## Corpus Check
- Corpus is ~30,449 words - fits in a single context window. You may not need a graph.

## Summary
- 618 nodes · 1549 edges · 34 communities (29 shown, 5 thin omitted)
- Extraction: 97% EXTRACTED · 3% INFERRED · 0% AMBIGUOUS · INFERRED: 41 edges (avg confidence: 0.86)
- Token cost: 0 input · 0 output

## Community Hubs (Navigation)
- Helper UI and Tests
- Config Menu and Flags
- Directory Terminal Console
- Runner Cache and Compilation
- Codeforces Submission Automation
- Theme and Visual State
- Command Receiver and Actions
- Got Diff Highlighting
- Four-Actor App Architecture
- TestCase Layout and Scrolling
- Splash Screen Rendering
- Output Capping Utilities
- Core Commands and Verdicts
- Filesystem Operations
- Problem Config and Caching
- Language Compile Matrix
- TestCase Preview Tests
- TestCase List Navigation
- Submit Config and Tracking
- Error Types and Formatting
- Boilerplate and Test Database
- List Events and Verdict Chips
- Case Verdict Styles
- Helper Command Protocol
- Pane Clipping and Rendering
- Case Focus and Frames
- README Product Overview
- Input Mode State Machine
- Release Workflow Jobs
- Pane Kinds and Hit Testing
- Submission Outcome Classification
- Active Box Selection
- ZCH Project
- WA Verdict Screenshot

## God Nodes (most connected - your core abstractions)
1. `Helper` - 57 edges
2. `TestCase` - 34 edges
3. `temp_root()` - 29 edges
4. `FsOps` - 25 edges
5. `sample_problem()` - 25 edges
6. `key()` - 24 edges
7. `Language` - 23 edges
8. `PassingCommand` - 22 edges
9. `DirTerminal` - 20 edges
10. `ActiveProblem` - 20 edges

## Surprising Connections (you probably didn't know these)
- `spawn_server()` --references--> `ToHelper`  [INFERRED]
  src/core/receiver.rs → src/core/mod.rs
- `run_app()` --calls--> `spawn_server()`  [INFERRED]
  src/main.rs → src/core/receiver.rs
- `run_app()` --calls--> `spawn_runner()`  [INFERRED]
  src/main.rs → src/core/runner.rs
- `ConfigMenu` --references--> `Language`  [EXTRACTED]
  src/components/config.rs → src/core/mod.rs
- `Helper` --references--> `ConfigMenu`  [EXTRACTED]
  src/helper.rs → src/components/config.rs

## Import Cycles
- 1-file cycle: `src/components/mod.rs -> src/components/mod.rs`

## Hyperedges (group relationships)
- **cargo-dist Release Pipeline** — github_workflows_release_plan, github_workflows_release_build_local_artifacts, github_workflows_release_build_global_artifacts, github_workflows_release_host, github_workflows_release_publish_homebrew_formula, github_workflows_release_announce, github_workflows_release_dist_tool [EXTRACTED 1.00]
- **Single-Channel Actor Architecture** — src_main, src_helper, src_core_receiver, src_core_runner, src_core_submitter, readme_passingcommand [EXTRACTED 1.00]
- **Ratatui UI Components** — src_components_dir_terminal, src_components_tc_list, src_components_testcase, src_components_config, src_components_splash [EXTRACTED 1.00]
- **Codeforces Submission Flow** — src_core_submitter, readme_safari_automation, readme_codeforces, readme_captcha_handling, readme_submissions_bar [EXTRACTED 1.00]

## Communities (34 total, 5 thin omitted)

### Community 0 - "Helper UI and Tests"
Cohesion: 0.07
Nodes (50): block(), PassingCommand, ToRunner, ToSubmitter, active_working_directory_uses_surface_background(), add_edit_and_persist_custom_case(), anonymous_problems_with_the_same_name_do_not_overwrite(), clicking_a_pane_in_edit_mode_moves_the_focus() (+42 more)

### Community 1 - "Config Menu and Flags"
Cohesion: 0.12
Nodes (26): accepts_a_value_after_a_flag_that_takes_one(), adds_flag_tags_and_derives_the_cf_language_id(), backspace_only_edits_the_draft(), ConfigMenu, DeleteStage, Confirm, Off, Select (+18 more)

### Community 2 - "Directory Terminal Console"
Cohesion: 0.11
Nodes (14): common_extension(), completes_common_prefix_and_lists_ambiguous_matches(), completes_from_home_prefix(), completes_nested_directories_level_by_level(), Completion, DirTerminal, HISTORY_LIMIT, key() (+6 more)

### Community 3 - "Runner Cache and Compilation"
Cohesion: 0.11
Nodes (20): Content-Hash Compile Cache, Precompiled Headers (PCH), std-with-pbds Helper Repo, macOS syspolicyd Startup Latency, artifact_stamp(), ArtifactStamp, CacheEntry, cancellation_kills_a_hung_case() (+12 more)

### Community 4 - "Codeforces Submission Automation"
Cohesion: 0.11
Nodes (20): CAPTCHA Foreground Handling, Safari AppleScript Submission Automation, Live Verdict Polling / Submissions Bar, APP_NAME, APPLESCRIPT_TEMPLATE, build_applescript(), CF_FILL_JS, CF_RESULT_JS (+12 more)

### Community 5 - "Theme and Visual State"
Cohesion: 0.13
Nodes (15): ACCENT, BORDER, CYAN, FILE, FOLDER, GREEN, Message, MUTED (+7 more)

### Community 6 - "Command Receiver and Actions"
Cohesion: 0.11
Nodes (13): PassingCommand Envelope, RunAction, Add, Edit, Run, Submit, COMMAND_TIMEOUT, handle_command_url() (+5 more)

### Community 7 - "Got Diff Highlighting"
Cohesion: 0.13
Nodes (16): COLLAPSED_HEIGHT, diff_ignores_trailing_whitespace(), diff_is_positional_across_lines(), diff_marks_got_chars_out_of_place(), diff_marks_the_shifted_tail_after_a_missing_newline(), DIFF_MAX_CHARS, DIFF_MAX_LINES, diff_never_invents_missing_characters() (+8 more)

### Community 8 - "Four-Actor App Architecture"
Cohesion: 0.13
Nodes (8): ToHelper, spawn_submitter(), main(), open_in_zed(), print_help(), run_app(), run_tui(), send_command()

### Community 10 - "Splash Screen Rendering"
Cohesion: 0.15
Nodes (7): cell_t(), gradient(), LOGO, logo_lines(), render(), splash_renders_on_narrow_and_wide_terminals(), STOPS

### Community 11 - "Output Capping Utilities"
Cohesion: 0.18
Nodes (10): Capped, capped_reports_hidden_lines_and_cut_characters(), contents(), diff_fits(), muted(), pane_lines(), pane_lines_add_a_note_only_when_something_is_cut(), red_spans() (+2 more)

### Community 12 - "Core Commands and Verdicts"
Cohesion: 0.12
Nodes (7): RunnerCommand, RunCode, Verdict, CompilationError, RuntimeError, Success, TimeLimitExceeded

### Community 13 - "Filesystem Operations"
Cohesion: 0.18
Nodes (4): FsOps, migrate_legacy_root(), short_hash(), source_url()

### Community 14 - "Problem Config and Caching"
Cohesion: 0.17
Nodes (13): ActiveProblem, ReceivingTestCase, UserConfig, artifact_path(), cache_key(), CacheKey, Compile, compile() (+5 more)

### Community 15 - "Language Compile Matrix"
Cohesion: 0.14
Nodes (7): cpp_compiles_without_implicit_flags(), Language, .ALL, Cpp, Java, Python, std_from_flags()

### Community 16 - "TestCase Preview Tests"
Cohesion: 0.19
Nodes (8): a_single_enter_grows_the_input_pane(), delete_focus_turns_every_pane_border_red(), giant_output_is_sized_to_the_capped_pane(), height_tracks_content_and_collapse(), huge_wa_output_is_cut_on_arrival_and_never_diffed(), retain_output_preview(), screen_of(), wa_previews_over_the_diff_budget_are_plain_not_diffed()

### Community 18 - "Submit Config and Tracking"
Cohesion: 0.23
Nodes (12): SubmitConfig, SubmitterCommand, SubmitCode, describe(), plan(), safari_installed(), strip_metadata(), strips_url_and_test_blocks() (+4 more)

### Community 19 - "Error Types and Formatting"
Cohesion: 0.18
Nodes (5): AppError, FileNotFound, Io, ParseError, UnsupportedPlatform

### Community 20 - "Boilerplate and Test Database"
Cohesion: 0.18
Nodes (6): ~/.zch/boilerplate.cpp, Validated Compile-Flag Tags, Test Database (~/.zch/.tc/), DEFAULT_CPP_BOILERPLATE, DEFAULT_JAVA_BOILERPLATE, DEFAULT_PY_BOILERPLATE

### Community 22 - "Case Verdict Styles"
Cohesion: 0.18
Nodes (7): CaseVerdict, AC, CE, Pending, RE, Tle, WA

### Community 23 - "Helper Command Protocol"
Cohesion: 0.18
Nodes (10): HelperCommand, CaseResult, Command, Error, New, Quit, RunFinished, RunPhase (+2 more)

### Community 24 - "Pane Clipping and Rendering"
Cohesion: 0.31
Nodes (4): clip_line(), clip_line_marks_what_does_not_fit(), inner_width(), offset_rect()

### Community 25 - "Case Focus and Frames"
Cohesion: 0.22
Nodes (5): tall_cases_scroll_through_their_full_height(), CaseFocus, Delete, Edit, None

### Community 26 - "README Product Overview"
Cohesion: 0.25
Nodes (8): Codeforces, Competitive Companion Extension, Homebrew / Installer Distribution, Live TUI Dashboard, ZCH, zch send Subcommand, Zed Editor, Zed Tasks / Command Palette Bridge

### Community 27 - "Input Mode State Machine"
Cohesion: 0.25
Nodes (8): InputMode, Config, ConfirmDelete, ConfirmSubmit, DeleteSelect, Directory, Editing, Normal

### Community 28 - "Release Workflow Jobs"
Cohesion: 0.67
Nodes (7): Release Workflow: announce job, Release Workflow: build-global-artifacts job, Release Workflow: build-local-artifacts job, cargo-dist (dist) Release Tooling v0.32.0, Release Workflow: host job (upload and GitHub Release), Release Workflow: plan job, Release Workflow: publish-homebrew-formula job

### Community 29 - "Pane Kinds and Hit Testing"
Cohesion: 0.40
Nodes (4): PaneKind, Expected, Got, Input

### Community 30 - "Submission Outcome Classification"
Cohesion: 0.40
Nodes (5): classify(), Outcome, Accepted, Failed, Rejected

### Community 31 - "Active Box Selection"
Cohesion: 0.50
Nodes (4): ActiveBox, Expected, Input, None

## Knowledge Gaps
- **68 isolated node(s):** `zch`, `Off`, `Select`, `Confirm`, `Input` (+63 more)
  These have ≤1 connection - possible missing edges or undocumented components. (Counts symbols only; 162 node(s) total have ≤1 connection when file, concept and rationale nodes are included.)
- **5 thin communities (<3 nodes) omitted from report** — run `graphify query` to explore isolated nodes.

## Suggested Questions
_Questions this graph is uniquely positioned to answer:_

- **Why does `Helper` connect `Helper UI and Tests` to `Config Menu and Flags`, `Directory Terminal Console`, `Four-Actor App Architecture`, `Filesystem Operations`, `Problem Config and Caching`, `TestCase List Navigation`, `Submit Config and Tracking`, `Input Mode State Machine`, `Pane Kinds and Hit Testing`?**
  _High betweenness centrality (0.102) - this node is a cross-community bridge._
- **What connects `zch`, `Off`, `Select` to the rest of the system?**
  _68 weakly-connected nodes found - possible documentation gaps or missing edges._
- **Should `Helper UI and Tests` be split into smaller, more focused modules?**
  _Cohesion score 0.0698289543290425 - nodes in this community are weakly interconnected._
- **Why does `TestCase` connect `TestCase Layout and Scrolling` to `Helper UI and Tests`, `Theme and Visual State`, `Got Diff Highlighting`, `Output Capping Utilities`, `TestCase Preview Tests`, `TestCase List Navigation`, `List Events and Verdict Chips`, `Case Verdict Styles`, `Pane Clipping and Rendering`?**
  _High betweenness centrality (0.083) - this node is a cross-community bridge._
- **Should `Config Menu and Flags` be split into smaller, more focused modules?**
  _Cohesion score 0.11818181818181818 - nodes in this community are weakly interconnected._
- **Why does `Language` connect `Language Compile Matrix` to `Helper UI and Tests`, `Config Menu and Flags`, `Runner Cache and Compilation`, `Core Commands and Verdicts`, `Filesystem Operations`, `Problem Config and Caching`, `Submit Config and Tracking`, `Boilerplate and Test Database`?**
  _High betweenness centrality (0.053) - this node is a cross-community bridge._
- **Should `Directory Terminal Console` be split into smaller, more focused modules?**
  _Cohesion score 0.1106612685560054 - nodes in this community are weakly interconnected._