use std::{
    fs::{self, File},
    io::{self, BufRead, BufReader, BufWriter, Read, Seek, SeekFrom, Write},
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::Instant,
};

use ratatui::{
    buffer::Buffer,
    crossterm::event::{MouseButton, MouseEvent, MouseEventKind},
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Paragraph, Widget},
};
use ratatui_textarea::TextArea;

use crate::components::{VisualState, icons, motion, status, theme, ui};

/// Where a case stands in the current run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunState {
    Idle,
    Queued,
    Running,
}

/// Everything a case needs to know to draw itself this frame.
pub struct CaseView {
    pub number: usize,
    pub focus: CaseFocus,
    pub active_box: ActiveBox,
    /// The keyboard selection in Normal mode.
    pub selected: bool,
    pub state: RunState,
    pub epoch: Instant,
    pub now: Instant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActiveBox {
    Input,
    Expected,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaneKind {
    Input,
    Expected,
    Got,
}

/// How a case is focused right now: being edited, being picked for deletion,
/// or not focused at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaseFocus {
    None,
    Edit,
    Delete,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaseVerdict {
    Pending,
    AC {
        ms: u128,
    },
    WA {
        ms: u128,
    },
    Tle,
    RE,
    /// Output limit exceeded.
    Ole,
    CE,
}

impl CaseVerdict {
    pub fn rgb(&self) -> theme::Rgb {
        match self {
            CaseVerdict::Pending => theme::p().muted,
            CaseVerdict::AC { .. } => theme::p().success,
            CaseVerdict::WA { .. } | CaseVerdict::RE | CaseVerdict::Ole => theme::p().error,
            CaseVerdict::Tle => theme::p().warning,
            CaseVerdict::CE => theme::p().modified,
        }
    }

    /// The one word for this verdict, used on the card, in the status line
    /// and in the run summary alike. `compact` is for narrow cards.
    pub fn label(&self, compact: bool) -> &'static str {
        match (self, compact) {
            (CaseVerdict::Pending, _) => "",
            (CaseVerdict::AC { .. }, false) => "accepted",
            (CaseVerdict::AC { .. }, true) => "ac",
            (CaseVerdict::WA { .. }, false) => "wrong answer",
            (CaseVerdict::WA { .. }, true) => "wa",
            (CaseVerdict::Tle, false) => "time limit exceeded",
            (CaseVerdict::Tle, true) => "tle",
            (CaseVerdict::RE, false) => "runtime error",
            (CaseVerdict::RE, true) => "re",
            (CaseVerdict::Ole, false) => "output limit exceeded",
            (CaseVerdict::Ole, true) => "ole",
            (CaseVerdict::CE, false) => "compilation error",
            (CaseVerdict::CE, true) => "ce",
        }
    }

    pub fn time_ms(&self) -> Option<u128> {
        match self {
            CaseVerdict::AC { ms } | CaseVerdict::WA { ms } => Some(*ms),
            _ => None,
        }
    }

    pub fn is_ac(&self) -> bool {
        matches!(self, CaseVerdict::AC { .. })
    }

    pub fn is_failed(&self) -> bool {
        matches!(
            self,
            CaseVerdict::WA { .. }
                | CaseVerdict::Tle
                | CaseVerdict::RE
                | CaseVerdict::Ole
                | CaseVerdict::CE
        )
    }
}

static NEXT_GOT_FILE: AtomicU64 = AtomicU64::new(0);

/// Lines the paged Got dialog reads at a time.
pub(crate) const GOT_PAGE_LINES: usize = 512;
/// One checkpoint every N lines, so a page can seek near its start without
/// keeping an offset for every line.
const GOT_INDEX_STRIDE: usize = 1024;
/// A single line wider than this is shown as a prefix; the rest stays on disk.
const GOT_PAGE_LINE_MAX_BYTES: usize = 8_000;

struct PageLine {
    text: String,
    truncated: bool,
}

/// Raw Got output spilled to a temp file when it is too large for the
/// in-memory dialog budget. Only the visible page is ever read.
struct GotFile {
    path: PathBuf,
    line_starts: Vec<u64>,
    line_count: usize,
    stderr_line: Option<usize>,
}

impl GotFile {
    fn spill(got: &str) -> io::Result<Self> {
        let path = std::env::temp_dir().join(format!(
            "zch-got-{}-{}.txt",
            std::process::id(),
            NEXT_GOT_FILE.fetch_add(1, Ordering::Relaxed)
        ));
        let file = File::create(&path)?;
        let mut writer = BufWriter::new(file);
        if let Err(error) = writer
            .write_all(got.as_bytes())
            .and_then(|_| writer.flush())
        {
            let _ = fs::remove_file(&path);
            return Err(error);
        }

        let mut line_starts = vec![0];
        let mut line = 0;
        for (index, byte) in got.bytes().enumerate() {
            if byte == b'\n' {
                line += 1;
                if line % GOT_INDEX_STRIDE == 0 {
                    line_starts.push((index + 1) as u64);
                }
            }
        }
        let line_count = if got.is_empty() {
            0
        } else {
            line + usize::from(!got.ends_with('\n'))
        };
        let stderr_line = got.find(STDERR_MARKER).map(|position| {
            got[..position]
                .bytes()
                .filter(|&byte| byte == b'\n')
                .count()
                + 1
        });

        Ok(Self {
            path,
            line_starts,
            line_count,
            stderr_line,
        })
    }

    fn read_page(&self, start: usize, count: usize) -> io::Result<Vec<PageLine>> {
        if start >= self.line_count || count == 0 {
            return Ok(Vec::new());
        }
        let checkpoint = start / GOT_INDEX_STRIDE;
        let offset = self.line_starts.get(checkpoint).copied().unwrap_or(0);
        let mut reader = BufReader::new(File::open(&self.path)?);
        reader.seek(SeekFrom::Start(offset))?;

        let mut line = checkpoint * GOT_INDEX_STRIDE;
        while line < start {
            if !skip_line(&mut reader)? {
                return Ok(Vec::new());
            }
            line += 1;
        }

        let end = self.line_count.min(start.saturating_add(count));
        let mut page = Vec::with_capacity(end - start);
        for _ in start..end {
            let Some((text, truncated)) = read_line_limited(&mut reader, GOT_PAGE_LINE_MAX_BYTES)?
            else {
                break;
            };
            page.push(PageLine { text, truncated });
        }
        Ok(page)
    }
}

impl Drop for GotFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn skip_line<R: BufRead>(reader: &mut R) -> io::Result<bool> {
    loop {
        let buffer = reader.fill_buf()?;
        if buffer.is_empty() {
            return Ok(false);
        }
        if let Some(position) = buffer.iter().position(|&byte| byte == b'\n') {
            reader.consume(position + 1);
            return Ok(true);
        }
        let len = buffer.len();
        reader.consume(len);
    }
}

/// Read one line without letting a single enormous line pull the whole file
/// into memory. The caller gets a prefix plus a truncation flag.
fn read_line_limited<R: BufRead>(reader: &mut R, max: usize) -> io::Result<Option<(String, bool)>> {
    let mut buffer = Vec::new();
    let mut limited = (&mut *reader).take(max as u64);
    let read = limited.read_until(b'\n', &mut buffer)?;
    if read == 0 {
        return Ok(None);
    }

    let mut truncated = false;
    if buffer.last() != Some(&b'\n') {
        let (first, second, empty) = {
            let next = reader.fill_buf()?;
            (next.first().copied(), next.get(1).copied(), next.is_empty())
        };
        if first == Some(b'\n') {
            reader.consume(1);
        } else if first == Some(b'\r') && second == Some(b'\n') {
            reader.consume(2);
        } else if !empty {
            truncated = true;
            skip_line(reader)?;
        }
    }
    if buffer.last() == Some(&b'\n') {
        buffer.pop();
    }
    if buffer.last() == Some(&b'\r') {
        buffer.pop();
    }
    Ok(Some((
        String::from_utf8_lossy(&buffer).to_string(),
        truncated,
    )))
}

pub struct TestCase {
    pub inp_content: TextArea<'static>,
    pub exp_content: TextArea<'static>,
    /// Inline preview only. The redraw loop never sees more than this.
    pub got_content: Option<String>,
    /// Full raw output, kept only when it fits the dialog safety budget.
    /// Absent when the output was small enough to live in `got_content`, and
    /// absent when it was too large to keep at all. Draw never reads it.
    dialog_got: Option<String>,
    /// The inline preview is shorter than the output that arrived.
    got_truncated: bool,
    /// The raw output exceeded the dialog safety budget and could not be
    /// spilled to disk, so only the preview remains.
    output_too_large: bool,
    /// Raw output on disk for the paged dialog. Dropping this removes the file.
    got_file: Option<GotFile>,
    /// Bumped whenever the result changes, so an open paged dialog reloads.
    got_version: u64,
    pub input_box: VisualState,
    pub expected_box: VisualState,
    pub got_box: VisualState,
    pub collapse_toggle_area: VisualState,
    pub area: VisualState,
    pub verdict: CaseVerdict,
    /// Peak memory of the last run, when the platform reports it.
    pub memory_kb: Option<u64>,
    /// When the current verdict arrived, for its colour-in.
    revealed_at: Option<Instant>,
    pub collapsed: bool,
    input_cap: Option<usize>,
    output_cap: Option<usize>,
}

impl TestCase {
    pub fn new(input_str: &str, expected_str: &str) -> Self {
        let mut inp_content = TextArea::default();
        inp_content.insert_str(input_str);

        let mut exp_content = TextArea::default();
        exp_content.insert_str(expected_str);

        Self {
            inp_content,
            exp_content,
            got_content: None,
            dialog_got: None,
            got_truncated: false,
            output_too_large: false,
            got_file: None,
            got_version: 0,
            got_box: VisualState::new(),
            expected_box: VisualState::new(),
            input_box: VisualState::new(),
            collapse_toggle_area: VisualState::new(),
            area: VisualState::new(),
            verdict: CaseVerdict::Pending,
            memory_kb: None,
            revealed_at: None,
            collapsed: false,
            input_cap: None,
            output_cap: None,
        }
    }

