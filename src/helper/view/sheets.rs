//! Sheets: help, the message log, submit, delete, quit and the pane viewer.

use super::*;

impl Helper {
    /// Help for what is open right now: only that mode's keys, under the
    /// mode's name, so the sheet never lists keys that do nothing here.
    pub(in crate::helper) fn draw_help(&self, frame: &mut Frame, area: Rect, opened: f32) -> Rect {
        let (mode, _) = self.mode_name();
        let keys = self.mode_keys();
        let key_width = keys
            .iter()
            .map(|(key, _)| ui::key(key).width())
            .max()
            .unwrap_or(0);
        let label_width = keys
            .iter()
            .map(|(_, label)| label.chars().count())
            .max()
            .unwrap_or(0);
        let width = (key_width + label_width + 9).max(30) as u16;
        let height = keys.len() as u16 + 4;
        let body = ui::sheet(
            frame,
            area,
            &format!("Keys · {}", mode.to_lowercase()),
            width,
            height,
            opened,
        );

        let lines: Vec<Line> = keys
            .iter()
            .map(|(key, label)| {
                let cap = ui::key(key);
                let pad = key_width.saturating_sub(cap.width()) + 3;
                Line::from(vec![
                    cap,
                    Span::raw(" ".repeat(pad)),
                    Span::styled(label.to_string(), Style::new().fg(theme::text())),
                ])
            })
            .collect();
        frame.render_widget(Paragraph::new(lines), body);
        body
    }

    pub(in crate::helper) fn draw_log(
        &self,
        frame: &mut Frame,
        area: Rect,
        opened: f32,
        now: Instant,
    ) -> Rect {
        let count = self.status.log().len();
        let height = (count.max(1) as u16 + 4).min(area.height.saturating_sub(4));
        let width = (area.width * 8 / 10).min(96);
        let body = ui::sheet(frame, area, "Messages", width, height, opened);
        if count == 0 {
            frame.render_widget(
                Paragraph::new(Span::styled(
                    "nothing yet — messages from this session collect here",
                    Style::new().fg(theme::faint()),
                )),
                body,
            );
            return body;
        }
        let visible = body.height as usize;
        for (row, notice) in self
            .status
            .log()
            .rev()
            .take(visible)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .enumerate()
        {
            let age = format!(
                "{} ago",
                format_elapsed(now.saturating_duration_since(notice.at))
            );
            let room = (body.width as usize).saturating_sub(age.len() + 4);
            let glyph = match notice.tone {
                Tone::Progress => "·",
                tone => tone.glyph(self.motion.epoch, now),
            };
            let line = Rect {
                y: body.y + row as u16,
                height: 1,
                ..body
            };
            ui::row(
                frame,
                line,
                Line::from(vec![
                    Span::styled(
                        format!("{glyph} "),
                        Style::new().fg(notice.tone.color()).bold(),
                    ),
                    Span::styled(
                        ui::truncate_revealing(&notice.text, room, line, "Message"),
                        Style::new().fg(theme::text()),
                    ),
                ]),
                Line::from(Span::styled(age, Style::new().fg(theme::faint()))),
            );
        }
        body
    }

