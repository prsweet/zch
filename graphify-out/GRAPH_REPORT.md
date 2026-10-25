# Graph Report - zch  (2026-10-25)

## Corpus Check
- cluster-only mode — file stats not available

## Summary
- 702 nodes · 1816 edges · 37 communities (33 shown, 4 thin omitted)
- Extraction: 98% EXTRACTED · 2% INFERRED · 0% AMBIGUOUS · INFERRED: 39 edges (avg confidence: 0.86)
- Token cost: 0 input · 0 output

## Graph Freshness
- Built from commit: `b637f386`
- Run `git rev-parse HEAD` and compare to check if the graph is stale.
- Run `graphify update .` after code changes (no API cost).

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
- Precompiled Headers (PCH)
- zch
- ZCH TUI Screenshot Showing WA Verdict (1ms)

## God Nodes (most connected - your core abstractions)
1. `Helper` - 62 edges
2. `TestCase` - 41 edges
3. `temp_root()` - 37 edges
4. `sample_problem()` - 33 edges
5. `key()` - 28 edges
6. `FsOps` - 25 edges
7. `ConfigMenu` - 24 edges
8. `Language` - 24 edges
9. `PassingCommand` - 22 edges
10. `TestCaseList` - 21 edges

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

## Hyperedges (group relationships)
- **Single-Channel Actor Architecture** — src_main, src_helper, src_core_receiver, src_core_runner, src_core_submitter, readme_passingcommand [EXTRACTED 1.00]
- **Codeforces Submission Flow** — src_core_submitter, readme_safari_automation, readme_codeforces, readme_captcha_handling, readme_submissions_bar [EXTRACTED 1.00]
- **Ratatui UI Components** — src_components_dir_terminal, src_components_tc_list, src_components_testcase, src_components_config, src_components_splash [EXTRACTED 1.00]
- **cargo-dist Release Pipeline** — github_workflows_release_plan, github_workflows_release_build_local_artifacts, github_workflows_release_build_global_artifacts, github_workflows_release_host, github_workflows_release_publish_homebrew_formula, github_workflows_release_announce, github_workflows_release_dist_tool [EXTRACTED 1.00]

## Communities (37 total, 4 thin omitted)

### Community 0 - "Helper UI and Tests"
Cohesion: 0.11
Nodes (53): block(), ToSubmitter, add_edit_and_persist_custom_case(), anonymous_problems_with_the_same_name_do_not_overwrite(), clicking_a_pane_in_edit_mode_moves_the_focus(), config_menu_cycles_language_and_saves(), delete_selects_and_asks_for_confirmation(), DialogContent (+45 more)

### Community 1 - "Config Menu and Flags"
Cohesion: 0.06
Nodes (18): PassingCommand, ToRunner, Helper, InputMode, Config, ConfirmDelete, ConfirmSubmit, DeleteSelect (+10 more)

### Community 2 - "Directory Terminal Console"
Cohesion: 0.11
Nodes (28): accepts_a_value_after_a_flag_that_takes_one(), adds_flag_tags_and_derives_the_cf_language_id(), backspace_only_edits_the_draft(), ConfigMenu, DeleteStage, Confirm, Off, Select (+20 more)

### Community 3 - "Runner Cache and Compilation"
Cohesion: 0.06
Nodes (24): COLLAPSED_HEIGHT, DIALOG_SAFE_MAX_BYTES, DIALOG_SAFE_MAX_LINES, diff_ignores_trailing_whitespace(), diff_is_positional_across_lines(), diff_marks_got_chars_out_of_place(), diff_marks_the_shifted_tail_after_a_missing_newline(), DIFF_MAX_CHARS (+16 more)

### Community 4 - "Codeforces Submission Automation"
Cohesion: 0.11
Nodes (15): common_extension(), completes_common_prefix_and_lists_ambiguous_matches(), completes_from_home_prefix(), completes_nested_directories_level_by_level(), Completion, DirTerminal, HISTORY_LIMIT, key() (+7 more)

### Community 5 - "Theme and Visual State"
Cohesion: 0.11
Nodes (20): a_small_drag_on_a_long_scrollbar_is_a_small_step(), ACCENT, BORDER, CYAN, dragging_a_scrollbar_thumb_moves_relative_to_the_grab_point(), FILE, FOLDER, GREEN (+12 more)