    fn input_text(&self) -> String {
        self.inp_content.lines().join("\n")
    }

    fn expected_text(&self) -> String {
        self.exp_content.lines().join("\n")
    }

    pub fn pane_text(&self, pane: PaneKind) -> String {
        match pane {
            PaneKind::Input => self.input_text(),
            PaneKind::Expected => self.expected_text(),
            PaneKind::Got => self.got_content.clone().unwrap_or_default(),
        }
    }

    /// Pane heights follow `pane_lines`/`got_view` — the very lines that get
    /// drawn — so a case can never grow past what its panes can show.
    fn input_lines_uncapped(&self) -> usize {
        pane_lines(&self.input_text()).len().max(1)
    }

    fn expected_lines_uncapped(&self) -> usize {
        pane_lines(&self.expected_text()).len().max(1)
    }

    fn got_lines_uncapped(&self) -> usize {
        self.got_view(RunState::Idle).len().max(1)
    }

    fn input_lines(&self) -> usize {
        self.input_lines_uncapped()
            .min(self.input_cap.unwrap_or(usize::MAX))
    }

    fn expected_lines(&self) -> usize {
        self.expected_lines_uncapped()
            .min(self.output_cap.unwrap_or(usize::MAX))
    }

    fn got_lines(&self) -> usize {
        self.got_lines_uncapped()
            .min(self.output_cap.unwrap_or(usize::MAX))
    }

    fn output_rows_uncapped(&self) -> usize {
        self.expected_lines_uncapped()
            .max(self.got_lines_uncapped())
    }

    /// Header, the input label and lines, a gap, the expected/got labels and
    /// lines, and a gap after the card.
    fn full_height(&self) -> usize {
        CARD_CHROME_ROWS + self.input_lines() + self.output_rows()
    }

    /// While editing, the active case may not grow taller than the list
    /// viewport. Pane heights are split from the content itself, never from
    /// which pane has focus, so pressing Tab does not make Input shrink.
    /// Both panes scroll internally when they need more rows.
    pub fn fit_to_height(&mut self, viewport: usize) {
        let budget = viewport.saturating_sub(CARD_CHROME_ROWS).max(1);
        let input_want = self.input_lines_uncapped();
        let output_want = self.output_rows_uncapped();
        let wanted = input_want.saturating_add(output_want);

        let (input, output) = if wanted <= budget {
            (input_want, output_want)
        } else {
            let input = input_want
                .saturating_mul(budget)
                .checked_div(wanted)
                .unwrap_or(0)
                .clamp(1, budget.saturating_sub(1).max(1));
            (input, budget.saturating_sub(input).max(1))
        };

        self.input_cap = Some(input);
        self.output_cap = Some(output);
    }

    pub fn clear_height_caps(&mut self) {
        self.input_cap = None;
        self.output_cap = None;
    }

    /// The case grows to fit its content: title row + input pane + (expected|got) pane.
    /// Collapsed cases keep one blank row below the title so stacked collapsed
    /// cases do not touch.
    pub fn row_height(&self) -> usize {
        if self.collapsed {
            COLLAPSED_HEIGHT
        } else {
            self.full_height()
        }
    }

    pub fn clear_hit_areas(&mut self) {
        self.area.rect = Rect::default();
        self.collapse_toggle_area.rect = Rect::default();
        self.input_box.rect = Rect::default();
        self.expected_box.rect = Rect::default();
        self.got_box.rect = Rect::default();
    }

    pub fn reset_result(&mut self) {
        self.got_content = None;
        self.dialog_got = None;
        self.got_truncated = false;
        self.output_too_large = false;
        self.got_file = None;
        self.got_version = self.got_version.wrapping_add(1);
        self.verdict = CaseVerdict::Pending;
        self.memory_kb = None;
        self.revealed_at = None;
    }

    pub fn set_result(&mut self, verdict: CaseVerdict, got: String) {
        // Keep up to 1e7 bytes in memory for the full dialog diff. Anything
        // larger (or too many lines) is spilled so the live case never owns it.
        let raw_len = got.len();
        let line_count = got.lines().count();
        let expected = self.exp_content.lines();
        let expected_bytes: usize = expected.iter().map(|line| line.len() + 1).sum();
        let paged = raw_len > DIALOG_SAFE_MAX_BYTES
            || line_count > DIALOG_SAFE_MAX_LINES
            || expected_bytes > DIALOG_SAFE_MAX_BYTES
            || expected.len() > DIALOG_SAFE_MAX_LINES;
        let preview_cut = raw_len > INLINE_PREVIEW_MAX_BYTES;
        let (dialog_got, got_file, output_too_large) = if paged {
            match GotFile::spill(&got) {
                Ok(file) => (None, Some(file), false),
                Err(_) => (None, None, true),
            }
        } else if preview_cut {
            (Some(got.clone()), None, false)
        } else {
            (None, None, false)
        };
        let (preview, got_truncated) = retain_output_preview(got);
        self.got_content = Some(preview);
        self.dialog_got = dialog_got;
        self.got_file = got_file;
        self.got_truncated = got_truncated;
        self.output_too_large = output_too_large;
        self.got_version = self.got_version.wrapping_add(1);
        self.verdict = verdict;
        self.revealed_at = Some(Instant::now());
        self.collapsed = verdict.is_ac();
    }

    /// Whether the verdict is still colouring in.
    pub fn revealing(&self, now: Instant) -> bool {
        self.revealed_at
            .is_some_and(|at| motion::active(at, motion::REVEAL, now))
    }

    pub fn got_is_paged(&self) -> bool {
        self.got_file.is_some()
    }

    pub fn got_line_count(&self) -> usize {
        self.got_file.as_ref().map_or(0, |file| file.line_count)
    }

    pub fn got_version(&self) -> u64 {
        self.got_version
    }

    pub fn got_page_lines(&self, start: usize, count: usize) -> Vec<Line<'static>> {
        let Some(file) = &self.got_file else {
            return Vec::new();
        };
        let Ok(page) = file.read_page(start, count) else {
            return Vec::new();
        };
        let expected = self.exp_content.lines();
        let diff = matches!(self.verdict, CaseVerdict::WA { .. });