    pub(in crate::helper) fn draw_submit_sheet(
        &self,
        frame: &mut Frame,
        area: Rect,
        opened: f32,
    ) -> Rect {
        let Some(pending) = &self.pending_submit else {
            return Rect::default();
        };
        let info = &pending.info;
        let body = ui::sheet(frame, area, "Submit", 68, 14, opened);
        let width = body.width.saturating_sub(12) as usize;
        // Each field sits on its own row of the body: a cut value is a
        // double-click away there.
        let shown = |row: u16, label: &str, text: &str| {
            let line = Rect {
                y: body.y + row,
                height: 1,
                ..body
            };
            ui::truncate_revealing(text, width, line.intersection(body), label)
        };
        let value = |row: u16, label: &str, text: String| {
            vec![Span::styled(
                shown(row, label, &text),
                Style::new().fg(theme::text()),
            )]
        };
        let quiet = |row: u16, label: &str, text: String| {
            vec![Span::styled(
                shown(row, label, &text),
                Style::new().fg(theme::muted()),
            )]
        };

        let problem = self
            .active_problem
            .as_ref()
            .map(|problem| problem.name.clone())
            .unwrap_or_default();
        let (tone, tests) = match &pending.tests {
            LocalTests::Passed { total } => (Tone::Success, format!("{total} of {total} passed")),
            LocalTests::Failed { failed, total } => (
                Tone::Error,
                format!("{failed} of {total} failed on the last run"),
            ),
            LocalTests::CompileError => {
                (Tone::Error, "did not compile on the last run".to_string())
            }
            LocalTests::Stale => (
                Tone::Warning,
                "the file changed since the last run".to_string(),
            ),
            LocalTests::NotRun { total } => (
                Tone::Warning,
                format!(
                    "not run yet · {total} case{}",
                    if *total == 1 { "" } else { "s" }
                ),
            ),
            LocalTests::NoCases => (Tone::Info, "no local cases".to_string()),
        };
        let browser = if info.safari_available {
            quiet(
                9,
                "Browser",
                format!("{} · opens in the background", info.browser),
            )
        } else {
            vec![
                Span::styled("✗ ", Style::new().fg(theme::error()).bold()),
                Span::styled(
                    format!("{} is not installed", info.browser),
                    Style::new().fg(theme::text()),
                ),
            ]
        };
        let mut tests_spans = vec![Span::styled(
            format!("{} ", tone.glyph(self.motion.epoch, Instant::now())),
            Style::new().fg(tone.color()).bold(),
        )];
        tests_spans.push(Span::styled(tests, Style::new().fg(theme::text())));

        let lines = vec![
            ui::field("problem", 12, value(0, "Problem", problem)),
            ui::field(
                "target",
                12,
                value(
                    1,
                    "Target",
                    format!("{} · {}", info.platform, info.problem_code),
                ),
            ),
            ui::field("page", 12, quiet(2, "Page", info.submit_url.clone())),
            ui::field("file", 12, value(3, "File", info.file_name.clone())),
            ui::field(
                "language",
                12,
                value(
                    4,
                    "Language",
                    format!("{} · id {}", info.language, info.language_id),
                ),
            ),
            ui::field(
                "time limit",
                12,
                quiet(5, "Time limit", format_ms(u128::from(pending.time_limit))),
            ),
            Line::default(),
            ui::field("local tests", 12, tests_spans),
            ui::field("browser", 12, browser),
        ];
        frame.render_widget(Paragraph::new(lines), body);
        body
    }

