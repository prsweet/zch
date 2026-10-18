use ratatui::{
    Frame,
    crossterm::event::{Event, KeyCode, KeyModifiers, MouseButton, MouseEvent, MouseEventKind},
    layout::{Constraint::{Length, Percentage}, Layout, Rect},
    widgets::{Scrollbar, ScrollbarState},
};

use crate::components::{
    testcase::{ActiveBox, TestCase},
    theme,
};

pub struct TestCaseList {
    pub testcases: Vec<TestCase>,
    pub available_height: usize,
    pub scroll_row: usize,
    pub active_area: usize,
    pub active_box: ActiveBox,
}

impl TestCaseList {
    pub fn new() -> Self {
        Self {
            testcases: Vec::new(),
            available_height: 0,
            scroll_row: 0,
            active_area: 0,
            active_box: ActiveBox::Input,
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
    }

    pub fn add_tc(&mut self) {
        self.testcases.push(TestCase::new("", ""));
        self.active_area = self.testcases.len().saturating_sub(1);
        self.active_box = ActiveBox::Input;
        self.update_scroll();
    }

    pub fn remove_last(&mut self) {
        if self.testcases.is_empty() {
            return;
        }
        self.testcases.pop();
        self.active_area = self.active_area.min(self.testcases.len().saturating_sub(1));
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

    pub fn draw(&mut self, frame: &mut Frame, area: Rect, is_editing: bool) {
        self.available_height = area.height as usize;
        let total = self.total_rows();
        let needs_scrollbar = total > self.available_height;

        let (tc_list_area, scrollbar_area) = if needs_scrollbar {
            let division = Layout::horizontal([Percentage(100), Length(2)]).split(area);
            (division[0], division[1])
        } else {
            (area, Rect::default())
        };

        let scroll_bar = Scrollbar::new(ratatui::widgets::ScrollbarOrientation::VerticalRight)
            .style(theme::MUTED)
            .track_style(theme::BORDER)
            .begin_symbol(Some("-"))
            .end_symbol(Some("-"));

        self.scroll_row = self.scroll_row.min(total.saturating_sub(self.available_height));
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

            let tc_y = tc_list_area.y as usize + (tc_top.saturating_sub(start_row));
            let tc_area = Rect {
                x: tc_list_area.x,
                y: tc_y as u16,
                width: tc_list_area.width,
                height: visible_height as u16,
            };

            let active_box = if idx == self.active_area && is_editing { self.active_box } else { ActiveBox::None };
            tc.draw_tc(frame, tc_area, idx + 1, active_box, is_editing && idx == self.active_area);
            current_row = tc_bottom;
        }

        let scroll_range = total.saturating_sub(self.available_height);
        let mut scroll_state = ScrollbarState::new(scroll_range).position(self.scroll_row);
        frame.render_stateful_widget(scroll_bar, scrollbar_area, &mut scroll_state);
    }

    pub fn handle_key(&mut self, event: Event) {
        if let Event::Key(key) = event {
            if self.testcases.is_empty() {
                return;
            }
            if key.code == KeyCode::Tab {
                let backwards = key.modifiers.contains(KeyModifiers::SHIFT);
                match (self.active_box, backwards) {
                    (ActiveBox::Input, false) => self.active_box = ActiveBox::Expected,
                    (ActiveBox::Input, true) => {
                        self.active_box = ActiveBox::Expected;
                        self.active_area = (self.active_area + self.testcases.len() - 1) % self.testcases.len();
                    }
                    (ActiveBox::Expected, false) => {
                        self.active_box = ActiveBox::Input;
                        self.active_area = (self.active_area + 1) % self.testcases.len();
                    }
                    (ActiveBox::Expected, true) => self.active_box = ActiveBox::Input,
                    (ActiveBox::None, _) => self.active_box = ActiveBox::Input,
                }
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

    pub fn handle_mouse(&mut self, mouse: &MouseEvent) {
        match mouse.kind {
            MouseEventKind::ScrollDown => {
                let max_scroll = self.total_rows().saturating_sub(self.available_height);
                self.scroll_row = self.scroll_row.saturating_add(1).min(max_scroll);
            }
            MouseEventKind::ScrollUp => {
                self.scroll_row = self.scroll_row.saturating_sub(1);
            }
            MouseEventKind::Down(MouseButton::Left) => {
                for (idx, tc) in self.testcases.iter_mut().enumerate() {
                    if tc.handle_mouse(mouse) {
                        if !tc.collapse_toggle_area.check_hover(mouse) {
                            self.active_area = idx;
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
