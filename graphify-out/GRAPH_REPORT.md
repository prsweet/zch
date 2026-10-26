# Graph Report - zch  (2026-10-26)

## Corpus Check
- Corpus is ~34,486 words - fits in a single context window. You may not need a graph.

## Summary
- 821 nodes · 2076 edges · 40 communities (37 shown, 3 thin omitted)
- Extraction: 99% EXTRACTED · 1% INFERRED · 0% AMBIGUOUS · INFERRED: 31 edges (avg confidence: 0.85)
- Token cost: 0 input · 0 output

## Community Hubs (Navigation)
- Helper UI tests
- Ratatui widget imports
- Directory console
- Helper shell and dialogs
- Config flag menu
- Test case rendering
- Theme and scrollbars
- Runner process channel
- README product concepts
- Codeforces submitter
- Test case layout
- Got output highlighting
- Companion receiver
- Verdict chips
- Language config model
- Submission flow
- App entrypoint
- Output paging
- Problem file storage
- Compile cache
- Boilerplate and errors
- Release workflow jobs
- Release workflow steps
- Language compile commands
- Cross-thread commands
- Release publish outputs
- Release plan env
- Local artifact matrix
- Checkout action inputs
- Homebrew formula publish
- Paged got file
- Input modes
- Release announcement
- README mechanisms
- Submitter channel
- Submission outcome
- CI triggers
- User config persistence
- Package name
- TUI screenshot

## God Nodes (most connected - your core abstractions)
1. `Helper` - 63 edges
2. `temp_root()` - 44 edges
3. `TestCase` - 41 edges
4. `sample_problem()` - 38 edges
5. `key()` - 32 edges
6. `DirTerminal` - 30 edges
7. `FsOps` - 25 edges
8. `ConfigMenu` - 24 edges
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

## Communities (40 total, 3 thin omitted)

### Community 0 - "Helper UI tests"
Cohesion: 0.14
Nodes (55): ToSubmitter, add_edit_and_persist_custom_case(), adding_a_case_keeps_tab_inside_that_case(), anonymous_problems_with_the_same_name_do_not_overwrite(), clicking_a_pane_in_edit_mode_moves_the_focus(), config_menu_cycles_language_and_saves(), console_does_not_repeat_footer_mode_hints(), delete_selects_and_asks_for_confirmation() (+47 more)

### Community 1 - "Ratatui widget imports"
Cohesion: 0.05
Nodes (25): cell_t(), gradient(), LOGO, logo_lines(), render(), splash_renders_on_narrow_and_wide_terminals(), STOPS, list_only_reserves_a_column_when_the_scrollbar_is_needed() (+17 more)

### Community 2 - "Directory console"
Cohesion: 0.08
Nodes (24): common_extension(), completes_common_prefix_and_lists_ambiguous_matches(), completes_from_home_prefix(), completes_nested_directories_level_by_level(), Completion, CONSOLE_HEIGHT, console_height_is_one_line_and_stable_for_wrapped_messages(), console_renders_the_active_message_on_one_line() (+16 more)

### Community 3 - "Helper shell and dialogs"
Cohesion: 0.07
Nodes (15): block(), scrollbar(), ToRunner, DialogContent, Lines, Paged, dim_area(), Helper (+7 more)

### Community 4 - "Config flag menu"
Cohesion: 0.11
Nodes (28): accepts_a_value_after_a_flag_that_takes_one(), adds_flag_tags_and_derives_the_cf_language_id(), backspace_only_edits_the_draft(), ConfigMenu, DeleteStage, Confirm, Off, Select (+20 more)

### Community 5 - "Test case rendering"
Cohesion: 0.06
Nodes (26): COLLAPSED_HEIGHT, DIALOG_SAFE_MAX_BYTES, DIALOG_SAFE_MAX_LINES, diff_does_not_shift_identical_lines(), diff_ignores_trailing_whitespace(), diff_is_positional_across_lines(), diff_marks_got_chars_out_of_place(), diff_marks_the_shifted_tail_after_a_missing_newline() (+18 more)

### Community 6 - "Theme and scrollbars"
Cohesion: 0.11
Nodes (20): a_small_drag_on_a_long_scrollbar_is_a_small_step(), ACCENT, BORDER, CYAN, dragging_a_scrollbar_thumb_moves_relative_to_the_grab_point(), FILE, FOLDER, GREEN (+12 more)

