//! The interactive transcript shown in place of the case list.

use super::*;

impl Helper {
    /// The case-list area when a problem has no samples: a live transcript.
    pub(in crate::helper) fn draw_session(&self, frame: &mut Frame, area: Rect, now: Instant) {
        if area.width == 0 || area.height == 0 {
            return;
        }
        let session = self.session.as_ref();
        let alive = session.is_some_and(|session| session.alive);
        let stopping = session.is_some_and(|session| session.stopping);
        let (state, state_color) = if stopping {
            ("stopping".to_string(), theme::warning())
        } else if alive {
            (
                format!("{} running", anim::spinner(self.motion.epoch, now)),
                theme::accent(),
            )
        } else if session.is_some() {
            ("stopped".to_string(), theme::muted())
        } else {
            ("no samples".to_string(), theme::faint())
        };
        let state = if session.is_some_and(|session| session.scroll > 0) {
            format!("scrolled · {state}")
        } else {
            state
        };
        ui::row(
            frame,
            Rect { height: 1, ..area },
            Line::from(Span::styled(
                "interactive",
                Style::new().fg(theme::text()).bold(),
            )),
            Line::from(Span::styled(state, Style::new().fg(state_color))),
        );
        if area.height < 3 {
            return;
        }

        let show_input = alive && !stopping;
        let input_rows = if show_input { 2 } else { 0 };
        let body = Rect {
            y: area.y + 2,
            height: area.height.saturating_sub(2 + input_rows),
            ..area
        };

        let mut rows: Vec<Line> = Vec::new();
        if let Some(session) = session {
            let has_partial = !session.partial.is_empty();
            let total = session.lines.len() + usize::from(has_partial);
            let room = body.height as usize;
            if room > 0 && total > 0 {
                let max_scroll = total.saturating_sub(room);
                let scroll = session.scroll.min(max_scroll);
                let end = total - scroll;
                let start = end.saturating_sub(room);
                let (start, show_indicator) = if start > 0 && room > 1 {
                    (end.saturating_sub(room - 1), true)
                } else {
                    (start, false)
                };
                if show_indicator {
                    rows.push(Line::from(Span::styled(
                        format!("↑ {start} earlier lines"),
                        Style::new().fg(theme::faint()),
                    )));
                }
                for index in start..end {
                    if let Some(line) = session.lines.get(index) {
                        rows.push(transcript_line(line.kind, &line.text));
                    } else if index == session.lines.len() && has_partial {
                        rows.push(transcript_line(session.partial_kind, &session.partial));
                    }
                }
            }
        }
        if rows.is_empty() && body.height > 0 {
            let hints: &[&str] = if session.is_none() {
                &[
                    "This problem came without samples.",
                    "r runs your program here, and you type its input.",
                ]
            } else if alive {
                &["starting your program…"]
            } else {
                &["r runs it again"]
            };
            for (index, hint) in hints.iter().enumerate() {
                let color = if index == 0 {
                    theme::text()
                } else {
                    theme::muted()
                };
                rows.push(Line::from(Span::styled(*hint, Style::new().fg(color))));
            }
        }
        if body.height > 0 {
            frame.render_widget(Paragraph::new(Text::from(rows)), body);
        }

        if !show_input {
            return;
        }
        let input_row = Rect {
            y: area.bottom() - 1,
            height: 1,
            ..area
        };
        let focused = self.input_mode == InputMode::Session;
        let (input, cursor) = session
            .map(|session| (session.input.as_str(), session.cursor))
            .unwrap_or(("", 0));
        let prompt = format!("{} ", crate::components::icons::PROMPT);
        let room = (input_row.width as usize).saturating_sub(prompt.width());
        let (visible, cursor_cells) = fit_input(input, cursor, room);
        let mut spans = vec![Span::styled(
            prompt.clone(),
            Style::new()
                .fg(if focused {
                    theme::accent()
                } else {
                    theme::faint()
                })
                .bold(),
        )];
        if visible.is_empty() && !focused {
            spans.push(Span::styled(
                "enter to type into the program",
                Style::new().fg(theme::faint()),
            ));
        } else {
            spans.push(Span::styled(visible, Style::new().fg(theme::text())));
        }
        frame.render_widget(Paragraph::new(Line::from(spans)), input_row);
        if focused && input_row.width > prompt.width() as u16 {
            let x = input_row.x + prompt.width() as u16 + cursor_cells as u16;
            frame.set_cursor_position((x.min(input_row.right().saturating_sub(1)), input_row.y));
        }
    }
}

/// One transcript line: your input after the prompt glyph, the program's
/// output in the reading colour, zch's own notes faint, errors in red.
pub(super) fn transcript_line(kind: TranscriptKind, text: &str) -> Line<'static> {
    let (prefix, style) = match kind {
        TranscriptKind::You => (
            format!("{} ", crate::components::icons::PROMPT),
            Style::new().fg(theme::accent()),
        ),
        TranscriptKind::Status => (String::new(), Style::new().fg(theme::faint())),
        TranscriptKind::Out => (String::new(), Style::new().fg(theme::text())),
        TranscriptKind::Err => (String::new(), Style::new().fg(theme::error())),
    };
    Line::from(vec![
        Span::styled(prefix, style.bold()),
        Span::styled(text.to_string(), style),
    ])
}
