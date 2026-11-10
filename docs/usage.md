# Using zch

Back to the [README](../README.md).

## Keys

The bottom line of the screen always shows the keys for the mode you are in, and `Ctrl-H` opens them as a sheet.

| Key | Action |
| --- | --- |
| `r` | compile (cached when unchanged) and run every case, or start an interactive session when the problem has no samples |
| `a` | add a case |
| `e` | edit cases |
| `x` | delete a case |
| `s` | submit to Codeforces |
| `d` | directory console: `cd <path>`, `ls`, `pwd` |
| `c` | config: language, compile flags, theme |
| `m` | messages: everything the console said this session |
| `Esc` | cancel a running test |
| `Ctrl-H` | keys for the current mode |
| `q` | quit (asks first while a submission is still being judged) |

## Cases

Each case has a header with its number, the verdict in the middle, and time and memory on the right. Below it sit three boxes: **input**, **expected** and **got**. The got box is framed in the verdict's colour, and for a wrong answer the characters that differ from the expected output are marked.

Accepted cases fold to their header; click `+` / `-` on a header to fold or unfold it by hand.

### Editing

- `e` edits the existing cases and `a` adds a new one. `Tab` / `Shift+Tab` move between the input and expected boxes, and on to the next case while editing.
- `Esc` saves.
- Only the box being typed into scrolls. The others show their content and end a line that runs past the edge with `…`. Box heights follow the content, never the focus, so `Tab` does not make the layout jump.

### Deleting

`x` expands every case and marks the selected one in red. `Tab` / `Shift+Tab` choose the case, `Enter` opens a confirmation showing its input and expected output, and `Enter` again deletes it. `Backspace` goes back and `Esc` cancels.

## Mouse

- **Double-click** anything shown cut short to read it whole: a case box whose content does not fit, a capped output, a config error, a console message, a submission or a file listing. Text that already fits opens nothing.
- In that viewer, scroll with the wheel, `↑` / `↓`, `←` / `→` and `PgUp` / `PgDn`, or drag either scrollbar. `Esc` closes it.
- **Drag** across text inside a panel to select it; releasing copies it to the clipboard.
- The **wheel** scrolls whatever is under the pointer, and every scrollbar can be dragged.

## Large output

- A result is cut to a 24 KB preview as soon as it arrives.
- A box shows at most 300 lines or 24,000 characters, ending with a note such as `… 812 more lines hidden`.
- The double-click viewer keeps the full output, with the full diff for a wrong answer, up to 10 MB or 20,000 lines.
- Anything larger is written to a temporary file and read page by page, with the diff recomputed for each page.
- A program is stopped once it writes 64 MB.

## Interactive problems

`r` on a problem with no saved cases starts the program live on a pseudo-terminal, so it behaves as it would in a real terminal and prompts appear immediately.

| Key | Action |
| --- | --- |
| `Enter` | start typing, then send the line |
| `Ctrl-D` | end of input |
| `Ctrl-C` | interrupt the program |
| `PgUp` / `PgDn` | scroll the transcript |
| `Esc` | stop typing; press again to stop the program and keep the transcript |
| `r` | restart |

The transcript keeps the last 2,000 lines (8 MB in total, 8 KB per line), and escape sequences are stripped so a program that prints colours cannot break the screen.

Only one run is active at a time. Editing, deleting or receiving a new problem cancels it and stops the whole process group.

## Directory console

`d` opens a one-line console for `cd <path>`, `ls` and `pwd`. `Tab` completes and cycles through matches, and `↑` / `↓` recall earlier commands. New problems are saved in the current directory.

Problems whose names clean up to the same filename (`A. Sum` exists in every contest) get a short suffix from their URL, so their tests and binaries never overwrite each other.

## From Zed or a script

```bash
zch send r /path/to/Problem.cpp   # load the file and run it
zch send s /path/to/Problem.cpp   # load the file and open the submit confirmation
```

`zch send` talks to the running dashboard and prints the real result, or the reason it was refused.