    pub(in crate::helper) fn draw_delete_sheet(
        &self,
        frame: &mut Frame,
        area: Rect,
        opened: f32,
    ) -> Rect {
        let index = self.tc_list.active_area;
        let Some(tc) = self.tc_list.testcases.get(index) else {
            return Rect::default();
        };
        let body = ui::sheet_with(
            frame,
            area,
            &format!("Delete case {}", index + 1),
            theme::p().error,
            56,
            9,
            opened,
        );
        let width = body.width.saturating_sub(10) as usize;
        let preview = |row: u16, label: &str, text: String| -> Vec<Span<'static>> {
            let mut lines = text.lines();
            let first = lines.next().unwrap_or("").trim().to_string();
            let more = lines.count();
            if first.is_empty() && more == 0 {
                return vec![Span::styled("empty", Style::new().fg(theme::faint()))];
            }
            let line = Rect {
                y: body.y + row,
                height: 1,
                ..body
            }
            .intersection(body);
            let shown = ui::truncate(&first, width.saturating_sub(12));
            if more > 0 || shown != first {
                ui::reveal(line, label, &text);
            }
            let mut spans = vec![Span::styled(shown, Style::new().fg(theme::text()))];
            if more > 0 {
                spans.push(Span::styled(
                    format!("  +{more} line{}", if more == 1 { "" } else { "s" }),
                    Style::new().fg(theme::faint()),
                ));
            }
            spans
        };
        let lines = vec![
            ui::field(
                "input",
                10,
                preview(0, "Input", tc.inp_content.lines().join("\n")),
            ),
            ui::field(
                "expected",
                10,
                preview(1, "Expected", tc.exp_content.lines().join("\n")),
            ),
            Line::default(),
            Line::from(Span::styled(
                "this can't be undone",
                Style::new().fg(theme::muted()),
            )),
        ];
        frame.render_widget(Paragraph::new(lines), body);
        body
    }

    pub(in crate::helper) fn draw_quit_sheet(
        &self,
        frame: &mut Frame,
        area: Rect,
        opened: f32,
    ) -> Rect {
        let pending = self.submissions.iter().filter(|s| !s.done()).count();
        let body = ui::sheet_with(frame, area, "Quit?", theme::p().warning, 56, 7, opened);
        let lines = vec![
            Line::from(vec![
                Span::styled("! ", Style::new().fg(theme::warning()).bold()),
                Span::styled(
                    format!(
                        "{pending} submission{} still being judged",
                        if pending == 1 { " is" } else { "s are" }
                    ),
                    Style::new().fg(theme::text()),
                ),
            ]),
            Line::from(Span::styled(
                "quitting loses the verdict here; Safari keeps the page",
                Style::new().fg(theme::muted()),
            )),
        ];
        frame.render_widget(Paragraph::new(lines), body);
        body
    }

    pub(in crate::helper) fn draw_pane_dialog(
        &mut self,
        frame: &mut Frame,
        area: Rect,
        opened: f32,
    ) -> Rect {
        let Helper {
            pane_dialog,
            tc_list,
            ..
        } = self;
        let Some(dialog) = pane_dialog.as_mut() else {
            return Rect::default();
        };

        let width = area.width.saturating_sub(8).max(20).min(area.width);
        let height = area.height.saturating_sub(4).max(8).min(area.height);
        let body = ui::sheet(frame, area, &dialog.title, width, height, opened);
        if body.height == 0 || body.width == 0 {
            return Rect::default();
        }

        if let DialogContent::Paged {
            case_index,
            version,
            line_count,
            page_start,
            warning,
            ..
        } = &mut dialog.content
            && let Some(tc) = tc_list.testcases.get(*case_index)
            && tc.got_version() != *version
        {
            *version = tc.got_version();
            *line_count = tc.got_line_count();
            *warning = tc.got_is_paged();
            *page_start = usize::MAX;
        }

        let total_lines = dialog.content.total_lines();
        dialog.scroll = dialog
            .scroll
            .min(total_lines.saturating_sub(body.height as usize));
        sync_paged_page(&mut dialog.content, tc_list, dialog.scroll);

        let content_width = dialog.content_width();
        let mut vscroll = total_lines > body.height as usize;
        let mut usable_width = body.width.saturating_sub(u16::from(vscroll) * 2);
        let mut hscroll = content_width > usable_width as usize;
        let mut content_height = body.height.saturating_sub(u16::from(hscroll) * 2);
        vscroll = total_lines > content_height as usize;
        usable_width = body.width.saturating_sub(u16::from(vscroll) * 2);
        if !hscroll && content_width > usable_width as usize {
            hscroll = true;
            content_height = body.height.saturating_sub(2);
            vscroll = total_lines > content_height as usize;
            usable_width = body.width.saturating_sub(u16::from(vscroll) * 2);
        }
        dialog.viewport = content_height.max(1);
        dialog.viewport_width = usable_width.max(1);
        dialog.h_scroll = dialog
            .h_scroll
            .min(content_width.saturating_sub(usable_width as usize));

        let max_scroll = total_lines.saturating_sub(dialog.viewport as usize);
        dialog.scroll = dialog.scroll.min(max_scroll);

        let scroll = dialog.scroll;
        let h_scroll = dialog.h_scroll;
        sync_paged_page(&mut dialog.content, tc_list, scroll);
        let display_lines = dialog.visible(scroll, content_height as usize);

        let content_area = Rect {
            width: usable_width,
            height: content_height,
            ..body
        };
        frame.render_widget(
            Paragraph::new(Text::from(display_lines))
                .style(Style::new().fg(theme::text()))
                .scroll((0, h_scroll.min(u16::MAX as usize) as u16)),
            content_area,
        );

        dialog.v_scroll_area = Rect::default();
        if vscroll {
            let mut state =
                ScrollbarState::new(total_lines - dialog.viewport as usize).position(dialog.scroll);
            let scroll_area = Rect {
                x: body.right().saturating_sub(1),
                y: content_area.y,
                width: 1,
                height: content_height.max(1),
            };
            dialog.v_scroll_area = scroll_area;
            frame.render_stateful_widget(
                scrollbar(ScrollbarOrientation::VerticalRight),
                scroll_area,
                &mut state,
            );
        } else {
            dialog.v_scroll_drag = None;
        }

        dialog.h_scroll_area = Rect::default();
        if hscroll {
            let max_h_scroll = content_width.saturating_sub(usable_width as usize);
            let mut state = ScrollbarState::new(max_h_scroll).position(dialog.h_scroll);
            let scroll_area = Rect {
                x: body.x,
                y: body.bottom().saturating_sub(1),
                width: usable_width,
                height: 1,
            };
            dialog.h_scroll_area = scroll_area;
            frame.render_stateful_widget(
                scrollbar(ScrollbarOrientation::HorizontalBottom),
                scroll_area,
                &mut state,
            );
        } else {
            dialog.h_scroll_drag = None;
        }
        content_area
    }
}
