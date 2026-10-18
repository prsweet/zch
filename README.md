<div align="center">
  <h1>🚀 ZCH </h1>
  <p><strong>A blazing-fast TUI dashboard for competitive programming in the <a href="https://zed.dev/">Zed</a> editor on macOS.</strong></p>

  [![macOS](https://img.shields.io/badge/os-macOS-black?style=flat-square&logo=apple)](#)
  [![Rust](https://img.shields.io/badge/rust-2024-orange?style=flat-square&logo=rust)](#)
  [![Zed](https://img.shields.io/badge/Editor-Zed-blue?style=flat-square)](#)
  [![License](https://img.shields.io/badge/license-MIT-green?style=flat-square)](#)
</div>

<br>

A single Rust binary that runs everything locally. It intercepts problems from the [Competitive Companion](https://github.com/jmerle/competitive-companion) browser extension, writes your template with the URL header, stores sample tests in a hidden database, compiles and runs every test with precise timings, and submits to **Codeforces** and **AtCoder** via Safari automation while streaming the live verdict — all from one TUI.

The original Python implementation is kept at `main.py` for reference.

---

## ✨ Features

* 🖥️ **Live TUI dashboard** — working directory bar, message console, per-case result cards, and a centered footer that always shows every key.
* ✔️ **Instant verdict cards** — each test case gets a chip (`✔ AC · 4ms`, `✘ WA`, `⏱ TLE`, `✘ RE`, `✘ CE`) and auto-collapses on AC. On WA the **Got** box shows exactly what your program printed, with the characters that sit somewhere other than the expected character at that position marked in red — missing characters are never invented.
* 📥 **Auto-parses problems** — Competitive Companion payloads are captured on `127.0.0.1:10043`; the file is created from your boilerplate with a `// URL:` header and opened in Zed automatically.
* 🧱 **Smart boilerplate injection** — your template from `~/.zch/boilerplate.cpp` (falls back to the repo default). No test comments in your code; tests live in `~/.zch/.tc/<Problem>.json`.
* ⚡ **Blazing fast execution** — compiles once, replays all cases with 1ms polling, per-case timing, and hard TLE kills. Binaries older than 24h are pruned automatically.
* 🤖 **One-key submissions** — a confirmation modal shows every detail (platform, problem ID, URL, language, browser, local tests) before submitting to Codeforces / AtCoder via Safari automation. Submissions run fully async — several can be in flight at once, each tracked in its own **Submissions bar** row (`⠹ submitting → ✔ ACCEPTED`) with an animated spinner, so run output in the console never overwrites them; the bar shows the latest 4 and scrolls internally with the mouse wheel through the last 20; `zch submit` does the same in its own terminal tab. The submission's Safari tab is tagged and tracked by URL (Safari exposes no tab id), so opening or closing other tabs never misreports it. If you close the Safari tab mid-flight, the submission is marked **INTERRUPTED** (after a ~20s grace period that tolerates transient hiccups) instead of hanging, and a verdict that never arrives ends as **TIMED OUT** rather than a bare `WAIT`. If a CAPTCHA blocks the page, the submission's Safari window is raised automatically with a notification alert so you can solve it in place.
* 🎛️ **Command palette from Zed** — Zed tasks drive the running TUI over HTTP (`send r`, `send s`, …) with the active `${ZED_FILE}`.
* 📁 **Live directory switching** — press `d` for the console: `cd <path>`, `ls`, `pwd`, with TAB path completion that walks nested paths level by level (`cd cp/q` ⇥ `cp/questions/`). New problems are saved wherever you point it.
* 🧩 **Multi-language** — C++17/20/23 (default C++23), Python 3, Java. Language + extra compile flags are configured in the built-in config menu (`c`) and persisted to `~/.zch/config.json`.
* 🧱 **Zero sandboxing limits** — native binary, no WASM/network restrictions.

---

## 📋 Prerequisites

- **macOS** *(relies on native Apple Events for Safari automation)*
- **Rust toolchain** (`cargo`) *(optional — only if building from source)*
- A **Nerd Font** in your terminal (e.g. JetBrainsMono Nerd Font) so the file/folder icons render the same on every machine.
- **Safari** with **"Allow JavaScript from Apple Events"** enabled:
  - `Safari` → `Settings` → `Advanced` → check `"Show features for web developers"`
  - `Develop` → `Allow JavaScript from Apple Events`
- **[Zed Code Editor](https://zed.dev/)**
- **C++ compiler** (`g++` via Homebrew, or Apple Clang)
- **[Competitive Companion](https://github.com/jmerle/competitive-companion)** browser extension.

> **💡 Why Safari only?**
> Submission automation requires the browser to run JavaScript in the background without stealing focus. **Only Safari supports this natively** via AppleScript. Other browsers would foreground the window mid-solve.

---

## 🛠️ Installation

### 1. Install

**Homebrew (recommended):**

```bash
brew install prsweet/tap/zch
```

**Or the one-liner** — no Rust toolchain needed, installs to `~/.local/bin`:

```bash
curl -fsSL https://github.com/prsweet/zch/releases/latest/download/zch-installer.sh | sh
```

**From source** (requires the Rust toolchain):

```bash
git clone https://github.com/prsweet/zch.git
cd zch
cargo build --release
cp target/release/zch ~/.local/bin/
```

### 2. Add your boilerplate (optional)

```bash
cp boilerplate.cpp ~/.zch/boilerplate.cpp   # then edit to taste
```

`~/.zch/` holds everything: `boilerplate.cpp`, `config.json`, `.tc/` (test cases), `.bin/` (compiled binaries). Anything from the old `~/.zed-cp-helper/` folder is migrated automatically on first run.

### 3. Setup Zed tasks

Append to `~/.config/zed/tasks.json` (press `cmd+shift+R` to spawn tasks):

<details>
<summary><strong>Click to view the <code>tasks.json</code> configuration</strong></summary>

```json
[
  {
    "label": "CP: Start Listener (Current Folder)",
    "command": "zch",
    "use_new_terminal": true,
    "allow_concurrent_runs": false,
    "hide": "never",
    "reveal_target": "center"
  },
  {
    "label": "CP: Run Tests",
    "command": "zch send r \"${ZED_FILE}\"",
    "use_new_terminal": false,
    "allow_concurrent_runs": false,
    "reveal": "never",
    "hide": "always"
  },
  {
    "label": "CP: Submit to Codeforces / AtCoder",
    "command": "zch submit \"${ZED_FILE}\"",
    "use_new_terminal": true,
    "allow_concurrent_runs": true,
    "reveal": "always",
    "hide": "never"
  },
  {
    "label": "CP: Add Custom Test Case",
    "command": "zch send a \"${ZED_FILE}\"",
    "use_new_terminal": false,
    "allow_concurrent_runs": false,
    "reveal": "never",
    "hide": "always"
  },
  {
    "label": "CP: Edit Test Cases",
    "command": "zch send e \"${ZED_FILE}\"",
    "use_new_terminal": false,
    "allow_concurrent_runs": false,
    "reveal": "never",
    "hide": "always"
  }
]
```
</details>

### 4. Optimize your keymap

Add to `~/.config/zed/keymap.json`:

```json
[
  {
    "context": "Editor",
    "bindings": {
      "cmd-'": ["task::Spawn", { "task_name": "CP: Run Tests" }],
      "cmd-enter": ["task::Spawn", { "task_name": "CP: Submit to Codeforces / AtCoder" }],
      "cmd-alt-a": ["task::Spawn", { "task_name": "CP: Add Custom Test Case" }],
      "cmd-alt-e": ["task::Spawn", { "task_name": "CP: Edit Test Cases" }]
    }
  },
  {
    "context": "Workspace",
    "bindings": {
      "cmd-r": ["task::Spawn", { "task_name": "CP: Start Listener (Current Folder)" }]
    }
  }
]
```

---

## 🎮 Usage

### 1. Start the TUI

Press **`cmd-r`** inside your problems folder (or run `zch` in a terminal). Drag the tab to the right side of your screen for a split-editor layout. The dashboard listens on `127.0.0.1:10043`.

### 2. Receive a problem

Click the **Competitive Companion** extension on any Codeforces / AtCoder problem page. The file is created in the current directory, saved to the test database, and opened in Zed automatically.

### 3. Test & submit from the TUI

| Key | Action |
| --- | --- |
| `r` | compile & run all test cases |
| `a` | add a custom test case |
| `e` | edit existing test cases |
| `s` | submit to Codeforces / AtCoder (confirmation modal → Enter submits / Backspace cancels) |
| `d` | console: `cd <path>` · `ls` · `pwd` (TAB completes paths) |
| `c` | config: language + extra compile flags |
| `q` | quit |

While editing, `Tab` switches between **Input** / **Expected** (and wraps to the previous/next case with `Shift+Tab`), `Esc` saves.

### 4. Test & submit from Zed

* **Run (`cmd-'`)** — asks the running TUI to load `${ZED_FILE}` and run it.
* **Submit (`cmd-enter`)** — opens a dedicated terminal tab that shows every submission detail (platform, problem ID, URL, language, browser, local tests) and waits for **`Enter`** to submit or **`Backspace`** to cancel. The verdict (and any CAPTCHA warning) streams into that tab.
* **Add / Edit `(cmd-alt-a / cmd-alt-e)`** — jump straight into editing the case list for the current file.

The `send` subcommand talks to the TUI over HTTP, while `submit` runs standalone in its own terminal:

```bash
zch send r /path/to/Problem.cpp   # run
zch send e /path/to/Problem.cpp   # edit cases
zch send a /path/to/Problem.cpp   # add case
zch send s /path/to/Problem.cpp   # submit (confirmation modal inside the TUI)
zch submit /path/to/Problem.cpp   # submit here (Enter/Backspace prompt, -y to skip)
```

Set `ZCH_PORT` to run on a different port than `10043`.

---

## 🛡️ Dealing with CAPTCHAs

Platforms like AtCoder and Codeforces occasionally use invisible CAPTCHAs. When automation hits one, the TUI alerts you (`⚠ CAPTCHA — solve it, submission continues automatically`) and Safari foregrounds itself. Solve it, and the live verdict polling resumes on its own.

---

## 💡 Pro-tips for maximum performance

### 1. Drop execution latency to 3-4ms ⚡
macOS security scans (`syspolicyd`) add ~200ms of lag when running newly compiled binaries. Fix:
1. **System Settings** → **Privacy & Security** → **Developer Tools**: add **Zed** and your terminal emulator, toggle them **ON**.
2. Add the same under **Full Disk Access**.

![macOS Settings](macos_developer_tools_settings.png)

### 2. Ultra-fast precompiled headers (PCH) 🔥
Compiling `#include <bits/stdc++.h>` costs 1.5s–3.0s per compile. A PCH drops this **under 0.20s**.

**For Clang (macOS default):**
```bash
clang++ -std=c++23 -O2 -x c++-header /usr/local/include/bits/stdc++.h -o /usr/local/include/bits/stdc++.h.pch
```
Then open the config menu (`c`) and add to **extra compile flags**:
```
-include-pch /usr/local/include/bits/stdc++.h.pch
```
The flag is appended to `g++ -std=c++23 -O2 -Wall -Wextra …` for every compile.

### 3. Fast policy-based data structures (PBDS) on Clang 🌲
Apple Clang doesn't ship bits/stdc++.h or PBDS. The [cp_with_clang](https://github.com/prsweet/cp_with_clang.git) helper repo sets up both for Apple Clang.

### 4. Sync templates between Zed and VS Code
Keep one snippet file via a symlink:
```bash
ln -sf "$HOME/Library/Application Support/Code/User/snippets/cpp.json" ~/.config/zed/snippets/c++.json
```
Name your main snippet exactly `boilerplate`.

---

## 🧱 Architecture

```
src/
  main.rs               — event loop: one channel, four actors
  helper.rs             — UI state machine (input modes, commands, verdicts)
  components/           — ratatui widgets
    dir_terminal.rs     — working-directory console (cd/ls/pwd + completion)
    tc_list.rs          — scrollable test case list with verdict chips
    testcase.rs         — one case: input / expected / got, diff highlighting
    config.rs           — language + compile flags popup
    splash.rs           — empty-state screen
  core/
    receiver.rs         — Competitive Companion HTTP listener (:10043) + command API
    runner.rs           — compile + run all cases (timings, TLE kills)
    submitter.rs        — Safari AppleScript automation + platform detection
    fs_ops.rs           — boilerplate, test DB, config, binary pruning
  error.rs              — shared error type
```

Everything runs on one channel: the UI thread owns all state, and the receiver / runner / submitter workers only exchange `PassingCommand` envelopes with it — no worker holds a handle to another.