        page.into_iter()
            .enumerate()
            .map(|(offset, line)| {
                let index = start + offset;
                let after_stderr = file.stderr_line.is_some_and(|stderr| index > stderr);
                let mut rendered = if file.stderr_line == Some(index) {
                    stderr_rule()
                } else if diff && !after_stderr {
                    let expected = expected.get(index).map(String::as_str).unwrap_or("");
                    highlight_got_inner(expected, &line.text, false)
                        .into_iter()
                        .next()
                        .unwrap_or_default()
                } else if after_stderr {
                    Line::styled(line.text, self.stderr_style())
                } else {
                    Line::styled(line.text, self.stdout_style(file.stderr_line.is_some()))
                };
                if line.truncated {
                    rendered
                        .spans
                        .push(Span::styled("…", Style::new().fg(theme::faint())));
                }
                rendered
            })
            .collect()
    }

    /// Whether the inline preview is shorter than the output that arrived.
    pub fn got_truncated(&self) -> bool {
        self.got_truncated
    }

    /// Got lines for the expand dialog.
    ///
    /// Output that fits the safety budget is shown in full, diff included.
    /// Output past that budget reuses the inline preview and, when there is
    /// stdout to show, a warning. Skipping the inline diff is not a reason
    /// to warn: empty and stderr-only results have nothing to compare.
    pub fn got_dialog_view(&self) -> (Vec<Line<'static>>, Option<&'static str>) {
        if self.got_is_paged() {
            return (
                self.got_page_lines(0, GOT_PAGE_LINES),
                Some(PAGED_OUTPUT_NOTE),
            );
        }
        if self.output_too_large {
            let warning = self
                .got_content
                .as_deref()
                .is_some_and(has_stdout)
                .then_some(LARGE_OUTPUT_WARNING);
            return (self.got_view(RunState::Idle), warning);
        }
        let Some(got) = self.dialog_source() else {
            return (self.got_view(RunState::Idle), None);
        };
        if got.is_empty() {
            return (self.got_view(RunState::Idle), None);
        }
        (self.full_got_lines(got), None)
    }

    /// Raw output for the dialog: the retained full string, or the preview
    /// when that preview already is the whole output.
    fn dialog_source(&self) -> Option<&str> {
        self.dialog_got.as_deref().or(self.got_content.as_deref())
    }