### Community 7 - "Runner process channel"
Cohesion: 0.13
Nodes (19): PassingCommand, RunnerCommand, RunCode, artifact_stamp(), ArtifactStamp, CacheEntry, cancellation_kills_a_hung_case(), collect_run() (+11 more)

### Community 8 - "README product concepts"
Cohesion: 0.08
Nodes (27): CAPTCHA Handling, One-Key Codeforces Submission, Codeforces, Competitive Companion, Live Directory Console, Validated Compile-Flag Tags, fs_ops.rs Storage, helper.rs UI State Machine (+19 more)

### Community 9 - "Codeforces submitter"
Cohesion: 0.13
Nodes (17): APP_NAME, APPLESCRIPT_TEMPLATE, build_applescript(), CF_FILL_JS, CF_RESULT_JS, derives_tracking_urls(), detect_platform(), detects_codeforces_contest() (+9 more)

### Community 10 - "Test case layout"
Cohesion: 0.13
Nodes (4): a_single_enter_grows_the_input_pane(), diff_fits(), editing_caps_a_tall_case_to_the_viewport(), TestCase

### Community 11 - "Got output highlighting"
Cohesion: 0.14
Nodes (15): Capped, capped_reports_hidden_lines_and_cut_characters(), contents(), extra_style(), has_stdout(), highlight_got_full(), highlight_got_inner(), line_from_runs() (+7 more)

### Community 12 - "Companion receiver"
Cohesion: 0.11
Nodes (12): RunAction, Add, Edit, Run, Submit, COMMAND_TIMEOUT, handle_command_url(), listen_port() (+4 more)

### Community 13 - "Verdict chips"
Cohesion: 0.12
Nodes (11): CaseVerdict, AC, CE, Pending, RE, Tle, WA, clip_line() (+3 more)

### Community 14 - "Language config model"
Cohesion: 0.11
Nodes (9): legacy_fallback_flags_become_the_current_language_flags(), normalize_prefers_the_selected_language_flags(), std_from_flag(), std_from_flags(), Verdict, CompilationError, RuntimeError, Success (+1 more)

### Community 15 - "Submission flow"
Cohesion: 0.15
Nodes (15): SubmitConfig, describe(), plan(), safari_installed(), strip_metadata(), strips_url_and_test_blocks(), SubmissionInfo, submit() (+7 more)

### Community 16 - "App entrypoint"
Cohesion: 0.16
Nodes (6): main(), open_in_zed(), print_help(), run_app(), run_tui(), send_command()

### Community 17 - "Output paging"
Cohesion: 0.19
Nodes (12): case_title_is_plain_before_a_run(), delete_focus_turns_every_pane_border_red(), editing_uses_accent_focus(), empty_or_stderr_only_huge_output_does_not_warn(), height_tracks_content_and_collapse(), huge_wa_output_is_cut_on_arrival_and_never_diffed(), one_e7_output_stays_in_memory_with_a_full_dialog_diff(), output_over_the_line_budget_is_paged_instead_of_dropped() (+4 more)

### Community 18 - "Problem file storage"
Cohesion: 0.20
Nodes (3): FsOps, short_hash(), source_url()

### Community 19 - "Compile cache"
Cohesion: 0.21
Nodes (12): ActiveProblem, ReceivingTestCase, artifact_path(), cache_key(), CacheKey, Compile, compile(), compile_cache_key_tracks_source_flags_and_language() (+4 more)

### Community 20 - "Boilerplate and errors"
Cohesion: 0.17
Nodes (4): DEFAULT_CPP_BOILERPLATE, DEFAULT_JAVA_BOILERPLATE, DEFAULT_PY_BOILERPLATE, migrate_legacy_root()

### Community 21 - "Release workflow jobs"
Cohesion: 0.17
Nodes (11): if, needs, runs-on, needs, runs-on, jobs, announce, build-global-artifacts (+3 more)

### Community 22 - "Release workflow steps"
Cohesion: 0.38
Nodes (12): steps, steps, steps, steps, steps, steps, id, if (+4 more)

### Community 23 - "Language compile commands"
Cohesion: 0.17
Nodes (6): cpp_compiles_without_implicit_flags(), Language, .ALL, Cpp, Java, Python

