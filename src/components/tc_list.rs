use ratatui::{
    Frame,
    buffer::Buffer,
    crossterm::event::{Event, KeyCode, KeyModifiers, MouseButton, MouseEvent, MouseEventKind},
    layout::{
        Constraint::{Length, Percentage},
        Layout, Position, Rect,
    },
    style::Modifier,
    widgets::{ScrollbarOrientation, ScrollbarState},
};

use crate::components::{
    ScrollbarDrag, scrollbar, step_wrapped,
    testcase::{ActiveBox, CaseFocus, PaneKind, TestCase},
};

pub struct TestCaseList {
    pub testcases: Vec<TestCase>,
    pub available_height: usize,
    pub scroll_row: usize,
    pub active_area: usize,
    pub active_box: ActiveBox,
    manual_scroll: bool,
    scrollbar_area: Rect,
    scrollbar_drag: Option<ScrollbarDrag>,
}

impl TestCaseList {
    pub fn new() -> Self {
        Self {
            testcases: Vec::new(),
            available_height: 0,
            scroll_row: 0,
            active_area: 0,
            active_box: ActiveBox::Input,
            manual_scroll: false,
            scrollbar_area: Rect::default(),
            scrollbar_drag: None,
        }
    }

    fn total_rows(&self) -> usize {
        self.testcases.iter().map(|tc| tc.row_height()).sum()
    }

    pub fn clear(&mut self) {
        self.testcases.clear();
        self.active_area = 0;
        self.scroll_row = 0;
        self.available_height = 0;
        self.active_box = ActiveBox::Input;
        self.manual_scroll = false;
        self.scrollbar_area = Rect::default();
        self.scrollbar_drag = None;
    }

    pub fn add_tc(&mut self) {
        self.testcases.push(TestCase::new("", ""));
        self.active_area = self.testcases.len().saturating_sub(1);
        self.active_box = ActiveBox::Input;
        self.manual_scroll = false;
        self.update_scroll();
    }

    pub fn remove_last(&mut self) {
        if self.testcases.is_empty() {
            return;
        }
        self.testcases.pop();
        self.active_area = self.active_area.min(self.testcases.len().saturating_sub(1));
        self.manual_scroll = false;
        self.update_scroll();
    }

    pub fn row_offset_of(&self, active_area: usize) -> usize {
        self.testcases[..active_area.min(self.testcases.len())]
            .iter()
            .map(|tc| tc.row_height())
            .sum()
    }

    pub fn focus_first(&mut self) {
        if !self.testcases.is_empty() {
            self.active_area = 0;
            self.active_box = ActiveBox::Input;
            self.manual_scroll = false;
            self.update_scroll();
        }
    }

    pub fn update_scroll(&mut self) {
        if self.testcases.is_empty() || self.available_height == 0 {
            return;
        }
        let active_area = self.active_area.min(self.testcases.len() - 1);
        let active_top = self.row_offset_of(active_area);
        let active_bottom = active_top + self.testcases[active_area].row_height();

        if active_top < self.scroll_row {
            self.scroll_row = active_top;
        } else if active_bottom > self.scroll_row + self.available_height {
            self.scroll_row = active_bottom.saturating_sub(self.available_height);
        }
    }

