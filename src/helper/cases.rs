//! Adding, editing and deleting test cases, and saving them.

use super::*;

impl Helper {
    pub fn cmd_add(&mut self) -> bool {
        if self.active_problem.is_none() {
            self.status.warn(RunRefusal::NoProblem.message());
            return false;
        }
        // Editing mutates cases the runner may still be writing to — retire the run.
        self.cancel_run();
        self.stop_session();
        self.input_mode = InputMode::Editing;
        self.adding_case = true;
        self.tc_list.add_tc();
        self.expand_all_cases();
        self.sync_problem_tests();
        true
    }

    pub fn cmd_edit(&mut self) -> bool {
        if self.active_problem.is_none() {
            self.status.warn(RunRefusal::NoProblem.message());
            return false;
        }
        if self.tc_list.testcases.is_empty() {
            self.status
                .warn("no test cases to edit — press a to add one");
            return false;
        }
        self.cancel_run();
        self.input_mode = InputMode::Editing;
        self.adding_case = false;
        self.tc_list.focus_first();
        self.expand_all_cases();
        true
    }

    /// `x` enters delete mode: the focused case is highlighted, `Tab` picks
    /// another one, `Enter` asks for confirmation, and only that deletes.
    pub fn cmd_delete(&mut self) {
        if self.tc_list.testcases.is_empty() {
            self.status.warn("no test cases to delete");
            return;
        }
        // A stale run must not write verdicts into a list that is about to shift.
        self.cancel_run();
        self.expand_all_cases();
        self.tc_list.active_area = self
            .tc_list
            .active_area
            .min(self.tc_list.testcases.len() - 1);
        self.tc_list.update_scroll();
        self.input_mode = InputMode::DeleteSelect;
    }

    /// Moves the delete selection by one case, wrapping around.
    pub(super) fn cycle_delete_selection(&mut self, backwards: bool) {
        let total = self.tc_list.testcases.len();
        if total == 0 {
            return;
        }
        self.tc_list.active_area = step_wrapped(self.tc_list.active_area, total, backwards);
        self.tc_list.update_scroll();
    }

    /// The confirmed half of the delete flow.
    pub(super) fn delete_selected_case(&mut self) {
        let total = self.tc_list.testcases.len();
        if total == 0 {
            self.input_mode = InputMode::Normal;
            return;
        }
        let index = self.tc_list.active_area.min(total - 1);
        self.tc_list.testcases.remove(index);
        self.tc_list.active_area = self
            .tc_list
            .active_area
            .min(self.tc_list.testcases.len().saturating_sub(1));
        self.tc_list.active_box = ActiveBox::Input;
        self.tc_list.update_scroll();
        self.input_mode = InputMode::Normal;
        if self.sync_problem_tests() {
            let left = self.tc_list.testcases.len();
            self.status
                .info(format!("deleted case {} · {left} left", index + 1));
        }
    }

    /// Nothing may stay hidden behind a collapsed card while editing — the
    /// user has to be able to see every case they are editing.
    pub(super) fn expand_all_cases(&mut self) {
        for tc in self.tc_list.testcases.iter_mut() {
            tc.collapsed = false;
        }
        self.tc_list.update_scroll();
    }

    pub(super) fn finish_editing(&mut self) {
        self.input_mode = InputMode::Normal;
        self.adding_case = false;

        // Drop a trailing case that was added but left completely empty.
        if self.tc_list.testcases.len() > 1
            && let Some(last) = self.tc_list.testcases.last()
            && last.inp_content.lines().join("").trim().is_empty()
            && last.exp_content.lines().join("").trim().is_empty()
        {
            self.tc_list.remove_last();
        }

        if self.sync_problem_tests() {
            let count = self.tc_list.testcases.len();
            self.status.success(format!(
                "saved {count} case{}",
                if count == 1 { "" } else { "s" }
            ));
        }
    }

    pub(super) fn current_test_cases(&self) -> Vec<crate::core::ReceivingTestCase> {
        self.tc_list
            .testcases
            .iter()
            .map(|tc| crate::core::ReceivingTestCase {
                input: tc.inp_content.lines().join("\n"),
                expected_output: tc.exp_content.lines().join("\n"),
            })
            .collect()
    }

    pub(super) fn sync_problem_tests(&mut self) -> bool {
        let test_cases = self.current_test_cases();
        let Some(problem) = &mut self.active_problem else {
            return false;
        };
        problem.test_cases = test_cases;
        let snapshot = problem.clone();
        if let Err(e) = self.fs_ops.save_tc_file(&snapshot) {
            self.status.error(format!("cannot save the cases — {e}"));
            return false;
        }
        true
    }
}