    /// Uncapped Got lines. Called once, when the dialog opens.
    fn full_got_lines(&self, got: &str) -> Vec<Line<'static>> {
        let (stdout, stderr) = split_stderr(got);
        let mut lines = if matches!(self.verdict, CaseVerdict::WA { .. }) && !stdout.is_empty() {
            highlight_got_full(&self.expected_text(), stdout)
        } else {
            let style = self.stdout_style(stderr.is_some());
            let mut lines: Vec<Line<'static>> = stdout
                .lines()
                .map(|line| Line::styled(line.to_string(), style))
                .collect();
            if lines.is_empty() {
                lines.push(Line::default());
            }
            lines
        };
        if let Some(stderr) = stderr {
            lines.push(stderr_rule());
            let style = self.stderr_style();
            lines.extend(
                stderr
                    .lines()
                    .map(|line| Line::styled(line.to_string(), style)),
            );
        }
        lines
    }

    /// One case. Pane labels sit in the gutter under the case number, so the
    /// content starts on the row right below the header:
    ///
    /// ```text
    /// ▎ 02 ▾ wrong answer                              9 ms · 3.0 MB
    ///    in  5
    ///        6
    ///    exp  10                       ░ 0 ░░░░░░░░░░   (got: a tinted panel)
    /// ```
    ///
    /// The rail marks focus only (selected, editing, delete target); the
    /// verdict is a word in its colour, which colours in as it arrives.
    pub fn draw_tc(&mut self, buf: &mut Buffer, area: Rect, view: &CaseView) {
        self.area.rect = area;
        self.collapse_toggle_area.rect = Rect { height: 1, ..area };
        self.input_box.rect = Rect::default();
        self.expected_box.rect = Rect::default();
        self.got_box.rect = Rect::default();
        if area.width < 12 || area.height == 0 {
            return;
        }

        let rail = match view.focus {
            CaseFocus::Edit => Some(theme::accent()),
            CaseFocus::Delete => Some(theme::error()),
            CaseFocus::None if view.selected => Some(theme::accent()),
            CaseFocus::None => None,
        };
        if let Some(rail) = rail {
            let height = if self.collapsed { 1 } else { area.height };
            for y in area.y..area.y + height.min(area.height) {
                buf[(area.x, y)]
                    .set_symbol(icons::RAIL)
                    .set_style(Style::new().fg(rail));
            }
        }

        self.draw_header(buf, Rect { height: 1, ..area }, view);
        if self.collapsed {
            return;
        }

        let content_x = area.x + CONTENT_INDENT;
        let content_width = area.width.saturating_sub(CONTENT_INDENT + 1);
        let input_rows = self.input_lines() as u16;
        let output_rows = self.output_rows() as u16;
        let input_y = area.y + 1;
        let output_y = input_y + input_rows;

        // Expected and got share the row band. Got sits on a tinted panel
        // with a cell of padding inside, so the two read apart without a
        // label or a rule between them.
        let left_width = content_width.saturating_sub(GOT_GAP) / 2;
        let got_panel_x = content_x + left_width + GOT_GAP;

        // Hit areas take the label too, so a click on "in" focuses input.
        self.input_box.rect = Rect {
            x: area.x + 1,
            y: input_y,
            width: area.width.saturating_sub(2),
            height: input_rows,
        };
        self.expected_box.rect = Rect {
            x: area.x + 1,
            y: output_y,
            width: got_panel_x.saturating_sub(area.x + 1),
            height: output_rows,
        };
        self.got_box.rect = Rect {
            x: got_panel_x,
            y: output_y,
            width: area.right().saturating_sub(got_panel_x + 1),
            height: output_rows,
        };

        let editing = view.focus == CaseFocus::Edit;
        let input_active = editing && view.active_box == ActiveBox::Input;
        let expected_active = editing && view.active_box == ActiveBox::Expected;
        gutter_label(buf, area.x, input_y, "inp", input_active);
        gutter_label(buf, area.x, output_y, "exp", expected_active);
        Self::render_pane(
            buf,
            &mut self.inp_content,
            Rect {
                x: content_x,
                y: input_y,
                width: content_width,
                height: input_rows,
            },
            input_active,
            editing,
            "type the input",
        );
        Self::render_pane(
            buf,
            &mut self.exp_content,
            Rect {
                x: content_x,
                y: output_y,
                width: left_width,
                height: output_rows,
            },
            expected_active,
            editing,
            "type the expected output",
        );

        let panel = self.got_box.rect.intersection(area);
        if panel.width > GOT_PAD * 2 {
            ui::fill_buf(buf, panel, theme::panel());
            let got = Rect {
                x: panel.x + GOT_PAD,
                width: panel.width - GOT_PAD * 2,
                ..panel
            };
            let got_lines: Vec<Line<'static>> = self
                .got_view(view.state)
                .into_iter()
                .map(|line| clip_line(line, got.width as usize))
                .collect();
            Paragraph::new(Text::from(got_lines))
                .style(Style::new().fg(theme::text()))
                .render(got, buf);
        }
    }

    fn draw_header(&self, buf: &mut Buffer, row: Rect, view: &CaseView) {
        let number_style = match view.focus {
            CaseFocus::Delete => Style::new().fg(theme::error()).bold(),
            CaseFocus::Edit => Style::new().fg(theme::text()).bold(),
            CaseFocus::None if view.selected => Style::new().fg(theme::text()).bold(),
            CaseFocus::None => Style::new().fg(theme::muted()),
        };
        let chevron = if self.collapsed {
            icons::COLLAPSED
        } else {
            icons::EXPANDED
        };
        let compact = row.width < 56;
        let mut left = vec![
            Span::styled(format!("{:02} ", view.number), number_style),
            Span::styled(format!("{chevron} "), Style::new().fg(theme::faint())),
        ];
        match (self.verdict, view.state) {
            (CaseVerdict::Pending, RunState::Running) => {
                left.push(Span::styled(
                    format!("{} ", motion::spinner(view.epoch, view.now)),
                    Style::new().fg(theme::accent()),
                ));
                left.push(Span::styled("running", Style::new().fg(theme::muted())));
            }
            (CaseVerdict::Pending, RunState::Queued) => {
                left.push(Span::styled("queued", Style::new().fg(theme::faint())));
            }
            (CaseVerdict::Pending, RunState::Idle) => {}
            (verdict, _) => {
                let reveal = self
                    .revealed_at
                    .map_or(1.0, |at| motion::ease_out(at, motion::REVEAL, view.now));
                left.push(Span::styled(
                    verdict.label(compact),
                    Style::new()
                        .fg(theme::blend(theme::p().muted, verdict.rgb(), reveal))
                        .bold(),
                ));
            }
        }

        let right = if view.focus == CaseFocus::Delete {
            Line::from(Span::styled(
                "delete?",
                Style::new().fg(theme::error()).bold(),
            ))
        } else {
            let mut facts = Vec::new();
            if let Some(ms) = self.verdict.time_ms() {
                facts.push(status::format_ms(ms));
            }
            if let Some(kb) = self.memory_kb
                && !compact
            {
                facts.push(format_memory(kb));
            }
            Line::from(Span::styled(
                facts.join(" · "),
                Style::new().fg(theme::faint()),
            ))
        };
        ui::row_buf(
            buf,
            Rect {
                x: row.x + 2,
                width: row.width.saturating_sub(3),
                ..row
            },
            Line::from(left),
            right,
        );
    }

    /// Whether this verdict's output explains a failure (RE, CE, TLE, OLE).
    fn reports_error(&self) -> bool {
        matches!(
            self.verdict,
            CaseVerdict::RE | CaseVerdict::CE | CaseVerdict::Tle | CaseVerdict::Ole
        )
    }

    /// stdout is the answer, so it stays in the reading colour, unless an
    /// error verdict has no separate stderr and the text *is* the error.
    fn stdout_style(&self, has_stderr: bool) -> Style {
        if self.reports_error() && !has_stderr {
            Style::new().fg(theme::error())
        } else {
            Style::new()
        }
    }

    /// stderr: the error itself on a failing verdict, debug notes otherwise.
    fn stderr_style(&self) -> Style {
        if self.reports_error() {
            Style::new().fg(theme::error())
        } else {
            Style::new().fg(theme::muted())
        }
    }

    fn wa_diff_active(&self) -> bool {
        if !matches!(self.verdict, CaseVerdict::WA { .. }) || self.got_truncated() {
            return false;
        }
        self.got_content.as_deref().is_some_and(|got| {
            let (stdout, _) = split_stderr(got);
            !stdout.is_empty() && diff_fits(stdout)
        })
    }

    fn output_rows(&self) -> usize {
        self.expected_lines().max(self.got_lines())
    }

    /// The Got pane's rendered lines — the output capped to the pane budget,
    /// ending with a muted note whenever something had to be left out.
    ///
    /// Large WA outputs take the plain preview path: diffing even the capped
    /// preview on every redraw is not worth it, and the user still sees the
    /// first screenful plus a clear truncation note.
    fn got_view(&self, state: RunState) -> Vec<Line<'static>> {
        let Some(got) = &self.got_content else {
            let hint = match state {
                RunState::Running => "running…",
                RunState::Queued => "waiting for its turn",
                RunState::Idle => "not run yet",
            };
            return vec![Line::from(Span::styled(
                hint,
                Style::new().fg(theme::faint()),
            ))];
        };

        if got.is_empty() {
            return vec![Line::from(Span::styled(
                "no output",
                Style::new().fg(theme::faint()),
            ))];
        }

        let (stdout, stderr) = split_stderr(got);
        let stdout = Capped::new(stdout);
        let stderr = stderr.map(Capped::new);
        let diffed = self.wa_diff_active();

        let mut lines = if diffed {
            let expected = self.expected_text();
            highlight_got(&expected, &stdout.text)
        } else {
            // With no stderr section, an error verdict's whole text is the
            // error (a compiler message, the time-limit report).
            let style = self.stdout_style(stderr.is_some());
            let mut lines: Vec<Line<'static>> = stdout
                .text
                .lines()
                .map(|line| Line::styled(line.to_string(), style))
                .collect();
            if lines.is_empty() {
                lines.push(Line::default());
            }
            lines
        };

        if let Some(stderr) = &stderr {
            // stdout is the answer; stderr is commentary, kept visibly apart.
            if stdout.text.is_empty() && lines.len() == 1 && lines[0].width() == 0 {
                lines.clear();
            }
            lines.push(stderr_rule());
            let style = self.stderr_style();
            lines.extend(
                stderr
                    .text
                    .lines()
                    .map(|line| Line::styled(line.to_string(), style)),
            );
        }

        let skipped_diff =
            matches!(self.verdict, CaseVerdict::WA { .. }) && !diffed && !stdout.text.is_empty();
        if self.got_truncated {
            lines.push(muted(OUTPUT_TRUNCATED_NOTE.to_string()));
        } else if skipped_diff {
            lines.push(muted(DIFF_SKIPPED_NOTE.to_string()));
        } else {
            let mut caps = vec![stdout];
            if let Some(stderr) = stderr {
                caps.push(stderr);
            }
            if let Some(note) = truncation_note(&caps) {
                lines.push(note);
            }
        }
        lines
    }

    fn render_pane(
        buf: &mut Buffer,
        textarea: &mut TextArea<'static>,
        content: Rect,
        is_active: bool,
        is_editing: bool,
        placeholder: &str,
    ) {
        if content.width == 0 || content.height == 0 {
            return;
        }

        // Only the box being edited is rendered as a TextArea — ratatui-textarea
        // positions the terminal cursor on render, so rendering another TextArea
        // after it would steal the cursor.
        if is_editing && is_active {
            textarea.set_style(Style::new().fg(theme::text()));
            textarea.set_cursor_style(Style::new().add_modifier(Modifier::REVERSED));
            textarea.set_cursor_line_style(Style::new());
            textarea.set_placeholder_text(placeholder);
            textarea.set_placeholder_style(Style::new().fg(theme::faint()));
            textarea.set_block(Block::default());
            textarea.render(content, buf);
        } else {
            let text = textarea.lines().join("\n");
            let panel = if text.trim().is_empty() {
                Paragraph::new(Span::styled("empty", Style::new().fg(theme::faint())))
            } else {
                let width = content.width as usize;
                let lines: Vec<Line<'static>> = pane_lines(&text)
                    .into_iter()
                    .map(|line| clip_line(line, width))
                    .collect();
                Paragraph::new(Text::from(lines)).style(Style::new().fg(theme::text()))
            };
            panel.render(content, buf);
        }
    }

    /// Where each pane's text was drawn last frame (input, expected, got),
    /// without the labels: what a mouse selection in that pane may cover.
    pub fn text_regions(&self) -> [Rect; 3] {
        let label = CONTENT_INDENT - 1;
        let shift = |rect: Rect, by: u16, trim: u16| {
            if rect.width <= by + trim {
                return Rect::default();
            }
            Rect {
                x: rect.x + by,
                width: rect.width - by - trim,
                ..rect
            }
        };
        [
            shift(self.input_box.rect, label, 0),
            // The gap before the got panel closes the expected pane.
            shift(self.expected_box.rect, label, GOT_GAP),
            // Inside the got panel's padding.
            shift(self.got_box.rect, GOT_PAD, GOT_PAD),
        ]
    }

    pub fn handle_mouse(&mut self, mouse: &MouseEvent) -> bool {
        if let MouseEventKind::Down(MouseButton::Left) = mouse.kind
            && self.collapse_toggle_area.check_hover(mouse)
        {
            self.collapsed = !self.collapsed;
            return true;
        }

        self.area.check_hover(mouse)
    }

    /// Places the hit areas where this case's rows landed on screen and clips
    /// the ones scrolled out of view, after `tc_list` blitted a partial case.
    pub fn relocate_hit_areas(&mut self, dx: i32, dy: i32, visible: Rect) {
        let relocate = |state: &mut VisualState| {
            state.rect = offset_rect(state.rect, dx, dy).intersection(visible);
        };
        relocate(&mut self.area);
        relocate(&mut self.collapse_toggle_area);
        relocate(&mut self.input_box);
        relocate(&mut self.expected_box);
        relocate(&mut self.got_box);
    }
}