### Community 6 - "Command Receiver and Actions"
Cohesion: 0.11
Nodes (20): CAPTCHA Foreground Handling, Safari AppleScript Submission Automation, Live Verdict Polling / Submissions Bar, APP_NAME, APPLESCRIPT_TEMPLATE, build_applescript(), CF_FILL_JS, CF_RESULT_JS (+12 more)

### Community 7 - "Got Diff Highlighting"
Cohesion: 0.11
Nodes (13): PassingCommand Envelope, RunAction, Add, Edit, Run, Submit, COMMAND_TIMEOUT, handle_command_url() (+5 more)

### Community 8 - "Four-Actor App Architecture"
Cohesion: 0.11
Nodes (9): ActiveProblem, legacy_fallback_flags_become_the_current_language_flags(), normalize_prefers_the_selected_language_flags(), ReceivingTestCase, RunnerCommand, RunCode, std_from_flag(), std_from_flags() (+1 more)

### Community 10 - "Splash Screen Rendering"
Cohesion: 0.13
Nodes (7): cell_t(), gradient(), LOGO, logo_lines(), render(), splash_renders_on_narrow_and_wide_terminals(), STOPS

### Community 11 - "Output Capping Utilities"
Cohesion: 0.13
Nodes (8): ToHelper, spawn_submitter(), main(), open_in_zed(), print_help(), run_app(), run_tui(), send_command()

### Community 12 - "Core Commands and Verdicts"
Cohesion: 0.16
Nodes (12): Capped, capped_reports_hidden_lines_and_cut_characters(), contents(), extra_style(), highlight_got_full(), highlight_got_inner(), muted(), pane_lines() (+4 more)

### Community 13 - "Filesystem Operations"
Cohesion: 0.15
Nodes (12): macOS syspolicyd Startup Latency, artifact_stamp(), ArtifactStamp, cancellation_kills_a_hung_case(), collect_run(), compile_cache_key_tracks_source_flags_and_language(), java_recompiles_after_source_changes(), MAX_CACHE_ENTRIES (+4 more)

### Community 14 - "Problem Config and Caching"
Cohesion: 0.15
Nodes (15): UserConfig, cache_key(), CacheEntry, CacheKey, command(), Compile, compile(), Cancelled (+7 more)

### Community 15 - "Language Compile Matrix"
Cohesion: 0.18
Nodes (13): a_single_enter_grows_the_input_pane(), case_title_is_plain_before_a_run(), delete_focus_turns_every_pane_border_red(), editing_uses_accent_focus(), empty_or_stderr_only_huge_output_does_not_warn(), height_tracks_content_and_collapse(), huge_wa_output_is_cut_on_arrival_and_never_diffed(), one_e7_output_stays_in_memory_with_a_full_dialog_diff() (+5 more)

### Community 16 - "TestCase Preview Tests"
Cohesion: 0.17
Nodes (4): FsOps, migrate_legacy_root(), short_hash(), source_url()

### Community 17 - "TestCase List Navigation"
Cohesion: 0.15
Nodes (4): Verdict Chips (AC/WA/TLE/RE/CE), mouse(), scrollbar_drag_scrolls_the_case_list(), tall_cases_scroll_through_their_full_height()

### Community 18 - "Submit Config and Tracking"
Cohesion: 0.12
Nodes (15): HelperCommand, CaseResult, Command, Error, New, Quit, RunFinished, RunPhase (+7 more)

### Community 20 - "Boilerplate and Test Database"
Cohesion: 0.26
Nodes (4): clip_line(), clip_line_marks_what_does_not_fit(), inner_width(), offset_rect()

### Community 21 - "List Events and Verdict Chips"
Cohesion: 0.17
Nodes (6): cpp_compiles_without_implicit_flags(), Language, .ALL, Cpp, Java, Python

### Community 22 - "Case Verdict Styles"
Cohesion: 0.23
Nodes (12): SubmitConfig, SubmitterCommand, SubmitCode, describe(), plan(), safari_installed(), strip_metadata(), strips_url_and_test_blocks() (+4 more)

### Community 23 - "Helper Command Protocol"
Cohesion: 0.18
Nodes (5): AppError, FileNotFound, Io, ParseError, UnsupportedPlatform

### Community 24 - "Pane Clipping and Rendering"
Cohesion: 0.18
Nodes (6): ~/.zch/boilerplate.cpp, Validated Compile-Flag Tags, Test Database (~/.zch/.tc/), DEFAULT_CPP_BOILERPLATE, DEFAULT_JAVA_BOILERPLATE, DEFAULT_PY_BOILERPLATE

