//! The frame around the work: header, progress rule, submissions, console.

use super::*;

/// Opens help from anywhere: Ctrl-H is never text. F1 and Option-H work
/// too, where the terminal delivers them.
pub(in crate::helper) const HELP_KEY: &str = "Ctrl-H";

/// Nerd Font powerline arrow: closes the mode segment into the console.
const SEGMENT_END: &str = "\u{e0b0}";
/// Blank rows above and below the header's content.
pub(super) const HEADER_PAD: u16 = 1;
/// Blank rows between the app bar and the folder row.
pub(super) const HEADER_GAP: u16 = 1;

/// Rows the header takes around `path_rows` of folder and command output.
pub(super) const fn header_rows(path_rows: u16) -> u16 {
    HEADER_PAD * 2 + 1 + HEADER_GAP + path_rows
}

impl Helper {
    /// The header block: the app bar and, under it, the folder new problems
    /// are saved to (the directory console when `d` is open). Both rows sit
    /// on the raised surface with the same 2-cell inset, so they read as one
    /// band rather than two bars and a rule.
    pub(in crate::helper) fn draw_header(&mut self, frame: &mut Frame, area: Rect, now: Instant) {
        ui::fill(frame, area, theme::surface());
        let app_bar = Rect {
            y: area.y + HEADER_PAD,
            height: 1,
            ..area
        };
        self.draw_app_bar(frame, inset(app_bar), now);
        let path = Rect {
            y: app_bar.bottom() + HEADER_GAP,
            height: area.height.saturating_sub(HEADER_PAD * 2 + 1 + HEADER_GAP),
            ..area
        };
        let directory = self.input_mode == InputMode::Directory;
        self.dir_terminal.draw_input(frame, path, directory, now);
        self.text_regions.push(inset(app_bar));
        self.text_regions.push(inset(path));
    }

    /// `◆ zch  v2.1.0` on the left; the loaded problem on the right, with a
    /// dot that lights up once there is one. A new problem's name fades in.
    fn draw_app_bar(&self, frame: &mut Frame, row: Rect, now: Instant) {
        let left = Line::from(wordmark());
        let room = (row.width as usize).saturating_sub(left.width() + 4);
        let right = match &self.active_problem {
            Some(problem) => {
                let arrive = self
                    .motion
                    .problem_at
                    .map_or(1.0, |at| anim::ease_out(at, anim::REVEAL, now));
                let p = theme::p();
                Line::from(vec![
                    Span::styled(
                        "● ",
                        Style::new().fg(theme::blend(p.faint, p.success, arrive)),
                    ),
                    Span::styled(
                        ui::truncate_revealing(
                            &problem.name,
                            room,
                            Rect {
                                x: row.right().saturating_sub(room as u16 + 2),
                                width: (room as u16 + 2).min(row.width),
                                ..row
                            },
                            "Problem",
                        ),
                        Style::new()
                            .fg(theme::blend(p.faint, p.text, arrive))
                            .bold(),
                    ),
                ])
            }
            None => Line::from(Span::styled(
                "no problem loaded",
                Style::new().fg(theme::faint()),
            )),
        };
        ui::row(frame, row, left, right);
    }