    pub fn draw(&mut self, frame: &mut Frame, area: Rect, focus: CaseFocus) {
        if area.height == 0 || area.width == 0 {
            self.available_height = 0;
            self.scrollbar_area = Rect::default();
            self.scrollbar_drag = None;
            for tc in self.testcases.iter_mut() {
                tc.clear_hit_areas();
            }
            return;
        }

        let viewport = area;
        self.available_height = viewport.height as usize;
        for tc in self.testcases.iter_mut() {
            tc.clear_height_caps();
        }
        if focus == CaseFocus::Edit
            && let Some(tc) = self.testcases.get_mut(self.active_area)
        {
            tc.fit_to_height(self.available_height);
            if !self.manual_scroll {
                self.update_scroll();
            }
        }

        let total = self.total_rows();
        let needs_scrollbar = total > self.available_height;

        let (tc_list_area, scrollbar_render_area) = if needs_scrollbar && viewport.width >= 2 {
            let division = Layout::horizontal([Percentage(100), Length(1)]).split(viewport);
            (division[0], division[1])
        } else {
            (viewport, Rect::default())
        };

        self.scrollbar_area = scrollbar_render_area;
        if !needs_scrollbar {
            self.scrollbar_drag = None;
        }

        let scroll_bar = scrollbar(ScrollbarOrientation::VerticalRight);

        self.scroll_row = self
            .scroll_row
            .min(total.saturating_sub(self.available_height));
        for tc in self.testcases.iter_mut() {
            tc.clear_hit_areas();
        }

        let start_row = self.scroll_row;
        let end_row = (start_row + self.available_height).min(total);
        let mut current_row: usize = 0;

        for (idx, tc) in self.testcases.iter_mut().enumerate() {
            let tc_height = tc.row_height();
            let tc_top = current_row;
            let tc_bottom = tc_top + tc_height;

            if tc_bottom <= start_row {
                current_row = tc_bottom;
                continue;
            }
            if tc_top >= end_row {
                break;
            }

            let clip_top = start_row.saturating_sub(tc_top);
            let clip_bottom = (end_row - tc_top).min(tc_height);
            let visible_height = clip_bottom.saturating_sub(clip_top);

            // A partially scrolled case keeps its own layout: draw it whole
            // into a scratch buffer, then blit just the rows on screen.
            let scratch_area = Rect {
                x: 0,
                y: 0,
                width: tc_list_area.width,
                height: tc_height as u16,
            };
            let mut scratch = Buffer::empty(scratch_area);
            let case_focus = if idx == self.active_area {
                focus
            } else {
                CaseFocus::None
            };
            let active_box = if case_focus == CaseFocus::Edit {
                self.active_box
            } else {
                ActiveBox::None
            };
            tc.draw_tc(&mut scratch, scratch_area, idx + 1, active_box, case_focus);
            if focus == CaseFocus::Delete && case_focus == CaseFocus::None {
                for cell in scratch.content.iter_mut() {
                    cell.modifier.insert(Modifier::DIM);
                }
            }

            let dest_x = tc_list_area.x;
            let dest_y = tc_list_area.y + (tc_top.saturating_sub(start_row)) as u16;
            let visible_area = Rect {
                x: dest_x,
                y: dest_y,
                width: tc_list_area.width,
                height: visible_height as u16,
            };
            {
                let dest = frame.buffer_mut();
                for row in 0..visible_height {
                    let src_y = (clip_top + row) as u16;
                    for column in 0..tc_list_area.width {
                        dest[(dest_x + column, dest_y + row as u16)] =
                            scratch[(column, src_y)].clone();
                    }
                }
            }
            tc.relocate_hit_areas(dest_x as i32, dest_y as i32 - clip_top as i32, visible_area);
            current_row = tc_bottom;
        }

        if needs_scrollbar {
            let scroll_range = total.saturating_sub(self.available_height);
            let mut scroll_state = ScrollbarState::new(scroll_range).position(self.scroll_row);
            frame.render_stateful_widget(scroll_bar, scrollbar_render_area, &mut scroll_state);
        }
    }

    pub fn handle_key(&mut self, event: Event, stay_in_case: bool) {
        if let Event::Key(key) = event {
            if self.testcases.is_empty() {
                return;
            }
            if matches!(key.code, KeyCode::Tab | KeyCode::BackTab) {
                let backwards =
                    key.code == KeyCode::BackTab || key.modifiers.contains(KeyModifiers::SHIFT);
                if stay_in_case {
                    self.active_box = match self.active_box {
                        ActiveBox::Input => ActiveBox::Expected,
                        ActiveBox::Expected => ActiveBox::Input,
                        ActiveBox::None => ActiveBox::Input,
                    };
                } else {
                    match (self.active_box, backwards) {
                        (ActiveBox::Input, false) => self.active_box = ActiveBox::Expected,
                        (ActiveBox::Input, true) => {
                            self.active_box = ActiveBox::Expected;
                            self.active_area =
                                step_wrapped(self.active_area, self.testcases.len(), true);
                        }
                        (ActiveBox::Expected, false) => {
                            self.active_box = ActiveBox::Input;
                            self.active_area =
                                step_wrapped(self.active_area, self.testcases.len(), false);
                        }
                        (ActiveBox::Expected, true) => self.active_box = ActiveBox::Input,
                        (ActiveBox::None, _) => self.active_box = ActiveBox::Input,
                    }
                }
                self.manual_scroll = false;
                self.update_scroll();
                return;
            }

            match self.active_box {
                ActiveBox::Expected => {
                    self.testcases[self.active_area].exp_content.input(key);
                }
                ActiveBox::Input => {
                    self.testcases[self.active_area].inp_content.input(key);
                }
                ActiveBox::None => {}
            }
        }
    }