### Community 24 - "Cross-thread commands"
Cohesion: 0.18
Nodes (10): HelperCommand, CaseResult, Command, Error, New, Quit, RunFinished, RunPhase (+2 more)

### Community 25 - "Release publish outputs"
Cohesion: 0.20
Nodes (10): if, needs, outputs, runs-on, host, publishing, tag, tag-flag (+2 more)

### Community 26 - "Release plan env"
Cohesion: 0.25
Nodes (9): env, env, env, BUILD_MANIFEST_NAME, GH_TOKEN, env, plan, env (+1 more)

### Community 27 - "Local artifact matrix"
Cohesion: 0.22
Nodes (9): container, if, name, needs, runs-on, strategy, build-local-artifacts, fail-fast (+1 more)

### Community 28 - "Checkout action inputs"
Cohesion: 0.22
Nodes (9): with, merge-multiple, name, path, pattern, persist-credentials, repository, submodules (+1 more)

### Community 29 - "Homebrew formula publish"
Cohesion: 0.25
Nodes (8): GITHUB_EMAIL, GITHUB_USER, PLAN, publish-homebrew-formula, env, if, needs, runs-on

### Community 30 - "Paged got file"
Cohesion: 0.32
Nodes (4): GotFile, PageLine, read_line_limited(), skip_line()

### Community 31 - "Input modes"
Cohesion: 0.25
Nodes (8): InputMode, Config, ConfirmDelete, ConfirmSubmit, DeleteSelect, Directory, Editing, Normal

### Community 32 - "Release announcement"
Cohesion: 0.40
Nodes (5): ANNOUNCEMENT_BODY, ANNOUNCEMENT_TITLE, PRERELEASE_FLAG, RELEASE_COMMIT, env

### Community 33 - "README mechanisms"
Cohesion: 0.40
Nodes (4): Content-Hash Compile Cache, runner.rs Test Runner, sample.png 1ms Run Screenshot, In-Memory Smoke Case

### Community 34 - "Submitter channel"
Cohesion: 0.40
Nodes (4): ToHelper, SubmitterCommand, SubmitCode, spawn_submitter()

### Community 35 - "Submission outcome"
Cohesion: 0.40
Nodes (5): classify(), Outcome, Accepted, Failed, Rejected

### Community 36 - "CI triggers"
Cohesion: 0.50
Nodes (4): tags, True, pull_request, push

## Knowledge Gaps
- **79 isolated node(s):** `zch`, `Off`, `Select`, `Confirm`, `Hint` (+74 more)
  These have ≤1 connection - possible missing edges or undocumented components. (Counts symbols only; 222 node(s) total have ≤1 connection when file, concept and rationale nodes are included.)
- **3 thin communities (<3 nodes) omitted from report** — run `graphify query` to explore isolated nodes.

## Suggested Questions
_Questions this graph is uniquely positioned to answer:_

- **Why does `Helper` connect `Helper shell and dialogs` to `Helper UI tests`, `Ratatui widget imports`, `Directory console`, `Config flag menu`, `Theme and scrollbars`, `Submission flow`, `App entrypoint`, `Problem file storage`, `Compile cache`, `Input modes`?**
  _High betweenness centrality (0.087) - this node is a cross-community bridge._
- **What connects `zch`, `Off`, `Select` to the rest of the system?**
  _79 weakly-connected nodes found - possible documentation gaps or missing edges._
- **Should `Helper UI tests` be split into smaller, more focused modules?**
  _Cohesion score 0.13622526636225266 - nodes in this community are weakly interconnected._
- **Why does `TestCase` connect `Test case layout` to `Helper UI tests`, `Ratatui widget imports`, `Test case rendering`, `Theme and scrollbars`, `Got output highlighting`, `Verdict chips`, `Output paging`, `Paged got file`?**
  _High betweenness centrality (0.069) - this node is a cross-community bridge._
- **Should `Ratatui widget imports` be split into smaller, more focused modules?**
  _Cohesion score 0.05174825174825175 - nodes in this community are weakly interconnected._
- **Why does `DirTerminal` connect `Directory console` to `Helper UI tests`, `Helper shell and dialogs`?**
  _High betweenness centrality (0.054) - this node is a cross-community bridge._
- **Should `Directory console` be split into smaller, more focused modules?**
  _Cohesion score 0.08143839238498149 - nodes in this community are weakly interconnected._