    /// The rule under the header doubles as the run's progress bar: it
    /// fills in the accent as cases finish (a soft shimmer while compiling),
    /// then flashes the outcome's colour and fades back to a plain line.
    pub(in crate::helper) fn draw_rule(&self, frame: &mut Frame, row: Rect, now: Instant) {
        let width = row.width as usize;
        let p = theme::p();
        let buf = frame.buffer_mut();
        let mut paint = |index: usize, symbol: &str, color: ratatui::style::Color| {
            buf[(row.x + index as u16, row.y)]
                .set_symbol(symbol)
                .set_style(Style::new().fg(color));
        };

        if self.running {
            let filled = (self.motion.progress * width as f32).round() as usize;
            for index in 0..width {
                if index < filled {
                    paint(index, "━", theme::accent());
                } else {
                    paint(index, "─", theme::border());
                }
            }
            let compiling = self.run_progress.current.is_none() && self.run_progress.done == 0;
            if compiling && anim::enabled() {
                // A short band sweeping across while there is nothing to count.
                let t = (now
                    .saturating_duration_since(self.motion.epoch)
                    .as_secs_f32()
                    / 1.4)
                    .fract();
                let band = (width as f32 * 0.18).max(6.0);
                let center = -band + t * (width as f32 + band * 2.0);
                for index in 0..width {
                    let strength = 1.0 - ((index as f32 - center).abs() / band).min(1.0);
                    if strength > 0.05 {
                        paint(
                            index,
                            if strength > 0.5 { "━" } else { "─" },
                            theme::blend(p.border, p.accent, strength),
                        );
                    }
                }
            }
            return;
        }

        if let Some((tone, since)) = self.motion.flash
            && anim::active(since, anim::FLASH, now)
        {
            let t = anim::ease_out(since, anim::FLASH, now);
            let color = theme::blend(tone.rgb(), p.border, t);
            for index in 0..width {
                paint(index, if t < 0.6 { "━" } else { "─" }, color);
            }
            return;
        }
        for index in 0..width {
            paint(index, "─", theme::border());
        }
    }

    /// The rule over the footer.
    pub(in crate::helper) fn draw_footer_rule(&self, frame: &mut Frame, row: Rect) {
        ui::hairline(frame, row, theme::border());
    }

    pub(in crate::helper) fn submissions_viewport(&self) -> usize {
        if self.submissions_area.height > 1 {
            self.submissions_area.height as usize - 1
        } else {
            SUBMISSION_VIEWPORT
        }
    }

    fn submissions_max_scroll(&self) -> usize {
        self.submissions
            .len()
            .saturating_sub(self.submissions_viewport())
    }

    /// Wheel scrolling inside the submissions rows (hovered area only).
    pub(in crate::helper) fn scroll_submissions(&mut self, down: bool) {
        let max_scroll = self.submissions_max_scroll();
        if down {
            self.submission_scroll = (self.submission_scroll + 1).min(max_scroll);
        } else {
            self.submission_scroll = self.submission_scroll.saturating_sub(1);
        }
        self.submissions_scrollbar_drag = None;
    }

    pub(in crate::helper) fn begin_submissions_scrollbar_drag(&mut self, position: Position) {
        if let Some((drag, scroll)) = ScrollbarDrag::begin(
            self.submissions_scrollbar_area,
            true,
            self.submissions_max_scroll(),
            self.submission_scroll,
            position,
        ) {
            self.submissions_scrollbar_drag = Some(drag);
            self.submission_scroll = scroll;
        }
    }

