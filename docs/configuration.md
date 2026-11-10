# Configuring zch

Back to the [README](../README.md).

## The config sheet

Press `c`. `Tab` moves between three fields:

- **Language** — C++, Python 3 or Java; `←` / `→` changes it.
- **Compile flags** — tags such as `-O2` or `-std=c++20`.
- **Theme** — prsweet, catppuccin, obsidian or daylight; `←` / `→` changes it, and the whole screen previews it live.

### Compile flags

zch adds no flags of its own. Type a flag and press `Enter` to add it:

- A flag must start with `-`. Duplicates and unknown standards such as `-std=c++1000` are refused, with the reason shown under the field.
- A value that belongs to a flag, such as the path after `-include-pch`, becomes its own tag right after it.
- `x` starts deleting: `Tab` / `Shift+Tab` choose a tag, `Enter` confirms, `Enter` again deletes. Deleting either half of an `-include-pch` pair removes both.
- Each language keeps its own list, so switching the language switches the flags with it.

## config.json

Everything is saved to `~/.zch/config.json`. Fields you can also edit by hand:

| Field | Meaning |
| --- | --- |
| `language` | `Cpp`, `Python` or `Java` |
| `flags_by_language` | compile flags for each language |
| `theme` | `prsweet`, `catppuccin`, `obsidian` or `daylight` |
| `checker` | how output is compared; see below |
| `time_limit_multiplier` | scales every time limit, for a machine slower than the judge (default `1`) |
| `cf_language_ids` | Codeforces language id overrides, such as `{ "Python": "31" }` |

```json
{
  "theme": "daylight",
  "checker": { "mode": "float", "epsilon": 1e-6 },
  "time_limit_multiplier": 1.5
}
```

### Checkers

| `checker` | Accepts when |
| --- | --- |
| `{ "mode": "tokens" }` | whitespace-separated tokens match exactly (default) |
| `{ "mode": "lines" }` | lines match, ignoring trailing spaces and trailing blank lines |
| `{ "mode": "case_insensitive" }` | tokens match ignoring case (`YES` equals `yes`) |
| `{ "mode": "float", "epsilon": 1e-6 }` | numbers are within `epsilon`, absolute or relative |
| `{ "mode": "custom", "command": "./check" }` | the command, run as `<command> <input> <output> <answer>`, exits with 0 |

A problem can override the checker in its saved test file under `~/.zch/.tc/`.

## Files

| Path | Holds |
| --- | --- |
| `~/.zch/config.json` | settings |
| `~/.zch/boilerplate.cpp`, `.py`, `.java` | the template a new problem's file starts from |
| `~/.zch/.tc/` | saved test cases for each problem |

## Environment variables

| Variable | Effect |
| --- | --- |
| `ZCH_PORT` | listen on a port other than `10043` |
| `ZCH_REDUCED_MOTION=1` or `ZCH_ANIMATIONS=off` | no transitions, static spinners |
| `ZCH_COLOR=truecolor` or `256` | force the colour depth when detection guesses wrong |
| `NO_COLOR` | honoured; every state is also shown by a word or symbol |
