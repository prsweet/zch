//! Selecting text with the mouse and copying it to the clipboard.
//!
//! Mouse capture takes the terminal's own selection away, so zch brings it
//! back: drag to select, release to copy. A selection stays inside the
//! panel it started in (one case's input, the got pane, the transcript, the
//! console…) instead of running across the whole screen line, shows as a
//! light wash while the button is held, and goes away on release.

use super::*;

/// How strongly the selection washes the cells under it.
const WASH: f32 = 0.3;

/// A mouse selection in screen cells, in the order it was made, kept
/// inside `bounds`: the panel it started in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct TextSelection {
    pub(super) anchor: Position,
    pub(super) head: Position,
    pub(super) bounds: Rect,
}

impl TextSelection {
    /// Start and end in reading order.
    pub(super) fn ordered(self) -> (Position, Position) {
        let (a, b) = (self.anchor, self.head);
        if (a.y, a.x) <= (b.y, b.x) {
            (a, b)
        } else {
            (b, a)
        }
    }

    /// The selected cells of row `y` as an inclusive column range.
    fn columns(self, y: u16) -> Option<(u16, u16)> {
        let (start, end) = self.ordered();
        if y < start.y || y > end.y || self.bounds.width == 0 {
            return None;
        }
        let left = self.bounds.x;
        let right = self.bounds.right() - 1;
        let from = if y == start.y { start.x } else { left };
        let to = if y == end.y { end.x } else { right };
        (from <= to).then_some((from.clamp(left, right), to.clamp(left, right)))
    }
}

/// Keep `position` inside `bounds`.
fn clamp_to(bounds: Rect, position: Position) -> Position {
    Position::new(
        position
            .x
            .clamp(bounds.x, bounds.right().saturating_sub(1).max(bounds.x)),
        position
            .y
            .clamp(bounds.y, bounds.bottom().saturating_sub(1).max(bounds.y)),
    )
}

/// The text a selection covers, like a terminal copies it: each row cut to
/// the panel, trailing spaces dropped.
pub(super) fn selected_text(rows: &[String], selection: TextSelection) -> String {
    let (start, end) = selection.ordered();
    let mut lines = Vec::new();
    for y in start.y..=end.y {
        let (Some(row), Some((from, to))) = (rows.get(y as usize), selection.columns(y)) else {
            continue;
        };
        let line: String = row
            .chars()
            .skip(from as usize)
            .take((to - from) as usize + 1)
            .collect();
        lines.push(line.trim_end().to_string());
    }
    lines.join("\n").trim_matches('\n').to_string()
}

/// Put `text` on the system clipboard: the platform tool when there is one,
/// and always OSC 52 too, so it also works over SSH and in tmux.
fn copy_to_clipboard(text: &str) -> bool {
    use std::io::Write;
    // Tests must never overwrite the developer's clipboard.
    if cfg!(test) {
        return !text.is_empty();
    }
    const TOOLS: &[(&str, &[&str])] = &[
        ("pbcopy", &[]),
        ("wl-copy", &[]),
        ("xclip", &["-selection", "clipboard"]),
    ];
    let mut copied = false;
    for (program, args) in TOOLS {
        let Ok(mut child) = std::process::Command::new(program)
            .args(*args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
        else {
            continue;
        };
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(text.as_bytes());
        }
        if child.wait().is_ok_and(|status| status.success()) {
            copied = true;
            break;
        }
    }
    use base64::Engine;
    let encoded = base64::engine::general_purpose::STANDARD.encode(text);
    let mut stdout = std::io::stdout();
    let osc = stdout
        .write_all(format!("\x1b]52;c;{encoded}\x07").as_bytes())
        .and_then(|_| stdout.flush());
    copied || osc.is_ok()
}

impl Helper {
    /// The panel under `position`: the smallest text region drawn there
    /// last frame, the case panes included; `None` outside every panel.
    fn region_at(&self, position: Position) -> Option<Rect> {
        let size = |rect: &Rect| u32::from(rect.width) * u32::from(rect.height);
        self.tc_list
            .testcases
            .iter()
            .flat_map(TestCase::text_regions)
            .chain(self.text_regions.iter().copied())
            .filter(|rect| rect.width > 0 && rect.height > 0 && rect.contains(position))
            .min_by_key(size)
    }