    /// One row per submission, newest first: its state glyph, the problem,
    /// what the judge says, and how long ago (or how long so far).
    pub(in crate::helper) fn draw_submissions(
        &mut self,
        frame: &mut Frame,
        area: Rect,
        now: Instant,
    ) {
        self.submissions_area = area;
        self.submissions_scrollbar_area = Rect::default();
        if area.height < 2 {
            self.submissions_scrollbar_drag = None;
            return;
        }

        // The title row: what this block is, and how many are still out.
        let total = self.submissions.len();
        let judging = self.submissions.iter().filter(|s| !s.done()).count();
        let title = Rect {
            x: area.x + ui::GUTTER,
            width: area.width.saturating_sub(ui::GUTTER * 2),
            height: 1,
            ..area
        };
        let count = match judging {
            0 => format!("{total} done"),
            n => format!("{n} judging · {} done", total - n),
        };
        ui::row(
            frame,
            title,
            Line::from(Span::styled(
                "Submissions",
                Style::new().fg(theme::muted()).bold(),
            )),
            Line::from(Span::styled(count, Style::new().fg(theme::faint()))),
        );

        let area = Rect {
            y: area.y + 1,
            height: area.height - 1,
            ..area
        };
        let viewport = area.height as usize;
        let max_scroll = total.saturating_sub(viewport);
        self.submission_scroll = self.submission_scroll.min(max_scroll);
        let needs_scrollbar = total > viewport;
        let rows = Rect {
            x: area.x + ui::GUTTER,
            width: area
                .width
                .saturating_sub(ui::GUTTER * 2 + u16::from(needs_scrollbar)),
            ..area
        };
        self.text_regions.push(rows);
        let label_width = self
            .submissions
            .iter()
            .map(|s| s.label.chars().count())
            .max()
            .unwrap_or(0)
            .min(rows.width as usize / 3);

        for (index, submission) in self
            .submissions
            .iter()
            .rev()
            .skip(self.submission_scroll)
            .take(viewport)
            .enumerate()
        {
            let line = Rect {
                y: rows.y + index as u16,
                height: 1,
                ..rows
            };
            // Only a submission still being judged shows a clock.
            let (message_color, when) = if submission.done() {
                (submission.tone.color(), String::new())
            } else {
                (
                    theme::muted(),
                    format_elapsed(now.saturating_duration_since(submission.started)),
                )
            };
            let label = ui::truncate(&submission.label, label_width);
            let pad = label_width.saturating_sub(label.chars().count());
            // The verdict message is not cut, but the row may be too narrow
            // for it: then the whole submission is a double-click away.
            let needed = 2 + label_width + 3 + submission.message.width() + 2 + when.width();
            if label != submission.label || needed > line.width as usize {
                ui::reveal(
                    line,
                    "Submission",
                    &format!("{}\n{}", submission.label, submission.message),
                );
            }
            let left = Line::from(vec![
                Span::styled(
                    format!("{} ", submission.tone.glyph(self.motion.epoch, now)),
                    Style::new().fg(submission.tone.color()).bold(),
                ),
                Span::styled(
                    format!("{label}{}   ", " ".repeat(pad)),
                    Style::new().fg(theme::text()),
                ),
                Span::styled(submission.message.clone(), Style::new().fg(message_color)),
            ]);
            let right = Line::from(Span::styled(when, Style::new().fg(theme::faint())));
            ui::row(frame, line, left, right);
        }

        if needs_scrollbar {
            let scrollbar_area = Rect {
                x: area.right() - 1,
                width: 1,
                ..area
            };
            self.submissions_scrollbar_area = scrollbar_area;
            let mut state = ScrollbarState::new(max_scroll).position(self.submission_scroll);
            frame.render_stateful_widget(
                scrollbar(ScrollbarOrientation::VerticalRight),
                scrollbar_area,
                &mut state,
            );
        } else {
            self.submissions_scrollbar_drag = None;
        }
    }

    /// The console: a segment in the mode's colour, the latest message, and
    /// this mode's keys on the right. Space goes first to the help key (so
    /// help is always one keystroke away), then to the message, and only
    /// what is left to the other keys, which give way from the right.
    pub(in crate::helper) fn draw_console(&mut self, frame: &mut Frame, row: Rect, now: Instant) {
        ui::fill(frame, row, theme::surface());
        let (mode, mode_rgb) = self.mode_badge();
        let mode_color = theme::paint(mode_rgb);
        let mut left = vec![
            Span::styled(
                format!(" {mode} "),
                Style::new().fg(theme::bg()).bg(mode_color).bold(),
            ),
            Span::styled(
                SEGMENT_END,
                Style::new().fg(mode_color).bg(theme::surface()),
            ),
            Span::raw(" "),
        ];
        let prefix: usize = left.iter().map(Span::width).sum();
        let room = (row.width as usize).saturating_sub(prefix + ui::GUTTER as usize);

        // 1. Help, or the way out of it.
        let anchor = fit_keys(&[self.console_anchor()], room);
        // 2. The message, up to whatever the help key leaves.
        let message_room = room.saturating_sub(anchor.width() + 2);
        let (message, elapsed) = self.status.line(message_room, self.motion.epoch, now);
        let mut message_width = message.width();
        // The text is the span after the glyph; when it was cut, the whole
        // message is a double-click away.
        if let Some(notice) = self.status.current()
            && message
                .spans
                .get(1)
                .is_some_and(|text| text.content != notice.text)
        {
            ui::reveal(
                Rect {
                    x: row.x + prefix as u16,
                    width: message_width as u16,
                    ..row
                },
                "Message",
                &notice.text,
            );
        }
        left.extend(message.spans);
        if !elapsed.spans.is_empty() {
            message_width += 2 + elapsed.width();
            left.push(Span::raw("  "));
            left.extend(elapsed.spans);
        }
        // 3. The other keys, in what is left.
        let keys_room = message_room.saturating_sub(message_width + 3);
        let mut right = fit_keys(&self.console_keys(), keys_room).spans;
        if !right.is_empty() {
            right.push(Span::raw("  "));
        }
        right.extend(anchor.spans);

        let inner = Rect {
            width: row.width.saturating_sub(ui::GUTTER),
            ..row
        };
        ui::row(frame, inner, Line::from(left), Line::from(right));
        self.text_regions.push(Rect {
            x: row.x + prefix as u16,
            width: message_width as u16,
            ..row
        });
    }