const DIFF_MAX_LINES: usize = 300;
const DIFF_MAX_CHARS: usize = 24_000;
/// Inline preview kept in the live UI state. Anything larger is cut as soon
/// as the result arrives, before the redraw loop can store or diff it.
const INLINE_PREVIEW_MAX_BYTES: usize = 24_000;
/// Full output is retained in memory for the dialog up to this budget.
/// Larger outputs are spilled to a temp file and read page by page.
const DIALOG_SAFE_MAX_BYTES: usize = 10_000_000;
/// A huge number of short lines is just as bad as a huge byte count.
const DIALOG_SAFE_MAX_LINES: usize = 20_000;
const LARGE_OUTPUT_WARNING: &str = "Output is too large to show fully. Showing a bounded preview.";
pub(crate) const PAGED_OUTPUT_NOTE: &str = "Large output · paged view · diff is computed per page";
/// A WA diff is only useful on a screenful. Larger previews are shown plain.
const WA_DIFF_MAX_CHARS: usize = 8_000;
/// A collapsed case is a single row, so a run of accepted cases reads as a
/// compact table.
const COLLAPSED_HEIGHT: usize = 1;
/// Space between the expected text and the got panel.
const GOT_GAP: u16 = 2;
/// Padding inside the got panel, left and right.
const GOT_PAD: u16 = 1;

/// A pane label in the gutter under the case number (`01 ▾`), centred in
/// it. The pane being edited gets the accent.
fn gutter_label(buf: &mut Buffer, card_x: u16, y: u16, label: &str, active: bool) {
    let style = if active {
        Style::new().fg(theme::accent()).bold()
    } else {
        Style::new().fg(theme::faint())
    };
    let width = label.chars().count() as u16;
    // The gutter runs from after the rail to the cell before the content.
    let gutter = CONTENT_INDENT - 2;
    let x = card_x + 1 + gutter.saturating_sub(width).div_ceil(2);
    Paragraph::new(Span::styled(label.to_string(), style)).render(
        Rect {
            x,
            y,
            width,
            height: 1,
        },
        buf,
    );
}
/// The header and the gap after the card; labels live in the gutter.
const CARD_CHROME_ROWS: usize = 2;
/// Content starts under the verdict word: rail, gap, `02 `, `▾ `.
const CONTENT_INDENT: u16 = 7;
pub(crate) const OUTPUT_TRUNCATED_NOTE: &str = "… output truncated";
const DIFF_SKIPPED_NOTE: &str = "… diff skipped for large output";

/// Separates a case's stdout from the stderr that `helper` appends to it.
pub(crate) const STDERR_MARKER: &str = "\n── stderr ──\n";

/// `812 KB`, `3.4 MB`, `256 MB`.
pub(crate) fn format_memory(kb: u64) -> String {
    if kb < 1024 {
        format!("{kb} KB")
    } else if kb < 100 * 1024 {
        format!("{:.1} MB", kb as f64 / 1024.0)
    } else {
        format!("{} MB", kb / 1024)
    }
}

/// Characters that differ from the expected answer: red on a dark red wash,
/// so the mismatch is visible even where the glyph is a space.
fn extra_style() -> Style {
    Style::new().fg(theme::error()).bg(theme::diff())
}

/// A note the pane adds about itself (truncation, skipped diff).
fn muted(text: String) -> Line<'static> {
    Line::from(Span::styled(text, Style::new().fg(theme::faint())))
}

fn stderr_rule() -> Line<'static> {
    Line::from(Span::styled("── stderr", Style::new().fg(theme::faint())))
}

fn split_stderr(got: &str) -> (&str, Option<&str>) {
    match got.split_once(STDERR_MARKER) {
        Some((stdout, stderr)) => (stdout, Some(stderr)),
        None => (got, None),
    }
}

fn has_stdout(got: &str) -> bool {
    !split_stderr(got).0.is_empty()
}

/// Takes ownership of the raw output but keeps only a bounded prefix, so a
/// giant WA/RE/AC output cannot sit in the live test-case list.
fn retain_output_preview(mut got: String) -> (String, bool) {
    if got.len() <= INLINE_PREVIEW_MAX_BYTES {
        return (got, false);
    }
    let mut end = INLINE_PREVIEW_MAX_BYTES;
    while end > 0 && !got.is_char_boundary(end) {
        end -= 1;
    }
    got.truncate(end);
    (got, true)
}

/// The WA diff path is bounded independently of the retention cap: once the
/// preview is larger than a screenful, plain rendering is both faster and
/// easier to read.
fn diff_fits(text: &str) -> bool {
    text.chars().take(WA_DIFF_MAX_CHARS + 1).count() <= WA_DIFF_MAX_CHARS
}

/// Text after the shared pane budget is applied.
struct Capped {
    text: String,
    raw_lines: usize,
    shown_lines: usize,
    chars_cut: bool,
}

impl Capped {
    fn hidden_lines(&self) -> usize {
        self.raw_lines.saturating_sub(self.shown_lines)
    }

    /// Output text capped to `DIFF_MAX_LINES` lines (trailing whitespace
    /// stripped) and `DIFF_MAX_CHARS` characters.
    fn new(text: &str) -> Self {
        Self::build(text, false)
    }

    /// Like `new`, but keeps the empty line a textarea shows after `Enter`.
    fn preserving_trailing(text: &str) -> Self {
        Self::build(text, true)
    }

    fn build(text: &str, preserve_trailing: bool) -> Self {
        let raw: Vec<&str> = if preserve_trailing {
            text.split('\n').collect()
        } else {
            text.lines().collect()
        };
        let raw_lines = raw.len();
        let mut capped = raw
            .iter()
            .take(DIFF_MAX_LINES)
            .map(|line| line.trim_end())
            .collect::<Vec<_>>()
            .join("\n");

        let chars_cut = capped.chars().count() > DIFF_MAX_CHARS;
        if chars_cut {
            capped = capped.chars().take(DIFF_MAX_CHARS).collect();
        }

        let shown_lines = if preserve_trailing {
            capped.split('\n').count()
        } else {
            capped.lines().count()
        };

        Self {
            shown_lines,
            text: capped,
            raw_lines,
            chars_cut,
        }
    }
}

/// One muted row telling the user what the pane left out, if anything.
fn truncation_note(caps: &[Capped]) -> Option<Line<'static>> {
    let hidden: usize = caps.iter().map(|capped| capped.hidden_lines()).sum();
    let total: usize = caps.iter().map(|capped| capped.raw_lines).sum();
    let chars_cut = caps.iter().any(|capped| capped.chars_cut);

    if hidden > 0 {
        Some(muted(format!(
            "… {hidden} more lines hidden ({total} total)"
        )))
    } else if chars_cut {
        Some(muted(OUTPUT_TRUNCATED_NOTE.to_string()))
    } else {
        None
    }
}

/// Lines an Input/Expected pane shows for `text`, capped with a note when the
/// content would not fit — mirroring what `got_view` does for Got.
fn pane_lines(text: &str) -> Vec<Line<'static>> {
    let capped = Capped::preserving_trailing(text);
    let mut lines: Vec<Line<'static>> = capped
        .text
        .split('\n')
        .map(|line| Line::from(line.to_string()))
        .collect();
    if let Some(note) = truncation_note(std::slice::from_ref(&capped)) {
        lines.push(note);
    }
    lines
}

/// Cuts a rendered line to `max_width` columns, ending with a muted `…` when
/// the content continues past the pane edge.
fn clip_line(line: Line<'static>, max_width: usize) -> Line<'static> {
    if max_width == 0 || line.width() <= max_width {
        return line;
    }

    let budget = max_width - 1;
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut used = 0;
    for grapheme in line.styled_graphemes(Style::default()) {
        let width = Span::raw(grapheme.symbol).width();
        if used + width > budget {
            break;
        }
        used += width;
        match spans.last_mut() {
            Some(last) if last.style == grapheme.style => {
                last.content.to_mut().push_str(grapheme.symbol)
            }
            _ => spans.push(Span::styled(grapheme.symbol.to_string(), grapheme.style)),
        }
    }
    spans.push(Span::styled("…", Style::new().fg(theme::faint())));
    Line::from(spans)
}

fn offset_rect(rect: Rect, dx: i32, dy: i32) -> Rect {
    let x = rect.x as i32 + dx;
    let y = rect.y as i32 + dy;
    if x < 0 || y < 0 {
        return Rect::default();
    }
    Rect {
        x: x as u16,
        y: y as u16,
        ..rect
    }
}

/// The Got pane on WA: the program's output, exactly as printed. Each line is
/// compared with the expected line at the same index; within a line, a
/// character is painted red when it differs at that position. Missing
/// characters are never invented.
pub(crate) fn highlight_got(expected: &str, got: &str) -> Vec<Line<'static>> {
    highlight_got_inner(expected, got, true)
}

