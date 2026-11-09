//! The viewer a double-click opens on anything drawn cut short — a case's
//! box or an error alike — with scrolling, panning and huge outputs read
//! page by page. Text that fits opens nothing: there is nothing more to see.

use ratatui::crossterm::event::MouseEvent;

use super::*;
use crate::components::ui;

pub(super) enum DialogContent {
    Lines(Vec<Line<'static>>),
    Paged {
        case_index: usize,
        version: u64,
        line_count: usize,
        page_start: usize,
        page: Vec<Line<'static>>,
        warning: bool,
    },
}

impl DialogContent {
    pub(super) fn total_lines(&self) -> usize {
        match self {
            Self::Lines(lines) => lines.len(),
            Self::Paged {
                line_count,
                warning,
                ..
            } => line_count + usize::from(*warning),
        }
    }

    pub(super) fn max_width(&self) -> usize {
        match self {
            Self::Lines(lines) => lines.iter().map(Line::width).max().unwrap_or(0),
            Self::Paged {
                page,
                warning,
                page_start,
                ..
            } => {
                let warning_width = if *warning && *page_start == 0 {
                    Line::from(PAGED_OUTPUT_NOTE).width()
                } else {
                    0
                };
                page.iter()
                    .map(Line::width)
                    .chain(std::iter::once(warning_width))
                    .max()
                    .unwrap_or(0)
            }
        }
    }
}

/// What the last click landed on, so the next one can tell a double-click.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ClickTarget {
    Pane(usize, PaneKind),
    Text(Rect),
}

const DOUBLE_CLICK: Duration = Duration::from_millis(450);

pub(super) struct PaneDialog {
    pub(super) title: String,
    /// The case box this shows, if it is one; any other text is a copy.
    pub(super) source: Option<(usize, PaneKind)>,
    pub(super) content: DialogContent,
    pub(super) scroll: usize,
    pub(super) h_scroll: usize,
    pub(super) viewport: u16,
    pub(super) viewport_width: u16,
    pub(super) v_scroll_area: Rect,
    pub(super) v_scroll_drag: Option<ScrollbarDrag>,
    pub(super) h_scroll_area: Rect,
    pub(super) h_scroll_drag: Option<ScrollbarDrag>,
}

#[cfg(test)]
impl PaneDialog {
    pub(super) fn visible_lines(&self) -> &[Line<'static>] {
        match &self.content {
            DialogContent::Lines(lines) => lines,
            DialogContent::Paged { page, .. } => page,
        }
    }

    pub(super) fn total_lines(&self) -> usize {
        self.content.total_lines()
    }
}