    fn mode_badge(&self) -> (String, theme::Rgb) {
        if self.show_help {
            return ("HELP".to_string(), theme::p().accent);
        }
        self.mode_name()
    }

    /// The mode under any help sheet: its name and colour.
    pub(in crate::helper) fn mode_name(&self) -> (String, theme::Rgb) {
        let p = theme::p();
        if self.pane_dialog.is_some() {
            return ("VIEW".to_string(), p.accent);
        }
        if self.show_log {
            return ("MESSAGES".to_string(), p.accent);
        }
        let position = || {
            format!(
                "{}/{}",
                self.tc_list.active_area + 1,
                self.tc_list.testcases.len()
            )
        };
        match self.input_mode {
            InputMode::Normal => ("NORMAL".to_string(), p.accent),
            InputMode::Editing => (format!("EDIT {}", position()), p.success),
            InputMode::DeleteSelect => (format!("DELETE {}", position()), p.error),
            InputMode::ConfirmDelete => ("DELETE".to_string(), p.error),
            InputMode::Directory => ("CONSOLE".to_string(), p.info),
            InputMode::Config => ("CONFIG".to_string(), p.warning),
            InputMode::ConfirmSubmit => ("SUBMIT".to_string(), p.purple),
            InputMode::ConfirmQuit => ("QUIT".to_string(), p.warning),
            InputMode::Session => ("SESSION".to_string(), p.info),
        }
    }

    /// The key the console never drops: help, or Esc while help is open.
    fn console_anchor(&self) -> (&'static str, &'static str) {
        if self.show_help {
            ("Esc", "close")
        } else {
            (HELP_KEY, "help")
        }
    }

    /// The keys of what is on screen now, most used first. Help is not in
    /// the list: the console always shows it on its own.
    pub(in crate::helper) fn console_keys(&self) -> Vec<(&'static str, &'static str)> {
        if self.show_help {
            Vec::new()
        } else {
            self.mode_keys()
        }
    }