/// The dialog variant: no pane cap, so a safe output keeps its complete diff.
fn highlight_got_full(expected: &str, got: &str) -> Vec<Line<'static>> {
    highlight_got_inner(expected, got, false)
}

fn highlight_got_inner(expected: &str, got: &str, capped: bool) -> Vec<Line<'static>> {
    let got = got.split(STDERR_MARKER).next().unwrap_or(got);
    let expected = if capped {
        Capped::new(expected).text
    } else {
        expected.to_string()
    };
    let got = if capped {
        Capped::new(got).text
    } else {
        got.to_string()
    };

    let expected_lines: Vec<&str> = expected.lines().collect();
    let got_lines: Vec<&str> = got.lines().collect();

    let mut lines = Vec::with_capacity(got_lines.len().max(1));
    for (index, got_line) in got_lines.iter().enumerate() {
        lines.push(match expected_lines.get(index) {
            Some(expected_line) => line_from_runs(line_runs(expected_line, got_line)),
            None => {
                let text = got_line.trim_end();
                if text.is_empty() {
                    Line::default()
                } else {
                    line_from_runs(vec![(true, text.to_string())])
                }
            }
        });
    }
    if lines.is_empty() {
        lines.push(Line::default());
    }
    lines
}

fn line_runs(expected: &str, got: &str) -> Vec<(bool, String)> {
    let expected: Vec<char> = expected.trim_end().chars().collect();
    let got: Vec<char> = got.trim_end().chars().collect();
    let mut matched = vec![false; got.len()];

    // Align characters within the already-aligned line. This prevents one
    // insertion from painting the entire suffix red, while the size guard
    // keeps pathological single lines out of an O(n*m) diff.
    if expected.len().saturating_mul(got.len()) <= 40_000 {
        let mut dp = vec![vec![0usize; got.len() + 1]; expected.len() + 1];
        for i in (0..expected.len()).rev() {
            for j in (0..got.len()).rev() {
                dp[i][j] = if expected[i] == got[j] {
                    dp[i + 1][j + 1] + 1
                } else {
                    dp[i + 1][j].max(dp[i][j + 1])
                };
            }
        }
        let (mut i, mut j) = (0, 0);
        while i < expected.len() && j < got.len() {
            if expected[i] == got[j] {
                matched[j] = true;
                i += 1;
                j += 1;
            } else if dp[i + 1][j] >= dp[i][j + 1] {
                i += 1;
            } else {
                j += 1;
            }
        }
    } else {
        for (index, ch) in got.iter().enumerate() {
            matched[index] = expected.get(index) == Some(ch);
        }
    }

    let mut runs: Vec<(bool, String)> = Vec::new();
    for (index, ch) in got.into_iter().enumerate() {
        let red = !matched[index];
        match runs.last_mut() {
            Some((last_red, text)) if *last_red == red => text.push(ch),
            _ => runs.push((red, String::from(ch))),
        }
    }
    runs
}