    /// Mouse text selection. A press only remembers where it happened (a
    /// click still clicks); dragging away from it selects, unless the
    /// press grabbed a scrollbar; the release copies and clears it.
    /// Returns whether the event was used up.
    pub(super) fn track_text_selection(
        &mut self,
        mouse: &ratatui::crossterm::event::MouseEvent,
    ) -> bool {
        let position = Position::new(mouse.column, mouse.row);
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                self.press = Some(position);
                self.text_selection = None;
                false
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                if self.dragging_a_scrollbar() {
                    return false;
                }
                let Some(anchor) = self.press else {
                    return false;
                };
                if anchor == position && self.text_selection.is_none() {
                    return false;
                }
                // Only text inside a panel can be selected; a drag that starts
                // on chrome or empty space selects nothing.
                let Some(bounds) = self
                    .text_selection
                    .map(|selection| selection.bounds)
                    .or_else(|| self.region_at(anchor))
                else {
                    return false;
                };
                self.text_selection = Some(TextSelection {
                    anchor,
                    head: clamp_to(bounds, position),
                    bounds,
                });
                true
            }
            MouseEventKind::Up(MouseButton::Left) => {
                self.press = None;
                let Some(selection) = self.text_selection.take() else {
                    return false;
                };
                let text = selected_text(&self.screen_rows, selection);
                if text.trim().is_empty() {
                    return true;
                }
                if copy_to_clipboard(&text) {
                    let count = text.chars().count();
                    self.status.success(format!(
                        "copied {count} character{}",
                        if count == 1 { "" } else { "s" }
                    ));
                } else {
                    self.status
                        .error("cannot copy — no clipboard tool found (pbcopy, wl-copy, xclip)");
                }
                true
            }
            _ => false,
        }
    }

    fn dragging_a_scrollbar(&self) -> bool {
        self.submissions_scrollbar_drag.is_some()
            || self.tc_list.is_dragging()
            || self.config_menu.is_dragging()
            || self
                .pane_dialog
                .as_ref()
                .is_some_and(|d| d.v_scroll_drag.is_some() || d.h_scroll_drag.is_some())
    }

    /// Keep this frame's text for copying, and wash the selected cells with
    /// 30% of the reading colour while the button is held.
    pub(super) fn paint_selection(&mut self, frame: &mut ratatui::Frame) {
        let buffer = frame.buffer_mut();
        let area = buffer.area;
        self.screen_rows = (area.top()..area.bottom())
            .map(|y| {
                (area.left()..area.right())
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect();
        let Some(selection) = self.text_selection else {
            return;
        };
        let p = theme::p();
        for y in area.top()..area.bottom() {
            let Some((from, to)) = selection.columns(y) else {
                continue;
            };
            for x in from..=to.min(area.right().saturating_sub(1)) {
                let cell = &mut buffer[(x, y)];
                let under = match cell.bg {
                    ratatui::style::Color::Rgb(r, g, b) => theme::Rgb(r, g, b),
                    _ => p.bg,
                };
                cell.set_bg(theme::blend(under, p.text, WASH));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows() -> Vec<String> {
        [
            "  01 ▾ wrong answer   ",
            "     in 5      │ got 7",
            "    exp 6      │ got 8",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect()
    }

    fn pick(bounds: Rect, a: (u16, u16), b: (u16, u16)) -> String {
        selected_text(
            &rows(),
            TextSelection {
                anchor: Position::new(a.0, a.1),
                head: Position::new(b.0, b.1),
                bounds,
            },
        )
    }

    #[test]
    fn a_selection_reads_like_a_terminal_copy() {
        let screen = Rect::new(0, 0, 22, 3);
        assert_eq!(pick(screen, (7, 0), (18, 0)), "wrong answer");
        assert_eq!(
            pick(screen, (18, 0), (7, 0)),
            "wrong answer",
            "either direction"
        );
    }

    #[test]
    fn a_selection_stays_inside_its_panel() {
        // The "got" pane: columns 17..22. Rows in between keep to it.
        let got = Rect::new(17, 1, 5, 2);
        assert_eq!(pick(got, (17, 1), (21, 2)), "got 7\ngot 8");
        let clamped = clamp_to(got, Position::new(2, 9));
        assert_eq!(clamped, Position::new(17, 2));
    }
}