    /// The keys of the mode under any help sheet: what the help sheet lists.
    pub(in crate::helper) fn mode_keys(&self) -> Vec<(&'static str, &'static str)> {
        if self.pane_dialog.is_some() {
            return vec![
                ("↑↓", "scroll"),
                ("←→", "pan"),
                ("PgUp/PgDn", "page"),
                ("Esc", "close"),
            ];
        }
        if self.show_log {
            return vec![("Esc", "close")];
        }
        match self.input_mode {
            InputMode::Normal if self.running => vec![("Esc", "cancel run")],
            InputMode::Normal if self.active_problem.is_none() => vec![
                ("d", "directory"),
                ("c", "config"),
                ("m", "messages"),
                ("q", "quit"),
            ],
            InputMode::Normal
                if self.tc_list.testcases.is_empty()
                    && self
                        .session
                        .as_ref()
                        .is_some_and(|s| s.alive && !s.stopping) =>
            {
                vec![("Enter", "type"), ("Esc", "stop"), ("r", "restart")]
            }
            InputMode::Normal => vec![
                ("r", "run"),
                ("s", "submit"),
                ("e", "edit"),
                ("a", "add"),
                ("x", "delete"),
                ("d", "directory"),
                ("c", "config"),
                ("m", "messages"),
                ("q", "quit"),
            ],
            InputMode::Editing if self.adding_case => {
                vec![("Tab", "input ⇄ expected"), ("Esc", "save")]
            }
            InputMode::Editing => vec![("Tab/⇧Tab", "pane"), ("Esc", "save")],
            InputMode::DeleteSelect => vec![
                ("Tab/⇧Tab", "select"),
                ("Enter", "confirm"),
                ("Esc", "cancel"),
            ],
            InputMode::ConfirmDelete => vec![("Enter", "delete"), ("Esc", "back")],
            InputMode::Directory => vec![
                ("Enter", "run"),
                ("Tab", "complete"),
                ("↑↓", "history"),
                ("Esc", "close"),
            ],
            InputMode::Config => self.config_menu.key_hints().to_vec(),
            InputMode::ConfirmSubmit => vec![("Enter", "submit"), ("Esc", "cancel")],
            InputMode::ConfirmQuit => vec![("Enter", "quit anyway"), ("Esc", "stay")],
            InputMode::Session => vec![
                ("Enter", "send"),
                ("Ctrl-D", "EOF"),
                ("Ctrl-C", "interrupt"),
                ("PgUp/PgDn", "scroll"),
                ("Esc", "back"),
            ],
        }
    }
}

/// A row's text area: the 2-cell inset every row in the frame shares.
fn inset(row: Rect) -> Rect {
    Rect {
        x: row.x + ui::GUTTER,
        width: row.width.saturating_sub(ui::GUTTER * 2),
        ..row
    }
}

/// `[r] run  [s] submit`, cut to `width`: keys drop from the right, and
/// when not even one fits the line is empty.
pub(in crate::helper) fn fit_keys(pairs: &[(&str, &str)], width: usize) -> Line<'static> {
    let mut spans = Vec::new();
    let mut used = 0;
    for (key, label) in pairs {
        let gap = if spans.is_empty() { 0 } else { 2 };
        let cell = [
            ui::key(key),
            Span::styled(format!(" {label}"), Style::new().fg(theme::muted())),
        ];
        let cell_width: usize = cell.iter().map(Span::width).sum();
        if used + gap + cell_width > width {
            break;
        }
        if gap > 0 {
            spans.push(Span::raw("  "));
        }
        spans.extend(cell);
        used += gap + cell_width;
    }
    Line::from(spans)
}

/// The header's wordmark: `zch` on a pill tinted with the theme's signature
/// colour, each letter taking its own stop of the splash gradient, then the
/// version, quiet. It echoes the splash logo in one row.
fn wordmark() -> Vec<Span<'static>> {
    let p = theme::p();
    let pill = theme::paint(theme::mix(p.surface, p.signature[0], 0.22));
    let mut spans = vec![
        Span::styled("", Style::new().fg(pill)),
        Span::styled(" ", Style::new().bg(pill)),
    ];
    for (index, letter) in ["z", "c", "h"].into_iter().enumerate() {
        let color = theme::gradient(&p.signature, index as f32 / 2.0);
        spans.push(Span::styled(
            letter,
            Style::new().fg(theme::paint(color)).bg(pill).bold(),
        ));
    }
    spans.push(Span::styled(" ", Style::new().bg(pill)));
    spans.push(Span::styled("", Style::new().fg(pill)));
    spans.push(Span::styled(
        format!("  v{}", env!("CARGO_PKG_VERSION")),
        Style::new().fg(theme::faint()),
    ));
    spans
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_drop_from_the_right_and_can_vanish() {
        let pairs = [("r", "run"), ("s", "submit"), ("e", "edit")];
        assert_eq!(
            fit_keys(&pairs, 200).to_string(),
            "[r] run  [s] submit  [e] edit"
        );
        assert_eq!(fit_keys(&pairs, 20).to_string(), "[r] run  [s] submit");
        assert_eq!(fit_keys(&pairs, 6).to_string(), "");
    }
}