pub(super) fn paged_warning_line(text: &'static str) -> Line<'static> {
    Line::from(vec![
        Span::styled("! ", Style::new().fg(theme::warning()).bold()),
        Span::styled(text, Style::new().fg(theme::muted())),
    ])
}

pub(super) fn sync_paged_page(content: &mut DialogContent, tc_list: &TestCaseList, scroll: usize) {
    let DialogContent::Paged {
        case_index,
        page_start,
        page,
        warning,
        ..
    } = content
    else {
        return;
    };
    let warning_rows = usize::from(*warning);
    let file_scroll = scroll.saturating_sub(warning_rows);
    let wanted = (file_scroll / GOT_PAGE_LINES) * GOT_PAGE_LINES;
    if *page_start != wanted {
        *page = tc_list
            .testcases
            .get(*case_index)
            .map(|tc| tc.got_page_lines(wanted, GOT_PAGE_LINES))
            .unwrap_or_default();
        *page_start = wanted;
    }
}

impl Helper {
    pub(super) fn open_pane_dialog(&mut self, index: usize, pane: PaneKind) {
        let Some(tc) = self.tc_list.testcases.get(index) else {
            return;
        };
        let label = match pane {
            PaneKind::Input => "input".to_string(),
            PaneKind::Expected => "expected".to_string(),
            PaneKind::Got if matches!(tc.verdict, CaseVerdict::Pending) => "got".to_string(),
            PaneKind::Got => format!("got · {}", tc.verdict.label(false)),
        };
        let content = if pane == PaneKind::Got && tc.got_is_paged() {
            DialogContent::Paged {
                case_index: index,
                version: tc.got_version(),
                line_count: tc.got_line_count(),
                page_start: 0,
                page: tc.got_page_lines(0, GOT_PAGE_LINES),
                warning: true,
            }
        } else {
            let (mut lines, warning) = match pane {
                PaneKind::Got => tc.got_dialog_view(),
                PaneKind::Input | PaneKind::Expected => {
                    let text = tc.pane_text(pane);
                    let lines = if text.is_empty() {
                        vec![Line::styled("empty", Style::new().fg(theme::faint()))]
                    } else {
                        text.split('\n')
                            .map(|line| Line::from(line.to_string()))
                            .collect()
                    };
                    (lines, None)
                }
            };
            if let Some(warning) = warning {
                lines.insert(0, paged_warning_line(warning));
            }
            DialogContent::Lines(lines)
        };
        self.show_dialog(
            format!("Case {} · {label}", index + 1),
            Some((index, pane)),
            content,
        );
    }

    /// Opens the viewer on a copy of `text`.
    pub(super) fn open_text_dialog(&mut self, title: &str, text: &str) {
        let lines = text
            .split('\n')
            .map(|line| Line::from(line.to_string()))
            .collect();
        self.show_dialog(title.to_string(), None, DialogContent::Lines(lines));
    }

    fn show_dialog(
        &mut self,
        title: String,
        source: Option<(usize, PaneKind)>,
        content: DialogContent,
    ) {
        self.pane_dialog = Some(PaneDialog {
            title,
            source,
            content,
            scroll: 0,
            h_scroll: 0,
            viewport: 10,
            viewport_width: 10,
            v_scroll_area: Rect::default(),
            v_scroll_drag: None,
            h_scroll_area: Rect::default(),
            h_scroll_drag: None,
        });
        self.last_click = None;
    }

    /// A second click on the same cut-short text opens it whole. A box
    /// counts only while nothing covers it and its content did not fit;
    /// any other cut text registered itself while drawing. Returns whether
    /// the click opened the viewer.
    pub(super) fn open_on_double_click(&mut self, mouse: &MouseEvent) -> bool {
        if mouse.kind != MouseEventKind::Down(MouseButton::Left) {
            return false;
        }
        let reveal = ui::reveal_at(Position::new(mouse.column, mouse.row));
        let target = match &reveal {
            Some(reveal) => Some(ClickTarget::Text(reveal.area)),
            None if self.open_sheet().is_none()
                && matches!(self.input_mode, InputMode::Normal | InputMode::Editing) =>
            {
                self.tc_list
                    .pane_at(mouse.column, mouse.row)
                    .filter(|(index, pane)| {
                        self.tc_list
                            .testcases
                            .get(*index)
                            .is_some_and(|tc| tc.pane_clipped(*pane))
                    })
                    .map(|(index, pane)| ClickTarget::Pane(index, pane))
            }
            None => None,
        };
        let double = target.is_some()
            && self
                .last_click
                .is_some_and(|(at, last)| Some(last) == target && at.elapsed() < DOUBLE_CLICK);
        self.last_click = target.map(|target| (Instant::now(), target));
        if !double {
            return false;
        }
        match (target, reveal) {
            (Some(ClickTarget::Pane(index, pane)), _) => self.open_pane_dialog(index, pane),
            (_, Some(reveal)) => self.open_text_dialog(&reveal.title, &reveal.text),
            _ => return false,
        }
        true
    }

    pub(super) fn handle_pane_dialog_event(&mut self, event: Event) {
        match event {
            Event::Key(key) => match key.code {
                KeyCode::Esc => {
                    self.pane_dialog = None;
                }
                KeyCode::Up => self.scroll_pane_dialog(-1),
                KeyCode::Down => self.scroll_pane_dialog(1),
                KeyCode::Left => self.scroll_pane_dialog_horizontal(-1),
                KeyCode::Right => self.scroll_pane_dialog_horizontal(1),
                KeyCode::PageUp => {
                    let page = self
                        .pane_dialog
                        .as_ref()
                        .map(|dialog| dialog.viewport.max(1) as i32)
                        .unwrap_or(1);
                    self.scroll_pane_dialog(-page);
                }
                KeyCode::PageDown => {
                    let page = self
                        .pane_dialog
                        .as_ref()
                        .map(|dialog| dialog.viewport.max(1) as i32)
                        .unwrap_or(1);
                    self.scroll_pane_dialog(page);
                }
                KeyCode::Home => self.set_pane_dialog_scroll(0),
                KeyCode::End => self.set_pane_dialog_scroll(usize::MAX),
                _ => {}
            },
            Event::Mouse(mouse) => match mouse.kind {
                MouseEventKind::ScrollDown => self.scroll_pane_dialog(1),
                MouseEventKind::ScrollUp => self.scroll_pane_dialog(-1),
                MouseEventKind::ScrollLeft => self.scroll_pane_dialog_horizontal(-1),
                MouseEventKind::ScrollRight => self.scroll_pane_dialog_horizontal(1),
                MouseEventKind::Down(MouseButton::Left) => {
                    let position = Position::new(mouse.column, mouse.row);
                    let vertical = self.pane_dialog.as_ref().and_then(|dialog| {
                        dialog.v_scroll_area.contains(position).then_some((
                            dialog.v_scroll_area,
                            dialog
                                .content
                                .total_lines()
                                .saturating_sub(dialog.viewport.max(1) as usize),
                            dialog.scroll,
                        ))
                    });
                    if let Some((area, max_scroll, scroll)) = vertical {
                        if let Some((drag, scroll)) =
                            ScrollbarDrag::begin(area, true, max_scroll, scroll, position)
                            && let Some(dialog) = self.pane_dialog.as_mut()
                        {
                            dialog.v_scroll_drag = Some(drag);
                            dialog.h_scroll_drag = None;
                            dialog.scroll = scroll;
                        }
                        return;
                    }

                    let horizontal = self.pane_dialog.as_ref().and_then(|dialog| {
                        dialog.h_scroll_area.contains(position).then_some((
                            dialog.h_scroll_area,
                            dialog
                                .content
                                .max_width()
                                .saturating_sub(dialog.viewport_width.max(1) as usize),
                            dialog.h_scroll,
                        ))
                    });
                    if let Some((area, max_scroll, scroll)) = horizontal {
                        if let Some((drag, scroll)) =
                            ScrollbarDrag::begin(area, false, max_scroll, scroll, position)
                            && let Some(dialog) = self.pane_dialog.as_mut()
                        {
                            dialog.h_scroll_drag = Some(drag);
                            dialog.v_scroll_drag = None;
                            dialog.h_scroll = scroll;
                        }
                        return;
                    }

                    if let Some(dialog) = self.pane_dialog.as_mut() {
                        dialog.v_scroll_drag = None;
                        dialog.h_scroll_drag = None;
                    }
                }
                MouseEventKind::Drag(MouseButton::Left) => {
                    let position = Position::new(mouse.column, mouse.row);
                    if let Some(dialog) = self.pane_dialog.as_mut() {
                        if let Some(drag) = dialog.v_scroll_drag {
                            dialog.scroll = drag.scroll_at(position);
                        } else if let Some(drag) = dialog.h_scroll_drag {
                            dialog.h_scroll = drag.scroll_at(position);
                        }
                    }
                }
                MouseEventKind::Up(MouseButton::Left) => {
                    if let Some(dialog) = self.pane_dialog.as_mut() {
                        dialog.v_scroll_drag = None;
                        dialog.h_scroll_drag = None;
                    }
                }
                _ => {}
            },
            _ => {}
        }
    }

    pub(super) fn scroll_pane_dialog(&mut self, rows: i32) {
        let Some(dialog) = self.pane_dialog.as_mut() else {
            return;
        };
        let max = dialog
            .content
            .total_lines()
            .saturating_sub(dialog.viewport.max(1) as usize);
        dialog.scroll = (dialog.scroll as i64 + rows as i64).clamp(0, max as i64) as usize;
    }

    pub(super) fn set_pane_dialog_scroll(&mut self, scroll: usize) {
        let Some(dialog) = self.pane_dialog.as_mut() else {
            return;
        };
        let max = dialog
            .content
            .total_lines()
            .saturating_sub(dialog.viewport.max(1) as usize);
        dialog.scroll = scroll.min(max);
    }

    pub(super) fn scroll_pane_dialog_horizontal(&mut self, columns: i32) {
        let Some(dialog) = self.pane_dialog.as_mut() else {
            return;
        };
        let max = dialog
            .content
            .max_width()
            .saturating_sub(dialog.viewport_width.max(1) as usize);
        dialog.h_scroll = (dialog.h_scroll as i64 + columns as i64).clamp(0, max as i64) as usize;
    }
}
