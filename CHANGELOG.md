# Changelog

## Unreleased

- Help is `Ctrl-H` only; the undocumented `F1` alias is gone.
- Shorter package description, so the Homebrew formula passes `brew style`.
- A demo GIF at the top of the README.

## 3.0.0

A redesign of the whole interface, and a faster, sturdier core.

### Interface

- Four themes, up from two — prsweet (default), catppuccin, obsidian and daylight, the first light theme — switched live from the config sheet. Every text colour is checked for legible contrast.
- Test cases show input, expected and got in their own padded boxes, the verdict centred in the header and the got box framed in the verdict's colour. Accepted cases fold away; `+` / `-` folds any case.
- Double-click now opens anything that was cut short, not just case boxes: errors, messages, submissions and file listings too. Text that fits opens nothing.
- A big braille ZCH on the empty screen, with a continuous shimmer.
- Dialogs open as a sheet with a bar across the top instead of a full border.
- `Ctrl-H` shows the keys for the current mode; `m` opens the message history.

### Running and checking

- Judge-style checkers: tokens (default), lines, case-insensitive, floating-point and custom testlib-style checkers, globally or per problem.
- Live sessions for interactive problems, on a real pseudo-terminal.
- Peak memory for every case, and a time-limit multiplier for slower machines.
- The expanded output viewer draws only the rows on screen, so scrolling a long output stays smooth.

### Submitting

- A Codeforces 403 or 429 page is reported as *blocked* instead of polling forever.

### Project

- MIT licensed.
- Documentation split into a short README and pages under `docs/`.
