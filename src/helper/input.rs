//! Keyboard and mouse: every event comes through `handle_event`.

use super::*;

impl Helper {
    pub fn handle_event(&mut self, event: Event) -> Option<PassingCommand> {
        self.dirty = true;
        // Some terminals report key releases too; only a press is an action.
        if let Event::Key(key) = &event
            && key.kind != KeyEventKind::Press
        {
            return None;
        }
        if let Event::Mouse(mouse) = &event
            && self.track_text_selection(mouse)
        {
            return None;
        }
        if let Event::Mouse(mouse) = &event
            && self.pane_dialog.is_none()
            && self.open_on_double_click(mouse)
        {
            return None;
        }
        // Help sits on top of everything (the message log included) and
        // closes back to whatever was open under it.
        if let Event::Key(key) = &event {
            if self.show_help {
                if is_help_key(key) || matches!(key.code, KeyCode::Esc | KeyCode::Char('q')) {
                    self.show_help = false;
                }
                return None;
            }
            if is_help_key(key) {
                self.show_help = true;
                return None;
            }
        }
        // The viewer opens over any sheet, the message log included.
        if self.pane_dialog.is_some() {
            self.handle_pane_dialog_event(event);
            return None;
        }
        if self.show_log {
            if let Event::Key(key) = &event
                && matches!(key.code, KeyCode::Char('m' | 'q') | KeyCode::Esc)
            {
                self.show_log = false;
            }
            return None;
        }
        match event {
            Event::Key(key) => match self.input_mode {
                InputMode::Editing => {
                    if key.code == KeyCode::Esc {
                        self.finish_editing();
                        return None;
                    }
                    self.tc_list.handle_key(Event::Key(key), self.adding_case);
                    self.tc_list.update_scroll();
                }
                InputMode::Normal => match key.code {
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        self.cancel_run();
                        self.stop_session();
                    }
                    KeyCode::Esc if self.running => self.cancel_run(),
                    KeyCode::Esc
                        if self
                            .session
                            .as_ref()
                            .is_some_and(|session| session.alive && !session.stopping) =>
                    {
                        self.stop_session();
                    }
                    KeyCode::Esc => {
                        // Esc steps back one layer: a text selection, then an
                        // alert.
                        if self.text_selection.take().is_none() {
                            self.status.dismiss();
                        }
                    }
                    KeyCode::Char('q') => {
                        if self.submissions.iter().any(|s| !s.done()) {
                            self.input_mode = InputMode::ConfirmQuit;
                            return None;
                        }
                        self.cancel_run();
                        self.stop_session();
                        return Some(ToHelper(HelperCommand::Quit));
                    }
                    KeyCode::Char('a') => {
                        self.cmd_add();
                    }
                    KeyCode::Char('x') => self.cmd_delete(),
                    KeyCode::Char('r') => return self.cmd_run(),
                    KeyCode::Enter
                        if self.tc_list.testcases.is_empty()
                            && self
                                .session
                                .as_ref()
                                .is_some_and(|session| session.alive && !session.stopping) =>
                    {
                        self.input_mode = InputMode::Session;
                    }
                    KeyCode::Char('e') => {
                        self.cmd_edit();
                    }
                    KeyCode::Char('s') => return self.cmd_submit(),
                    KeyCode::Char('d') => {
                        self.dir_terminal.clear_output();
                        self.input_mode = InputMode::Directory;
                    }
                    KeyCode::Char('c') => {
                        let (config, _) = self.fs_ops.load_config();
                        self.config_menu = ConfigMenu::new(&config);
                        self.input_mode = InputMode::Config;
                    }
                    KeyCode::Char('m') => self.show_log = true,
                    _ => {}
                },
                InputMode::ConfirmQuit => match key.code {
                    KeyCode::Enter | KeyCode::Char('q') => {
                        self.cancel_run();
                        self.stop_session();
                        return Some(ToHelper(HelperCommand::Quit));
                    }
                    KeyCode::Esc | KeyCode::Backspace | KeyCode::Char('n') => {
                        self.input_mode = InputMode::Normal;
                    }
                    _ => {}
                },
                InputMode::DeleteSelect => match key.code {
                    KeyCode::BackTab => self.cycle_delete_selection(true),
                    KeyCode::Tab => {
                        let backwards = key.modifiers.contains(KeyModifiers::SHIFT);
                        self.cycle_delete_selection(backwards);
                    }
                    KeyCode::Enter => {
                        self.input_mode = InputMode::ConfirmDelete;
                    }
                    KeyCode::Esc => {
                        self.input_mode = InputMode::Normal;
                    }
                    _ => {}
                },
                InputMode::ConfirmDelete => match key.code {
                    KeyCode::Enter => self.delete_selected_case(),
                    KeyCode::Backspace | KeyCode::Esc => {
                        self.input_mode = InputMode::DeleteSelect;
                    }
                    _ => {}
                },
                InputMode::Directory => {
                    if self.dir_terminal.handle_event(key) {
                        self.input_mode = InputMode::Normal;
                    }
                }
                InputMode::Config => {
                    if self.config_menu.handle_key(Event::Key(key)) {
                        let config = self.config_menu.to_config();
                        let saved = self.fs_ops.save_config(&config);
                        self.input_mode = InputMode::Normal;
                        if saved {
                            theme::set(config.theme);
                            self.status.success("config saved");
                        } else {
                            self.status
                                .error("cannot save the config — is ~/.zch writable?");
                        }
                    }
                }
                InputMode::ConfirmSubmit => match key.code {
                    KeyCode::Enter => {
                        if let Some(pending) = self.pending_submit.take() {
                            self.input_mode = InputMode::Normal;
                            let id = self.next_submission_id;
                            self.next_submission_id += 1;
                            let label = self
                                .active_problem
                                .as_ref()
                                .map(|p| p.name.clone())
                                .unwrap_or_else(|| pending.info.file_name.clone());
                            self.submissions.push(Submission {
                                id,
                                label: label.clone(),
                                tone: Tone::Progress,
                                message: format!(
                                    "opening {} {}",
                                    pending.info.platform, pending.info.problem_code
                                ),
                                started: Instant::now(),
                                finished: None,
                            });
                            self.submission_scroll = 0;
                            if self.submissions.len() > MAX_SUBMISSIONS {
                                self.submissions.remove(0);
                            }
                            return Some(ToSubmitter(SubmitterCommand::SubmitCode {
                                id,
                                config: pending.config,
                            }));
                        }
                    }
                    KeyCode::Backspace | KeyCode::Esc => {
                        self.pending_submit = None;
                        self.input_mode = InputMode::Normal;
                    }
                    _ => {}
                },
                InputMode::Session => self.handle_session_key(key),
            },
            Event::Mouse(mouse) if matches!(self.input_mode, InputMode::Config) => {
                self.config_menu.handle_mouse(&mouse);
            }
            Event::Mouse(mouse) => {
                let position = Position::new(mouse.column, mouse.row);
                if self.submissions_scrollbar_area.contains(position) {
                    match mouse.kind {
                        MouseEventKind::Down(MouseButton::Left) => {
                            self.begin_submissions_scrollbar_drag(position);
                            return None;
                        }
                        MouseEventKind::Drag(MouseButton::Left) => {
                            if let Some(drag) = self.submissions_scrollbar_drag {
                                self.submission_scroll = drag.scroll_at(position);
                            }
                            return None;
                        }
                        MouseEventKind::Up(MouseButton::Left) => {
                            self.submissions_scrollbar_drag = None;
                            return None;
                        }
                        _ => {}
                    }
                }

                if matches!(mouse.kind, MouseEventKind::Up(MouseButton::Left)) {
                    self.submissions_scrollbar_drag = None;
                }
                if matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) {
                    self.submissions_scrollbar_drag = None;
                }

                let over_submissions = self
                    .submissions_area
                    .contains(Position::new(mouse.column, mouse.row));
                let transcript = self.tc_list.testcases.is_empty() && self.session.is_some();
                match mouse.kind {
                    MouseEventKind::ScrollDown if over_submissions => self.scroll_submissions(true),
                    MouseEventKind::ScrollUp if over_submissions => self.scroll_submissions(false),
                    MouseEventKind::ScrollDown if transcript => {
                        if let Some(session) = self.session.as_mut() {
                            session.scroll_forward(SESSION_SCROLL_STEP);
                        }
                    }
                    MouseEventKind::ScrollUp if transcript => {
                        if let Some(session) = self.session.as_mut() {
                            session.scroll_back(SESSION_SCROLL_STEP);
                        }
                    }
                    MouseEventKind::ScrollDown => {
                        let editing = matches!(self.input_mode, InputMode::Editing);
                        if !editing || !self.tc_list.scroll_active_pane(&mouse, true) {
                            self.tc_list.handle_mouse(&mouse);
                        }
                    }
                    MouseEventKind::ScrollUp => {
                        let editing = matches!(self.input_mode, InputMode::Editing);
                        if !editing || !self.tc_list.scroll_active_pane(&mouse, false) {
                            self.tc_list.handle_mouse(&mouse);
                        }
                    }
                    _ => self.tc_list.handle_mouse(&mouse),
                }
            }
            _ => {}
        }
        None
    }
}

/// Ctrl-H or Option-H: keys that are never text, so help opens from every
/// mode, typing ones included. Without "Option as Meta", macOS
/// terminals send Option-H as `˙`, which counts too.
fn is_help_key(key: &ratatui::crossterm::event::KeyEvent) -> bool {
    match key.code {
        KeyCode::Char('˙') => true,
        KeyCode::Char('h' | 'H') => key
            .modifiers
            .intersects(KeyModifiers::ALT | KeyModifiers::CONTROL),
        _ => false,
    }
}
