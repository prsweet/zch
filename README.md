# 🚀 ZCH

**A blazing-fast TUI dashboard for competitive programming in the [Zed](https://zed.dev/) editor on macOS.**

[![macOS](https://img.shields.io/badge/os-macOS-black?style=flat-square&logo=apple)](#)
[![Rust](https://img.shields.io/badge/rust-2024-orange?style=flat-square&logo=rust)](#)
[![Zed](https://img.shields.io/badge/Editor-Zed-blue?style=flat-square)](#)

A single Rust binary that receives problems from the [Competitive Companion](https://github.com/jmerle/competitive-companion) extension, runs your samples with precise timings, and submits to **Codeforces** via Safari automation — all from one local TUI.

---

## ✨ Features

* 🖥️ **Live TUI dashboard** — directory bar, message console, verdict cards, and a footer that always shows every key.
* ✔️ **Instant verdicts** — each test case gets a chip (`✔ AC · 4ms`, `✘ WA`, `⏱ TLE`, `✘ RE`, `✘ CE`) and auto-collapses on AC; on WA the **Got** box marks the exact characters that differ in red.
* 📥 **Auto-parses problems** — payloads are captured on `127.0.0.1:10043`; the file is created from `~/.zch/boilerplate.cpp` with a `// URL:` header, tests live in `~/.zch/.tc/`, and the file opens in Zed automatically.
* ⚡ **Blazing fast execution** — compile once, replay every case with 1ms polling, per-case timing, and hard TLE kills.
* 🤖 **One-key Codeforces submissions** — a confirmation modal shows every detail, then async verdicts stream into the **Submissions bar** (`⠹ submitting → ✔ ACCEPTED`); Safari tabs are tracked by URL (**INTERRUPTED** / **TIMED OUT** handled) and CAPTCHAs raise the window automatically. `zch submit` works from the terminal too.
* 🎛️ **Command palette from Zed** — Zed tasks drive the running TUI over HTTP (`send r`, `send s`, …) with the active `${ZED_FILE}`.
* 📁 **Live directory switching** — `cd`, `ls`, `pwd` console with TAB path completion; new problems are saved wherever you point it.
* 🧩 **Multi-language** — C++17/20/23, Python 3, Java; language + flags persist in `~/.zch/config.json` via the `c` menu.

---

## 📋 Prerequisites

- **macOS** *(relies on native Apple Events for Safari automation)*
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

**Or:**

```bash
curl -fsSL https://github.com/prsweet/zch/releases/latest/download/zch-installer.sh | sh
```

### 2. Setup Zed tasks

Append to `~/.config/zed/tasks.json` (press `cmd+shift+R` to spawn tasks):

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
    "label": "CP: Submit to Codeforces",
    "command": "zch submit \"${ZED_FILE}\"",
    "use_new_terminal": true,
    "allow_concurrent_runs": true,
    "reveal": "always",
    "hide": "never"
  }
]
```

### 3. Optimize your keymap

Add to `~/.config/zed/keymap.json`:

```json
[
  {
    "context": "Editor",
    "bindings": {
      "cmd-'": ["task::Spawn", { "task_name": "CP: Run Tests" }],
      "cmd-enter": ["task::Spawn", { "task_name": "CP: Submit to Codeforces" }]
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

Click the **Competitive Companion** extension on any Codeforces problem page. The file is created in the current directory, saved to the test database, and opened in Zed automatically.

### 3. Test & submit from the TUI

| Key | Action |
| --- | --- |
| `r` | compile & run all test cases |
| `a` | add a custom test case |
| `e` | edit existing test cases |
| `s` | submit to Codeforces (confirmation modal → Enter submits / Backspace cancels) |
| `d` | console: `cd <path>` · `ls` · `pwd` (TAB completes paths) |
| `c` | config: language + extra compile flags |
| `q` | quit |

While editing, `Tab` switches between **Input** / **Expected** (and wraps to the previous/next case with `Shift+Tab`), `Esc` saves.

### 4. Test & submit from Zed

* **Run (`cmd-'`)** — asks the running TUI to load `${ZED_FILE}` and run it.
* **Submit (`cmd-enter`)** — opens a dedicated terminal tab that shows every submission detail (platform, problem ID, URL, language, browser, local tests) and waits for **`Enter`** to submit or **`Backspace`** to cancel. The verdict (and any CAPTCHA warning) streams into that tab.

The `send` subcommand talks to the TUI over HTTP, while `submit` runs standalone in its own terminal:

```bash
zch send r /path/to/Problem.cpp   # run
zch send s /path/to/Problem.cpp   # submit (confirmation modal inside the TUI)
zch submit /path/to/Problem.cpp   # submit here (Enter/Backspace prompt, -y to skip)
```

Set `ZCH_PORT` to run on a different port than `10043`.

---

## 🛡️ Dealing with CAPTCHAs

Codeforces occasionally serves an invisible CAPTCHA. When automation hits one, the TUI alerts you (`⚠ CAPTCHA — solve it, submission continues automatically`) and Safari foregrounds itself. Solve it, and the live verdict polling resumes on its own.

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

---

## 💡 Pro-tips for maximum performance

![zch running a test case in 1ms](sample.png)

### 1. Drop execution latency to 3-4ms ⚡

macOS security scans (`syspolicyd`) add ~200ms of lag when running newly compiled binaries. Fix:

1. **System Settings** → **Privacy & Security** → **Developer Tools**: add **Zed** and your terminal emulator, toggle them **ON**.
2. Add the same under **Full Disk Access**.

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

Apple Clang doesn't ship bits/stdc++.h or PBDS. The [std-with-pbds](https://github.com/prsweet/std-with-pbds.git) helper repo sets up both for Apple Clang.