fn line_from_runs(runs: Vec<(bool, String)>) -> Line<'static> {
    let spans: Vec<Span<'static>> = runs
        .into_iter()
        .map(|(red, text)| Span::styled(text, if red { extra_style() } else { Style::new() }))
        .collect();
    Line::from(spans)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend};

    #[test]
    fn height_tracks_content_and_collapse() {
        let mut tc = TestCase::new("1 2", "3");
        // Header, 1 input line, 1 output line, the gap after the card.
        assert_eq!(tc.row_height(), 4);

        tc.inp_content.insert_str("\n4 5");
        assert_eq!(tc.row_height(), 5);

        tc.set_result(CaseVerdict::WA { ms: 1 }, "0\n1\n2".into());
        assert_eq!(tc.row_height(), 7);

        // An accepted case folds to a single row.
        tc.set_result(CaseVerdict::AC { ms: 1 }, "3".into());
        assert!(tc.collapsed);
        assert_eq!(tc.row_height(), 1);
    }

    #[test]
    fn editing_caps_a_tall_case_to_the_viewport() {
        let input = (0..100)
            .map(|i| format!("line-{i:03}"))
            .collect::<Vec<_>>()
            .join("\n");
        let mut tc = TestCase::new(&input, "ok");
        let full = tc.row_height();
        assert!(full > 14);

        tc.fit_to_height(14);
        assert!(
            tc.row_height() <= 14,
            "the active case must fit the list viewport"
        );
        tc.clear_height_caps();
        assert_eq!(tc.row_height(), full);
    }

    fn view(focus: CaseFocus, active_box: ActiveBox, selected: bool, state: RunState) -> CaseView {
        // A reveal that finished long ago: colours are at rest.
        let now = Instant::now() + std::time::Duration::from_secs(5);
        CaseView {
            number: 1,
            focus,
            active_box,
            selected,
            state,
            epoch: now,
            now,
        }
    }

    fn draw(tc: &mut TestCase, view: &CaseView) -> (Buffer, String) {
        let area = Rect::new(0, 0, 80, tc.row_height() as u16);
        let mut buffer = Buffer::empty(area);
        tc.draw_tc(&mut buffer, area, view);
        let title: String = (0..area.width).map(|x| buffer[(x, 0)].symbol()).collect();
        (buffer, title)
    }

    #[test]
    fn the_rail_marks_focus_only() {
        let mut tc = TestCase::new("1", "2");
        tc.set_result(CaseVerdict::WA { ms: 1 }, "0".into());
        tc.collapsed = false;

        let (buffer, _) = draw(
            &mut tc,
            &view(CaseFocus::None, ActiveBox::None, false, RunState::Idle),
        );
        assert_eq!(
            buffer[(0, 0)].symbol(),
            " ",
            "a verdict alone draws no rail"
        );

        let (buffer, _) = draw(
            &mut tc,
            &view(CaseFocus::None, ActiveBox::None, true, RunState::Idle),
        );
        assert_eq!(buffer[(0, 0)].symbol(), icons::RAIL);
        assert_eq!(
            buffer[(0, 0)].fg,
            theme::accent(),
            "selection is the accent"
        );

        let (buffer, title) = draw(
            &mut tc,
            &view(CaseFocus::Delete, ActiveBox::None, false, RunState::Idle),
        );
        assert_eq!(
            buffer[(0, 0)].fg,
            theme::error(),
            "the delete target is red"
        );
        assert!(title.contains("delete?"), "{title}");
        let height = tc.row_height() as u16;
        assert_eq!(
            buffer[(0, height - 1)].symbol(),
            icons::RAIL,
            "an expanded card's rail runs its full height"
        );
    }

    #[test]
    fn each_verdict_is_one_word_in_its_colour() {
        for (verdict, word, rgb) in [
            (CaseVerdict::AC { ms: 4 }, "accepted", theme::p().success),
            (CaseVerdict::WA { ms: 4 }, "wrong answer", theme::p().error),
            (CaseVerdict::Tle, "time limit", theme::p().warning),
            (CaseVerdict::RE, "runtime error", theme::p().error),
            (CaseVerdict::Ole, "output limit", theme::p().error),
            (CaseVerdict::CE, "compile error", theme::p().modified),
        ] {
            let mut tc = TestCase::new("1", "2");
            tc.set_result(verdict, "0".into());
            let (buffer, title) = draw(
                &mut tc,
                &view(CaseFocus::None, ActiveBox::None, false, RunState::Idle),
            );
            let x = title
                .find(word)
                .unwrap_or_else(|| panic!("{word} in {title}")) as u16;
            assert_eq!(buffer[(x, 0)].fg, theme::paint(rgb), "{verdict:?}");
            if verdict.time_ms().is_some() {
                assert!(title.contains("4 ms"), "{title}");
            }
        }
    }

    #[test]
    fn a_fresh_verdict_colours_in() {
        let mut tc = TestCase::new("1", "2");
        tc.set_result(CaseVerdict::WA { ms: 1 }, "0".into());
        tc.collapsed = false;
        let now = Instant::now();
        let fresh = CaseView {
            now,
            epoch: now,
            ..view(CaseFocus::None, ActiveBox::None, false, RunState::Idle)
        };
        let (buffer, title) = draw(&mut tc, &fresh);
        let x = title.find("wrong answer").unwrap() as u16;
        if motion::enabled() {
            assert_ne!(buffer[(x, 0)].fg, theme::error(), "starts from muted");
        }
        assert!(tc.revealing(now) || !motion::enabled());
        assert!(!tc.revealing(now + motion::REVEAL));
    }

    #[test]
    fn running_and_queued_cases_say_so() {
        let mut tc = TestCase::new("1", "2");
        let (_, title) = draw(
            &mut tc,
            &view(CaseFocus::None, ActiveBox::None, false, RunState::Running),
        );
        assert!(title.contains("running"), "{title}");
        let (_, title) = draw(
            &mut tc,
            &view(CaseFocus::None, ActiveBox::None, false, RunState::Queued),
        );
        assert!(title.contains("queued"), "{title}");
        let (_, title) = draw(
            &mut tc,
            &view(CaseFocus::None, ActiveBox::None, false, RunState::Idle),
        );
        assert!(
            !title.contains("running") && !title.contains("queued"),
            "{title}"
        );
    }

    #[test]
    fn editing_marks_the_active_pane() {
        let mut tc = TestCase::new("1", "2");
        tc.set_result(CaseVerdict::WA { ms: 1 }, "0".into());
        tc.collapsed = false;
        let (buffer, _) = draw(
            &mut tc,
            &view(CaseFocus::Edit, ActiveBox::Input, false, RunState::Idle),
        );
        // Labels are centred in the gutter under `01 ▾`: "in" at 3..5.
        let input = tc.input_box.rect;
        assert_eq!(buffer[(3, input.y)].symbol(), "i");
        let label_end = 4;
        assert_eq!(
            buffer[(label_end, input.y)].fg,
            theme::accent(),
            "the active pane's label is the accent"
        );
        let expected = tc.expected_box.rect;
        assert_eq!(buffer[(label_end, expected.y)].fg, theme::faint());
    }

    #[test]
    fn panes_are_labelled_and_divided() {
        let mut tc = TestCase::new("5", "10");
        tc.set_result(CaseVerdict::WA { ms: 9 }, "0".into());
        tc.collapsed = false;
        let (buffer, _) = draw(
            &mut tc,
            &view(CaseFocus::None, ActiveBox::None, false, RunState::Idle),
        );
        let screen: Vec<String> = (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect()
            })
            .collect();
        let text = screen.join("\n");
        for word in [" in  5", "exp  10"] {
            assert!(text.contains(word), "{word} missing:\n{text}");
        }
        // Got has no label: its tinted panel sets it apart.
        assert!(!text.contains("got"), "{text}");
        let got = tc.got_box.rect;
        assert_eq!(buffer[(got.x, got.y)].bg, theme::panel());
        let [_, _, got_text] = tc.text_regions();
        assert_eq!(buffer[(got_text.x, got_text.y)].symbol(), "0");
        assert_eq!(
            tc.text_regions()[0].x,
            CONTENT_INDENT,
            "text starts under the verdict"
        );
    }

    #[test]
    fn a_single_enter_grows_the_input_pane() {
        let mut tc = TestCase::new("1", "");
        let before = tc.row_height();
        tc.inp_content.insert_newline();
        assert_eq!(
            tc.row_height(),
            before + 1,
            "the empty line under the cursor must count"
        );
    }

    fn red_spans(line: &Line<'static>) -> Vec<String> {
        line.spans
            .iter()
            .filter(|span| span.style.fg == Some(theme::error()))
            .map(|span| span.content.to_string())
            .collect()
    }

    fn contents<'a>(line: &'a Line<'static>) -> Vec<&'a str> {
        line.spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect()
    }

    #[test]
    fn diff_marks_got_chars_out_of_place() {
        let got = highlight_got("3 4 5", "3 9 7");
        assert_eq!(got.len(), 1);
        assert_eq!(contents(&got[0]), vec!["3 ", "9", " ", "7"]);
        assert_eq!(red_spans(&got[0]), vec!["9", "7"]);
        for span in &got[0].spans {
            let differs = span.style.fg == Some(theme::error());
            assert_eq!(
                span.style.bg,
                differs.then(theme::diff),
                "only differing characters get the dark red wash: {span:?}"
            );
        }
    }

    #[test]
    fn diff_marks_the_shifted_tail_after_a_missing_newline() {
        // With the line break missing, every following character sits one
        // place off — so all of it is marked.
        let got = highlight_got(
            "hi there\nthis is kushal goyal",
            "hi therethis is kushal goyal",
        );
        assert_eq!(got.len(), 1);
        assert_eq!(contents(&got[0]), vec!["hi there", "this is kushal goyal"]);
        assert_eq!(red_spans(&got[0]), vec!["this is kushal goyal"]);
    }

    #[test]
    fn diff_never_invents_missing_characters() {
        // expected "lskdjf" vs got "lkej": l, k, and j can be aligned; the
        // missing s/d/f never appear in the pane, and e stays red.
        let got = highlight_got("lskdjf", "lkej");
        assert_eq!(got.len(), 1);
        assert_eq!(contents(&got[0]), vec!["lk", "e", "j"]);
        assert_eq!(red_spans(&got[0]), vec!["e"]);

        // expected "lskjf" vs got "lkej": the trailing j lands correctly, so
        // only e is red.
        let got = highlight_got("lskjf", "lkej");
        assert_eq!(contents(&got[0]), vec!["lk", "e", "j"]);
        assert_eq!(red_spans(&got[0]), vec!["e"]);
    }

    #[test]
    fn diff_is_positional_across_lines() {
        let got = highlight_got("a\nb\nc", "a\nx\nb\nc");
        assert_eq!(got.len(), 4);
        assert!(red_spans(&got[0]).is_empty());
        assert_eq!(red_spans(&got[1]), vec!["x"]);
        assert_eq!(red_spans(&got[2]), vec!["b"]);
        assert_eq!(red_spans(&got[3]), vec!["c"]);

        let got = highlight_got("a\nb\nc", "a\nc");
        assert_eq!(got.len(), 2);
        assert!(red_spans(&got[0]).is_empty());
        assert_eq!(red_spans(&got[1]), vec!["c"]);
    }

    #[test]
    fn diff_does_not_shift_identical_lines() {
        let got = highlight_got("abc\n144\ntail", "abcdef\n144\ntail");
        assert_eq!(got.len(), 3);
        assert_eq!(red_spans(&got[0]), vec!["def"]);
        assert!(
            red_spans(&got[1]).is_empty(),
            "the identical 144 line stays plain"
        );
        assert!(red_spans(&got[2]).is_empty());
    }

    #[test]
    fn diff_matches_characters_within_each_line() {
        let got = highlight_got("1\n2\n0\n3", "0 1\n2 1\n1 0\n5 3");
        assert_eq!(contents(&got[0]), vec!["0 ", "1"]);
        assert_eq!(red_spans(&got[0]), vec!["0 "]);
        assert_eq!(contents(&got[1]), vec!["2", " 1"]);
        assert_eq!(red_spans(&got[1]), vec![" 1"]);
        assert_eq!(contents(&got[2]), vec!["1 ", "0"]);
        assert_eq!(red_spans(&got[2]), vec!["1 "]);
        assert_eq!(contents(&got[3]), vec!["5 ", "3"]);
        assert_eq!(red_spans(&got[3]), vec!["5 "]);
    }

    #[test]
    fn diff_ignores_trailing_whitespace() {
        let got = highlight_got("3  \n4", "3\n4");
        assert!(got.iter().all(|line| red_spans(line).is_empty()));
    }

    #[test]
    fn capped_reports_hidden_lines_and_cut_characters() {
        let many = (0..500)
            .map(|i| i.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        let capped = Capped::new(&many);
        assert_eq!(capped.raw_lines, 500);
        assert_eq!(capped.shown_lines, DIFF_MAX_LINES);
        assert_eq!(capped.hidden_lines(), 200);
        assert!(!capped.chars_cut);

        let capped = Capped::new(&"x".repeat(DIFF_MAX_CHARS + 10));
        assert!(capped.chars_cut);
        assert_eq!(capped.shown_lines, 1);

        let fits = Capped::new("a\nb");
        assert!(!fits.chars_cut);
        assert_eq!(fits.hidden_lines(), 0);
        assert!(truncation_note(std::slice::from_ref(&fits)).is_none());
    }

    #[test]
    fn pane_lines_add_a_note_only_when_something_is_cut() {
        let small = pane_lines("a\nb");
        assert_eq!(small.len(), 2);

        let many = (0..500)
            .map(|i| i.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        let capped = pane_lines(&many);
        assert_eq!(capped.len(), DIFF_MAX_LINES + 1);
        assert!(
            capped
                .last()
                .unwrap()
                .to_string()
                .contains("200 more lines hidden")
        );
    }

    #[test]
    fn clip_line_marks_what_does_not_fit() {
        let line = Line::from(vec![
            Span::styled("abcd", extra_style()),
            Span::styled("ef", Style::new()),
        ]);
        let clipped = clip_line(line, 5);
        assert_eq!(clipped.width(), 5);
        assert_eq!(contents(&clipped), vec!["abcd", "…"]);
        assert_eq!(red_spans(&clipped), vec!["abcd"]);

        let fits = clip_line(Line::from("abc"), 3);
        assert_eq!(contents(&fits), vec!["abc"], "no marker when the line fits");
    }

    fn screen_of(terminal: &Terminal<TestBackend>) -> String {
        let buffer = terminal.backend().buffer();
        let area = buffer.area;
        (0..area.height)
            .map(|y| {
                (0..area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn giant_output_is_sized_to_the_capped_pane() {
        // The reproduction from a real run: n=1000 prints 1000 lines of 2000
        // characters. The case must stay small — the pane shows a bounded
        // preview plus a note, not 1000 mostly-empty rows.
        let mut tc = TestCase::new("1", "");
        let line = format!("A{}0", " ".repeat(1998));
        tc.set_result(CaseVerdict::WA { ms: 7 }, vec![line; 1000].join("\n"));

        assert!(tc.got_truncated());
        assert!(tc.got_content.as_ref().unwrap().len() <= INLINE_PREVIEW_MAX_BYTES);
        let rows = tc.got_lines();
        assert!(
            rows < 20,
            "huge output must not inflate the pane: {rows} rows"
        );
        assert_eq!(tc.row_height(), CARD_CHROME_ROWS + 1 + rows);
        assert!(
            tc.got_view(RunState::Idle)
                .last()
                .unwrap()
                .to_string()
                .contains("output truncated")
        );

        let mut terminal = Terminal::new(TestBackend::new(80, 40)).unwrap();
        let area = Rect::new(0, 0, 80, tc.row_height() as u16);
        let look = view(CaseFocus::None, ActiveBox::None, false, RunState::Idle);
        terminal
            .draw(|frame| tc.draw_tc(frame.buffer_mut(), area, &look))
            .unwrap();

        let screen = screen_of(&terminal);
        if std::env::var("ZCH_DUMP").is_ok() {
            println!("{screen}");
        }
        assert!(screen.contains("output truncated"), "screen:\n{screen}");
    }

    #[test]
    fn huge_wa_output_is_cut_on_arrival_and_never_diffed() {
        let mut tc = TestCase::new("1", "1");
        let huge = "9".repeat(2_000_000);
        tc.set_result(CaseVerdict::WA { ms: 7 }, huge);

        assert!(tc.got_truncated());
        assert!(tc.got_content.as_ref().unwrap().len() <= INLINE_PREVIEW_MAX_BYTES);
        assert!(tc.pane_text(PaneKind::Got).len() <= INLINE_PREVIEW_MAX_BYTES);
        assert!(
            !tc.wa_diff_active(),
            "huge WA output must bypass the diff path"
        );

        let view = tc.got_view(RunState::Idle);
        assert!(view.len() <= DIFF_MAX_LINES + 2);
        assert!(view.iter().all(|line| red_spans(line).is_empty()));
        assert!(
            view.last()
                .unwrap()
                .to_string()
                .contains("output truncated")
        );

        // Repeated redraws walk only the retained 24 KB preview, never the
        // original 2 MB string.
        for _ in 0..20 {
            assert_eq!(tc.got_view(RunState::Idle).len(), view.len());
        }
    }

    #[test]
    fn wa_previews_over_the_diff_budget_are_plain_not_diffed() {
        let mut tc = TestCase::new("x", "y");
        tc.set_result(
            CaseVerdict::WA { ms: 1 },
            "a".repeat(WA_DIFF_MAX_CHARS + 10),
        );

        assert!(!tc.got_truncated(), "the retention cap was not reached");
        assert!(!tc.wa_diff_active(), "large previews must not be diffed");
        let view = tc.got_view(RunState::Idle);
        assert!(view.iter().all(|line| red_spans(line).is_empty()));
        assert!(view.last().unwrap().to_string().contains("diff skipped"));
    }

    #[test]
    fn safe_large_output_is_retained_for_the_dialog_but_not_the_live_pane() {
        let mut tc = TestCase::new("x", "y");
        let line = "A".repeat(60);
        let output = (0..600)
            .map(|_| line.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(output.len() > INLINE_PREVIEW_MAX_BYTES);
        assert!(output.len() <= DIALOG_SAFE_MAX_BYTES);
        tc.set_result(CaseVerdict::WA { ms: 1 }, output);

        assert!(tc.got_truncated());
        assert!(!tc.output_too_large);
        assert!(tc.got_content.as_ref().unwrap().len() <= INLINE_PREVIEW_MAX_BYTES);
        assert!(
            tc.got_view(RunState::Idle)
                .last()
                .unwrap()
                .to_string()
                .contains("output truncated")
        );

        let (lines, warning) = tc.got_dialog_view();
        assert!(warning.is_none(), "safe output must not warn");
        assert_eq!(lines.len(), 600, "the dialog keeps the full output");
        assert!(
            !red_spans(&lines[0]).is_empty(),
            "the dialog keeps the full WA diff"
        );
    }

    #[test]
    fn output_over_the_line_budget_is_paged_instead_of_dropped() {
        let mut tc = TestCase::new("x", "y");
        let output = (0..=DIALOG_SAFE_MAX_LINES)
            .map(|_| "A")
            .collect::<Vec<_>>()
            .join("\n");
        assert!(output.len() < DIALOG_SAFE_MAX_BYTES);
        tc.set_result(CaseVerdict::WA { ms: 1 }, output);

        assert!(tc.got_truncated());
        assert!(!tc.output_too_large);
        assert!(tc.got_is_paged());
        assert_eq!(tc.got_line_count(), DIALOG_SAFE_MAX_LINES + 1);

        let (lines, warning) = tc.got_dialog_view();
        assert_eq!(warning, Some(PAGED_OUTPUT_NOTE));
        assert_eq!(
            lines.len(),
            GOT_PAGE_LINES,
            "the dialog reads one page at a time"
        );
        assert!(
            !red_spans(&lines[0]).is_empty(),
            "paged WA output still diffs"
        );
    }

    #[test]
    fn one_e7_output_stays_in_memory_with_a_full_dialog_diff() {
        let mut tc = TestCase::new("x", "y");
        let output = "A".repeat(DIALOG_SAFE_MAX_BYTES);
        tc.set_result(CaseVerdict::WA { ms: 1 }, output);

        assert!(!tc.got_is_paged());
        assert!(!tc.output_too_large);
        let (lines, warning) = tc.got_dialog_view();
        assert!(warning.is_none());
        assert_eq!(lines.len(), 1);
        assert!(!red_spans(&lines[0]).is_empty());
    }

    #[test]
    fn empty_or_stderr_only_huge_output_does_not_warn() {
        let mut tc = TestCase::new("x", "y");
        tc.set_result(CaseVerdict::WA { ms: 1 }, String::new());
        assert!(tc.got_dialog_view().1.is_none());

        let mut tc = TestCase::new("x", "y");
        let stderr = format!(
            "{}{}",
            STDERR_MARKER,
            (0..=DIALOG_SAFE_MAX_LINES)
                .map(|_| "e")
                .collect::<Vec<_>>()
                .join("\n")
        );
        tc.set_result(CaseVerdict::WA { ms: 1 }, stderr);
        assert!(tc.got_is_paged());
        assert!(
            tc.got_dialog_view().1 != Some(LARGE_OUTPUT_WARNING),
            "stderr-only output has no stdout to warn about"
        );
    }
}
