//! Interactive sessions: a problem without samples runs live on a
//! pseudo-terminal, and its transcript is kept here.

use super::*;

pub(super) const SESSION_LINE_CAP: usize = 2_000;
pub(super) const SESSION_LINE_BYTE_CAP: usize = 8 * 1024;
pub(super) const SESSION_PARTIAL_BYTE_CAP: usize = 64 * 1024;
pub(super) const SESSION_TOTAL_BYTE_CAP: usize = 8 * 1024 * 1024;
pub(super) const SESSION_INPUT_BYTE_CAP: usize = 64 * 1024;
pub(super) const SESSION_SCROLL_STEP: usize = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TranscriptKind {
    You,
    Status,
    Out,
    Err,
}

pub(super) enum SessionEdit {
    Backspace,
    Delete,
    Left,
    Right,
    Home,
    End,
    Insert(char),
}

pub(super) struct TranscriptLine {
    pub(super) kind: TranscriptKind,
    pub(super) text: String,
}

/// A live program the user can type to. Shown in place of the case list when
/// a received problem arrived with no samples.
pub(super) struct InteractiveSession {
    pub(super) id: u64,
    pub(super) lines: VecDeque<TranscriptLine>,
    /// The currently unterminated output line. The PTY merges stdout and
    /// stderr, so one partial is enough.
    pub(super) partial: String,
    pub(super) partial_kind: TranscriptKind,
    /// The partial hit its byte cap and further bytes are being dropped until
    /// the next newline.
    pub(super) partial_dropped: bool,
    /// Bytes retained in `lines`, used to enforce the total memory cap.
    pub(super) bytes: usize,
    pub(super) input: String,
    /// Byte index into `input`, always on a char boundary.
    pub(super) cursor: usize,
    /// Lines scrolled back from the tail. 0 follows new output.
    pub(super) scroll: usize,
    pub(super) alive: bool,
    pub(super) stopping: bool,
    pub(super) cancel: Arc<AtomicBool>,
    pub(super) pgid: Arc<AtomicI32>,
    pub(super) input_tx: Option<Sender<SessionInput>>,
}

impl InteractiveSession {
    pub(super) fn new(
        id: u64,
        cancel: Arc<AtomicBool>,
        pgid: Arc<AtomicI32>,
        input_tx: Sender<SessionInput>,
    ) -> Self {
        Self {
            id,
            lines: VecDeque::new(),
            partial: String::new(),
            partial_kind: TranscriptKind::Out,
            partial_dropped: false,
            bytes: 0,
            input: String::new(),
            cursor: 0,
            scroll: 0,
            alive: true,
            stopping: false,
            cancel,
            pgid,
            input_tx: Some(input_tx),
        }
    }

    pub(super) fn push_line(&mut self, kind: TranscriptKind, text: String) {
        self.push_line_anchored(kind, text, false);
    }

    /// Push the line that was previously shown as the unterminated partial.
    /// It was already counted in the visible total, so completing it must not
    /// move a scrolled transcript.
    pub(super) fn push_partial_line(&mut self, kind: TranscriptKind, text: String) {
        self.push_line_anchored(kind, text, true);
    }

    pub(super) fn push_line_anchored(
        &mut self,
        kind: TranscriptKind,
        mut text: String,
        counted: bool,
    ) {
        if text.len() > SESSION_LINE_BYTE_CAP {
            let end = floor_char_boundary(&text, SESSION_LINE_BYTE_CAP);
            text.truncate(end);
            text.push('…');
        }
        self.bytes += text.len();
        self.lines.push_back(TranscriptLine { kind, text });
        // Trimming the front does not change which content is visible: the
        // scroll anchor is the tail, so only appending a new line moves it.
        while self.lines.len() > SESSION_LINE_CAP {
            if let Some(old) = self.lines.pop_front() {
                self.bytes = self.bytes.saturating_sub(old.text.len());
            }
        }
        while self.bytes > SESSION_TOTAL_BYTE_CAP && self.lines.len() > 1 {
            if let Some(old) = self.lines.pop_front() {
                self.bytes = self.bytes.saturating_sub(old.text.len());
            }
        }
        if self.scroll > 0 && !counted {
            self.scroll += 1;
        }
    }