    /// Mouse wheel over the focused editing pane scrolls the textarea itself;
    /// elsewhere it scrolls the case list.
    pub fn scroll_active_pane(&mut self, mouse: &MouseEvent, down: bool) -> bool {
        let position = Position::new(mouse.column, mouse.row);
        let Some(tc) = self.testcases.get_mut(self.active_area) else {
            return false;
        };
        let step = if down { 1 } else { -1 };
        match self.active_box {
            ActiveBox::Input if tc.input_box.rect.contains(position) => {
                tc.inp_content.scroll((step, 0));
                true
            }
            ActiveBox::Expected if tc.expected_box.rect.contains(position) => {
                tc.exp_content.scroll((step, 0));
                true
            }
            _ => false,
        }
    }

    pub fn pane_at(&self, column: u16, row: u16) -> Option<(usize, PaneKind)> {
        let position = Position::new(column, row);
        for (index, tc) in self.testcases.iter().enumerate() {
            if tc.collapsed {
                continue;
            }
            if tc.input_box.rect.contains(position) {
                return Some((index, PaneKind::Input));
            }
            if tc.expected_box.rect.contains(position) {
                return Some((index, PaneKind::Expected));
            }
            if tc.got_box.rect.contains(position) {
                return Some((index, PaneKind::Got));
            }
        }
        None
    }

    pub fn scroll_with_key(&mut self, key: KeyCode) {
        let max_scroll = self.total_rows().saturating_sub(self.available_height);
        let page = self.available_height.saturating_sub(1).max(1);
        self.scroll_row = match key {
            KeyCode::PageDown => self.scroll_row.saturating_add(page).min(max_scroll),
            KeyCode::PageUp => self.scroll_row.saturating_sub(page),
            KeyCode::Home => 0,
            KeyCode::End => max_scroll,
            _ => return,
        };
        self.manual_scroll = true;
        self.scrollbar_drag = None;
    }

