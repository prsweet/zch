# zch

**zch is a terminal dashboard for competitive programming in the [Zed](https://zed.dev/) editor on macOS. It imports problems from [Competitive Companion](https://github.com/jmerle/competitive-companion), runs your samples with diffs and timings, and submits to Codeforces without leaving your editor.**

[![Release](https://img.shields.io/github/v/release/prsweet/zch?style=flat-square)](https://github.com/prsweet/zch/releases/latest)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue?style=flat-square)](LICENSE)
[![macOS](https://img.shields.io/badge/os-macOS-black?style=flat-square&logo=apple)](#prerequisites)
[![Rust](https://img.shields.io/badge/rust-2024-orange?style=flat-square&logo=rust)](https://www.rust-lang.org/)

Click Competitive Companion on a Codeforces problem and the source file opens in Zed with its samples saved. Press `r` to compile and run every sample, then `s` to submit; the verdict streams back into the same terminal. zch is a single Rust binary built with [ratatui](https://ratatui.rs/), and a keyboard-first alternative to the VS Code cph extension for people who write their solutions in Zed.

## Features

- **Problem import** — Competitive Companion sends problems to `127.0.0.1:10043`; zch creates the file from your boilerplate, saves the samples and opens it in Zed.
- **Fast runs** — binaries are cached by source and flags, so unchanged code replays instantly. Each case reports its verdict, time and peak memory, and runaway programs are killed at the time limit.
- **Clear verdicts** — every case shows input, expected and got side by side, with the exact characters of a wrong answer marked. Accepted cases fold away.
- **Judge-style checking** — token (default), line, case-insensitive, floating-point or a custom testlib-style checker.
- **Interactive problems** — a problem without samples runs live on a pseudo-terminal.
- **Codeforces submission** — one key submits through Safari in the background, and the verdict streams back.
- **Read anything cut short** — double-click a box, error or message that did not fit to open it whole.
- **Four themes** — prsweet (default), catppuccin, obsidian and daylight (light).
- **C++, Python 3 and Java** — compile flags are explicit tags you set per language; zch adds none of its own.

## Prerequisites

- **macOS** — submission uses Safari and Apple Events.
- **[Zed](https://zed.dev/)** and the **[Competitive Companion](https://github.com/jmerle/competitive-companion)** browser extension.
- A **compiler** for your language: Apple Clang or Homebrew `g++` for C++, `python3`, or a JDK.
- A **[Nerd Font](https://www.nerdfonts.com/)** in your terminal, for the file and folder icons.
- To submit: **Safari** with *Allow JavaScript from Apple Events* turned on (Settings → Advanced → *Show features for web developers*, then Develop → *Allow JavaScript from Apple Events*).

## Installation

```bash
brew install prsweet/tap/zch
```

Or with the install script:

```bash
curl -fsSL https://github.com/prsweet/zch/releases/latest/download/zch-installer.sh | sh
```

Prebuilt binaries for Apple Silicon and Intel Macs are on the [releases page](https://github.com/prsweet/zch/releases).

## Zed integration

Add these tasks to `~/.config/zed/tasks.json`:

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
    "command": "zch send s \"${ZED_FILE}\"",
    "use_new_terminal": false,
    "allow_concurrent_runs": false,
    "reveal": "never",
    "hide": "always"
  }
]
```

And these keys to `~/.config/zed/keymap.json`:

```json
[
  {
    "context": "Editor",
    "bindings": {
      "cmd-'": ["task::Spawn", { "task_name": "CP: Run Tests" }],
      "cmd-enter": ["task::Spawn", { "task_name": "CP: Submit to Codeforces" }],
      "cmd-r": ["workspace::SendKeystrokes", "cmd-alt-shift-r cmd-\\"]
    }
  },
  {
    "context": "Workspace",
    "bindings": {
      "cmd-alt-shift-r": ["task::Spawn", { "task_name": "CP: Start Listener (Current Folder)" }]
    }
  }
]
```

| Key in Zed | Does |
| --- | --- |
| `cmd-r` | opens zch in a split beside your code |
| `cmd-'` | runs the file you are editing |
| `cmd-enter` | opens the submit confirmation for the file you are editing |

`zch send` prints the real result, or why it was refused, so the tasks work from any editor or script.

## Usage

1. Press `cmd-r` in your problems folder (or run `zch` in a terminal).
2. Click Competitive Companion on a problem page. The file opens in Zed.
3. Write your solution, then press `r` in zch (or `cmd-'` in Zed) to run the samples.
4. Press `s` (or `cmd-enter` in Zed) and confirm with `Enter` to submit.

| Key | Action |
| --- | --- |
| `r` | compile and run every case |
| `a` / `e` / `x` | add / edit / delete cases |
| `s` | submit to Codeforces |
| `d` | directory console (`cd`, `ls`, `pwd`) |
| `c` | config: language, compile flags, theme |
| `m` | message history |
| `Esc` | cancel a running test |
| `Ctrl-H` / `F1` | keys for the current mode |
| `q` | quit |

The bottom line always shows the keys for the mode you are in. Editing, interactive sessions, mouse support and the limits on large output are covered in [docs/usage.md](docs/usage.md).

## Configuration

Press `c` to choose the language, add compile flags (`-O2`, `-std=c++20`, …) and pick a theme. Everything is saved to `~/.zch/config.json`, which also sets the output checker and a time-limit multiplier:

```json
{
  "theme": "daylight",
  "checker": { "mode": "float", "epsilon": 1e-6 },
  "time_limit_multiplier": 1.5
}
```

New files start from `~/.zch/boilerplate.cpp` (or `.py` / `.java`). All options, checker modes and environment variables are in [docs/configuration.md](docs/configuration.md).

## Codeforces submission

`s` opens a confirmation sheet with the problem, file, language and your last local result. `Enter` submits through Safari in a background tab, using the account you are already signed into. The verdict streams into the submissions band: green for accepted, yellow for pretests passed, red for rejected.

- **CAPTCHA:** Safari comes to the front; solve it and polling continues on its own.
- **403 / 429 from Codeforces:** the submission is marked *blocked* instead of waiting forever. If the page refused before sending, nothing was submitted; if after, the tab stays open so you can check the verdict on the site.
- **Language id:** C++ follows your last `-std=` flag (C++17 → 54, C++20 → 89, C++23 → 91); Python goes to PyPy 3 (70) and Java to Java 21 (87). Override them with `cf_language_ids` in the config.

## FAQ

**How do I use Competitive Companion with Zed?**
Run zch in your problems folder; it listens on the port Competitive Companion sends to. Click the extension on a problem page and the file opens in Zed with its samples saved.

**Can I submit to Codeforces without leaving my editor?**
Yes. Press `cmd-enter` in Zed (with the task above) or `s` in zch, then `Enter` to confirm.

**Why Safari only?**
Submission needs a browser that runs JavaScript in a background tab without stealing focus. Safari does that natively through AppleScript; other browsers would come to the front mid-solve.

**Does zch work on Linux or Windows?**
Not yet: submission depends on macOS Apple Events and Safari.

**Is my code or login sent anywhere?**
Your code leaves your machine only when you submit, through the Safari tab you are already signed into. zch never asks for your password, and its listener accepts connections from your own machine only.

**How do I make C++ compile faster?**
Precompile `bits/stdc++.h` and pass it with `-include-pch`. See [docs/performance.md](docs/performance.md).

## Documentation

- [Usage](docs/usage.md) — keys, editing, mouse, interactive sessions, large output
- [Configuration](docs/configuration.md) — config sheet, `config.json`, checkers, environment variables
- [Performance tips](docs/performance.md) — faster runs and precompiled headers on macOS
- [Architecture](docs/architecture.md) — how the code is organised

## License

[MIT](LICENSE) © prsweet