    pub(super) fn push_status(&mut self, text: &str) {
        self.flush_partial();
        let mut rest = text;
        while let Some(index) = rest.find('\n') {
            self.push_line(TranscriptKind::Status, rest[..index].to_string());
            rest = &rest[index + 1..];
        }
        if !rest.is_empty() {
            self.push_line(TranscriptKind::Status, rest.to_string());
        }
    }

    pub(super) fn push_you(&mut self, text: String) {
        // The prompt (an unterminated output line) must stay above the echo;
        // otherwise the transcript shows the user replying before the prompt.
        self.flush_partial();
        self.push_line(TranscriptKind::You, text);
        self.scroll = 0;
    }

    pub(super) fn push_stream(&mut self, kind: TranscriptKind, text: &str) {
        match kind {
            TranscriptKind::Status => self.push_status(text),
            TranscriptKind::You => self.push_you(text.to_string()),
            TranscriptKind::Out | TranscriptKind::Err => self.push_output(kind, text),
        }
    }

    pub(super) fn push_output(&mut self, kind: TranscriptKind, text: &str) {
        if kind != self.partial_kind {
            self.flush_partial();
            self.partial_kind = kind;
        }
        let mut rest = text;
        loop {
            if self.partial_dropped {
                let Some(index) = rest.find('\n') else {
                    return;
                };
                let line = std::mem::take(&mut self.partial);
                self.push_partial_line(self.partial_kind, line);
                self.partial_dropped = false;
                rest = &rest[index + 1..];
                continue;
            }
            let Some(index) = rest.find('\n') else {
                self.partial.push_str(rest);
                self.clamp_partial();
                return;
            };
            let counted = !self.partial.is_empty();
            self.partial.push_str(&rest[..index]);
            let mut line = std::mem::take(&mut self.partial);
            if line.ends_with('\r') {
                line.pop();
            }
            self.push_line_anchored(self.partial_kind, line, counted);
            rest = &rest[index + 1..];
        }
    }

    pub(super) fn clamp_partial(&mut self) {
        if self.partial.len() <= SESSION_PARTIAL_BYTE_CAP {
            return;
        }
        let end = floor_char_boundary(&self.partial, SESSION_PARTIAL_BYTE_CAP);
        self.partial.truncate(end);
        self.partial.push('…');
        self.partial_dropped = true;
    }

    pub(super) fn flush_partial(&mut self) {
        if !self.partial.is_empty() {
            let text = std::mem::take(&mut self.partial);
            self.partial_dropped = false;
            self.push_partial_line(self.partial_kind, text);
        }
    }

    pub(super) fn scroll_back(&mut self, lines: usize) {
        self.scroll = self.scroll.saturating_add(lines);
    }

    pub(super) fn scroll_forward(&mut self, lines: usize) {
        self.scroll = self.scroll.saturating_sub(lines);
    }
}

impl Helper {
    /// Start or restart the live program. The case list stays empty.
    pub(super) fn cmd_interact(&mut self) -> Option<PassingCommand> {
        if self.running {
            self.status.warn(RunRefusal::AlreadyRunning.message());
            return None;
        }
        let Some(problem) = self.active_problem.clone() else {
            self.status.warn(RunRefusal::NoProblem.message());
            return None;
        };
        self.stop_session();
        let id = self.next_session_id;
        self.next_session_id = self.next_session_id.wrapping_add(1);
        let cancel = Arc::new(AtomicBool::new(false));
        let pgid = Arc::new(AtomicI32::new(0));
        let (input_tx, input_rx) = mpsc::channel();
        self.session = Some(InteractiveSession::new(
            id,
            cancel.clone(),
            pgid.clone(),
            input_tx,
        ));
        self.input_mode = InputMode::Session;
        self.status.clear();
        Some(ToRunner(RunnerCommand::Interact {
            problem,
            session_id: id,
            cancel,
            input: input_rx,
            pgid,
        }))
    }