    pub fn handle_mouse(&mut self, mouse: &MouseEvent) {
        let position = Position::new(mouse.column, mouse.row);
        let max_scroll = self.total_rows().saturating_sub(self.available_height);
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) if self.scrollbar_area.contains(position) => {
                if let Some((drag, scroll)) = ScrollbarDrag::begin(
                    self.scrollbar_area,
                    true,
                    max_scroll,
                    self.scroll_row,
                    position,
                ) {
                    self.scrollbar_drag = Some(drag);
                    self.scroll_row = scroll;
                    self.manual_scroll = true;
                }
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                if let Some(drag) = self.scrollbar_drag {
                    self.scroll_row = drag.scroll_at(position);
                    self.manual_scroll = true;
                }
            }
            MouseEventKind::Up(MouseButton::Left) => self.scrollbar_drag = None,
            MouseEventKind::ScrollDown => {
                self.scrollbar_drag = None;
                let max_scroll = self.total_rows().saturating_sub(self.available_height);
                self.scroll_row = self.scroll_row.saturating_add(1).min(max_scroll);
                self.manual_scroll = true;
            }
            MouseEventKind::ScrollUp => {
                self.scrollbar_drag = None;
                self.scroll_row = self.scroll_row.saturating_sub(1);
                self.manual_scroll = true;
            }
            MouseEventKind::Down(MouseButton::Left) => {
                self.scrollbar_drag = None;
                for (idx, tc) in self.testcases.iter_mut().enumerate() {
                    if tc.handle_mouse(mouse) {
                        if !tc.collapse_toggle_area.check_hover(mouse) {
                            self.active_area = idx;
                            self.manual_scroll = false;
                            // Clicking a pane in edit mode moves the focus there.
                            if tc.input_box.check_hover(mouse) {
                                self.active_box = ActiveBox::Input;
                            } else if tc.expected_box.check_hover(mouse) {
                                self.active_box = ActiveBox::Expected;
                            }
                        }
                        break;
                    }
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::testcase::CaseVerdict;
    use ratatui::{Terminal, backend::TestBackend};

    fn mouse(kind: MouseEventKind, column: u16, row: u16) -> MouseEvent {
        MouseEvent {
            kind,
            column,
            row,
            modifiers: KeyModifiers::NONE,
        }
    }

    #[test]
    fn keyboard_scroll_pages_and_stays_within_bounds() {
        let mut list = TestCaseList::new();
        for _ in 0..10 {
            list.testcases.push(TestCase::new("1", "2"));
        }
        let mut terminal = Terminal::new(TestBackend::new(60, 12)).unwrap();
        terminal
            .draw(|frame| list.draw(frame, frame.area(), CaseFocus::None))
            .unwrap();
        list.scroll_with_key(KeyCode::PageDown);
        assert_eq!(list.scroll_row, 11);
        terminal
            .draw(|frame| list.draw(frame, frame.area(), CaseFocus::None))
            .unwrap();
        assert_eq!(list.scroll_row, 11);
        list.scroll_with_key(KeyCode::End);
        let end = list.total_rows().saturating_sub(list.available_height);
        assert_eq!(list.scroll_row, end);
        list.scroll_with_key(KeyCode::PageDown);
        assert_eq!(list.scroll_row, end);
        list.scroll_with_key(KeyCode::Home);
        list.scroll_with_key(KeyCode::PageUp);
        assert_eq!(list.scroll_row, 0);
    }

    #[test]
    fn tall_cases_scroll_through_their_full_height() {
        // A case taller than the list viewport, partially scrolled: it must
        // show its real middle rows, not a squashed copy of its top.
        let mut list = TestCaseList::new();
        let mut tc = TestCase::new("1", "");
        let body = (0..200)
            .map(|i| format!("line-{i:03}"))
            .collect::<Vec<_>>()
            .join("\n");
        tc.set_result(CaseVerdict::WA { ms: 1 }, body);
        list.testcases.push(tc);

        let mut terminal = Terminal::new(TestBackend::new(60, 12)).unwrap();
        list.scroll_row = 150;
        terminal
            .draw(|frame| list.draw(frame, frame.area(), CaseFocus::None))
            .unwrap();

        let buffer = terminal.backend().buffer();
        let screen = (0..12)
            .map(|y| (0..60).map(|x| buffer[(x, y)].symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n");
        if std::env::var("ZCH_DUMP").is_ok() {
            println!("{screen}");
        }
        assert!(
            screen.contains("line-145"),
            "mid-case rows are visible:\n{screen}"
        );
        assert!(
            !screen.contains("line-000"),
            "the case top is scrolled away:\n{screen}"
        );
    }
    #[test]
    fn scrollbar_drag_scrolls_the_case_list() {
        let mut list = TestCaseList::new();
        for index in 0..6 {
            list.testcases.push(TestCase::new(&format!("{index}"), ""));
        }

        let mut terminal = Terminal::new(TestBackend::new(60, 12)).unwrap();
        terminal
            .draw(|frame| list.draw(frame, frame.area(), CaseFocus::None))
            .unwrap();

        let area = list.scrollbar_area;
        assert!(area.height > 2, "the case list must expose a scrollbar");
        assert_eq!(area.width, 1, "the case-list scrollbar is one cell wide");
        let buffer = terminal.backend().buffer();
        for row in area.y..area.y + area.height {
            assert!(
                matches!(buffer[(area.x, row)].symbol(), "┃" | "│"),
                "the case-list scrollbar uses thin glyphs"
            );
        }
        let bottom = area.y + area.height - 2;

        list.handle_mouse(&mouse(
            MouseEventKind::Down(MouseButton::Left),
            area.x,
            bottom,
        ));
        assert!(list.scroll_row > 0, "clicking the track must seek down");

        list.handle_mouse(&mouse(
            MouseEventKind::Drag(MouseButton::Left),
            area.x,
            area.y + 1,
        ));
        assert_eq!(list.scroll_row, 0, "dragging to the top must seek back");

        list.handle_mouse(&mouse(
            MouseEventKind::Up(MouseButton::Left),
            area.x,
            area.y + 1,
        ));
        assert!(list.scrollbar_drag.is_none());
    }

    #[test]
    fn list_only_reserves_a_column_when_the_scrollbar_is_needed() {
        let mut list = TestCaseList::new();
        list.testcases.push(TestCase::new("0", ""));

        let mut terminal = Terminal::new(TestBackend::new(60, 20)).unwrap();
        terminal
            .draw(|frame| list.draw(frame, frame.area(), CaseFocus::None))
            .unwrap();
        let width_without_scrollbar = list.testcases[0].area.rect.width;
        assert_eq!(list.scrollbar_area, Rect::default());
        assert_eq!(width_without_scrollbar, 60);

        for index in 1..20 {
            list.testcases.push(TestCase::new(&format!("{index}"), ""));
        }
        terminal
            .draw(|frame| list.draw(frame, frame.area(), CaseFocus::None))
            .unwrap();

        assert_eq!(list.scrollbar_area.width, 1, "the scrollbar is thin");
        assert_eq!(
            list.testcases[0].area.rect.width,
            width_without_scrollbar - 1,
            "the list gives up one column only when the scrollbar appears"
        );
    }
}