### Community 25 - "Case Focus and Frames"
Cohesion: 0.20
Nodes (7): CaseVerdict, AC, CE, Pending, RE, Tle, WA

### Community 26 - "README Product Overview"
Cohesion: 0.32
Nodes (4): GotFile, PageLine, read_line_limited(), skip_line()

### Community 27 - "Input Mode State Machine"
Cohesion: 0.25
Nodes (8): Codeforces, Competitive Companion Extension, Homebrew / Installer Distribution, Live TUI Dashboard, ZCH, zch send Subcommand, Zed Editor, Zed Tasks / Command Palette Bridge

### Community 28 - "Release Workflow Jobs"
Cohesion: 0.67
Nodes (7): Release Workflow: announce job, Release Workflow: build-global-artifacts job, Release Workflow: build-local-artifacts job, cargo-dist (dist) Release Tooling v0.32.0, Release Workflow: host job (upload and GitHub Release), Release Workflow: plan job, Release Workflow: publish-homebrew-formula job

### Community 29 - "Pane Kinds and Hit Testing"
Cohesion: 0.40
Nodes (4): PaneKind, Expected, Got, Input

### Community 30 - "Submission Outcome Classification"
Cohesion: 0.40
Nodes (3): diff_fits(), has_stdout(), split_stderr()

### Community 31 - "Active Box Selection"
Cohesion: 0.40
Nodes (5): classify(), Outcome, Accepted, Failed, Rejected

### Community 32 - "ZCH Project"
Cohesion: 0.50
Nodes (4): ActiveBox, Expected, Input, None

### Community 33 - "WA Verdict Screenshot"
Cohesion: 0.50
Nodes (4): CaseFocus, Delete, Edit, None

### Community 34 - "Precompiled Headers (PCH)"
Cohesion: 0.67
Nodes (3): Content-Hash Compile Cache, Precompiled Headers (PCH), std-with-pbds Helper Repo

## Knowledge Gaps
- **69 isolated node(s):** `Paged`, `Config`, `ConfirmDelete`, `ConfirmSubmit`, `DeleteSelect` (+64 more)
  These have ≤1 connection - possible missing edges or undocumented components. (Counts symbols only; 173 node(s) total have ≤1 connection when file, concept and rationale nodes are included.)
- **4 thin communities (<3 nodes) omitted from report** — run `graphify query` to explore isolated nodes.

## Suggested Questions
_Questions this graph is uniquely positioned to answer:_

- **Why does `Helper` connect `Config Menu and Flags` to `Helper UI and Tests`, `Directory Terminal Console`, `Codeforces Submission Automation`, `Theme and Visual State`, `Four-Actor App Architecture`, `Output Capping Utilities`, `TestCase Preview Tests`, `Error Types and Formatting`, `Case Verdict Styles`, `Pane Kinds and Hit Testing`?**
  _High betweenness centrality (0.102) - this node is a cross-community bridge._
- **What connects `Paged`, `Config`, `ConfirmDelete` to the rest of the system?**
  _69 weakly-connected nodes found - possible documentation gaps or missing edges._
- **Should `Helper UI and Tests` be split into smaller, more focused modules?**
  _Cohesion score 0.11230120090879585 - nodes in this community are weakly interconnected._
- **Why does `TestCase` connect `TestCase Layout and Scrolling` to `Helper UI and Tests`, `Runner Cache and Compilation`, `Theme and Visual State`, `Core Commands and Verdicts`, `Language Compile Matrix`, `TestCase List Navigation`, `Error Types and Formatting`, `Boilerplate and Test Database`, `Case Focus and Frames`, `README Product Overview`, `Submission Outcome Classification`?**
  _High betweenness centrality (0.090) - this node is a cross-community bridge._
- **Should `Config Menu and Flags` be split into smaller, more focused modules?**
  _Cohesion score 0.06038961038961039 - nodes in this community are weakly interconnected._
- **Why does `Language` connect `List Events and Verdict Chips` to `Helper UI and Tests`, `Directory Terminal Console`, `Four-Actor App Architecture`, `Filesystem Operations`, `Problem Config and Caching`, `TestCase Preview Tests`, `Case Verdict Styles`, `Pane Clipping and Rendering`?**
  _High betweenness centrality (0.047) - this node is a cross-community bridge._
- **Should `Directory Terminal Console` be split into smaller, more focused modules?**
  _Cohesion score 0.11058823529411765 - nodes in this community are weakly interconnected._