    /// Ask the runner to stop the live program. The transcript stays so the
    /// last output is still readable. The process group is killed from here as
    /// well, so quitting cannot outrun the runner's poll loop.
    pub(super) fn stop_session(&mut self) {
        let Some(session) = self.session.as_mut() else {
            return;
        };
        if !session.alive {
            return;
        }
        session.cancel.store(true, Ordering::SeqCst);
        session.input_tx = None;
        session.stopping = true;
        let pgid = session.pgid.load(Ordering::SeqCst);
        if pgid > 0 {
            crate::core::runner::kill_session_group(pgid);
        }
    }

    pub fn session_chunk(&mut self, session_id: u64, stream: SessionStream, text: String) {
        let Some(session) = self.session.as_mut() else {
            return;
        };
        if session.id != session_id || !session.alive {
            return;
        }
        let kind = match stream {
            SessionStream::Status => TranscriptKind::Status,
            SessionStream::Out => TranscriptKind::Out,
            SessionStream::Err => TranscriptKind::Err,
        };
        session.push_stream(kind, &text);
    }

    pub fn session_ended(&mut self, session_id: u64, message: String) {
        let Some(session) = self.session.as_mut() else {
            return;
        };
        if session.id != session_id {
            return;
        }
        session.flush_partial();
        session.alive = false;
        session.stopping = false;
        session.input_tx = None;
        if message == "exited" || message == "stopped" {
            session.push_status(&message);
        } else {
            session.push_stream(TranscriptKind::Err, &message);
            session.flush_partial();
        }
        match message.as_str() {
            "exited" => self.status.success("program exited"),
            "stopped" => self.status.info("program stopped"),
            "compile failed" => self.status.error("compile error"),
            "failed to start" => self.status.error("program did not start"),
            other => self.status.error(format!("program {other}")),
        }
        if matches!(self.input_mode, InputMode::Session) {
            self.input_mode = InputMode::Normal;
        }
    }

    pub(super) fn handle_session_key(&mut self, key: ratatui::crossterm::event::KeyEvent) {
        if key.kind != KeyEventKind::Press {
            return;
        }
        let Some(session) = self.session.as_ref() else {
            self.input_mode = InputMode::Normal;
            return;
        };
        if !session.alive || session.stopping {
            if key.code == KeyCode::Esc {
                self.input_mode = InputMode::Normal;
            }
            return;
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Char('c') => self.send_session_signal(SessionInput::Interrupt, "^C"),
                KeyCode::Char('d') => self.send_session_signal(SessionInput::Eof, "^D"),
                _ => {}
            }
            return;
        }
        match key.code {
            KeyCode::Esc => self.input_mode = InputMode::Normal,
            KeyCode::Enter => self.send_session_line(),
            KeyCode::PageUp => {
                if let Some(session) = self.session.as_mut() {
                    session.scroll_back(SESSION_SCROLL_STEP);
                }
            }
            KeyCode::PageDown => {
                if let Some(session) = self.session.as_mut() {
                    session.scroll_forward(SESSION_SCROLL_STEP);
                }
            }
            KeyCode::Backspace => self.edit_session_input(SessionEdit::Backspace),
            KeyCode::Delete => self.edit_session_input(SessionEdit::Delete),
            KeyCode::Left => self.edit_session_input(SessionEdit::Left),
            KeyCode::Right => self.edit_session_input(SessionEdit::Right),
            KeyCode::Home => self.edit_session_input(SessionEdit::Home),
            KeyCode::End => self.edit_session_input(SessionEdit::End),
            KeyCode::Char(ch) => self.edit_session_input(SessionEdit::Insert(ch)),
            _ => {}
        }
    }

    pub(super) fn send_session_line(&mut self) {
        let Some(session) = self.session.as_mut() else {
            return;
        };
        if !session.alive || session.stopping {
            return;
        }
        let line = std::mem::take(&mut session.input);
        session.cursor = 0;
        let sent = session
            .input_tx
            .as_ref()
            .is_some_and(|tx| tx.send(SessionInput::Line(line.clone())).is_ok());
        if sent {
            session.push_you(line);
        } else {
            session.stopping = true;
            session.input_tx = None;
            session.push_status("session ended");
        }
    }

    pub(super) fn send_session_signal(&mut self, signal: SessionInput, label: &str) {
        let Some(session) = self.session.as_mut() else {
            return;
        };
        if !session.alive || session.stopping {
            return;
        }
        let sent = session
            .input_tx
            .as_ref()
            .is_some_and(|tx| tx.send(signal).is_ok());
        if sent {
            session.push_you(label.to_string());
        }
    }

    pub(super) fn edit_session_input(&mut self, edit: SessionEdit) {
        let Some(session) = self.session.as_mut() else {
            return;
        };
        if !session.alive || session.stopping {
            return;
        }
        match edit {
            SessionEdit::Backspace => {
                if session.cursor == 0 {
                    return;
                }
                let prev = session.input[..session.cursor]
                    .char_indices()
                    .next_back()
                    .map(|(index, _)| index)
                    .unwrap_or(0);
                session.input.replace_range(prev..session.cursor, "");
                session.cursor = prev;
            }
            SessionEdit::Delete => {
                if session.cursor >= session.input.len() {
                    return;
                }
                let next = session.input[session.cursor..]
                    .chars()
                    .next()
                    .map(|ch| ch.len_utf8())
                    .unwrap_or(0);
                let end = session.cursor + next;
                session.input.replace_range(session.cursor..end, "");
            }
            SessionEdit::Left => {
                if session.cursor == 0 {
                    return;
                }
                session.cursor = session.input[..session.cursor]
                    .char_indices()
                    .next_back()
                    .map(|(index, _)| index)
                    .unwrap_or(0);
            }
            SessionEdit::Right => {
                if session.cursor >= session.input.len() {
                    return;
                }
                let next = session.input[session.cursor..]
                    .chars()
                    .next()
                    .map(|ch| ch.len_utf8())
                    .unwrap_or(0);
                session.cursor += next;
            }
            SessionEdit::Home => session.cursor = 0,
            SessionEdit::End => session.cursor = session.input.len(),
            SessionEdit::Insert(ch) => {
                if session.input.len() + ch.len_utf8() > SESSION_INPUT_BYTE_CAP {
                    return;
                }
                session.input.insert(session.cursor, ch);
                session.cursor += ch.len_utf8();
            }
        }
    }
}

pub(super) fn fit_input(input: &str, cursor: usize, width: usize) -> (String, usize) {
    if width == 0 {
        return (String::new(), 0);
    }
    let cursor = floor_char_boundary(input, cursor.min(input.len()));
    if input.width() <= width {
        return (input.to_string(), input[..cursor].width());
    }
    let mut end = cursor;
    let mut used = 0;
    for (index, ch) in input[..cursor].char_indices().rev() {
        let cells = ch.to_string().width();
        if used + cells > width {
            break;
        }
        used += cells;
        end = index;
    }
    let mut shown = String::new();
    let mut cells = 0;
    for ch in input[end..].chars() {
        let ch_cells = ch.to_string().width();
        if cells + ch_cells > width {
            break;
        }
        cells += ch_cells;
        shown.push(ch);
    }
    (shown, input[end..cursor].width())
}

pub(super) fn floor_char_boundary(text: &str, mut index: usize) -> usize {
    if index > text.len() {
        index = text.len();
    }
    while index > 0 && !text.is_char_boundary(index) {
        index -= 1;
    }
    index
}
