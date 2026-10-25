use std::{
    fs::{self, File},
    io::{self, BufRead, BufReader, BufWriter, Read, Seek, SeekFrom, Write},
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

use ratatui::{
    buffer::Buffer, crossterm::event::{MouseButton, MouseEvent, MouseEventKind}, layout::{Constraint::{Length, Percentage}, HorizontalAlignment::{Center, Left, Right}, Layout, Rect}, style::{Color, Modifier, Style}, text::{Line, Span, Text}, widgets::{BorderType, Borders, Padding, Paragraph, Widget},
};
use ratatui_textarea::TextArea;

use crate::components::{VisualState, block, theme};

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
    AC { ms: u128 },
    WA { ms: u128 },
    Tle,
    RE,
    CE,
}

impl CaseVerdict {
    pub fn color(&self) -> Color {
        match self {
            CaseVerdict::Pending => theme::MUTED,
            CaseVerdict::AC { .. } => theme::GREEN,
            CaseVerdict::WA { .. } => theme::RED,
            CaseVerdict::Tle => theme::YELLOW,
            CaseVerdict::RE => theme::PURPLE,
            CaseVerdict::CE => theme::ORANGE,
        }
    }

    pub fn chip(&self) -> String {
        match self {
            CaseVerdict::Pending => String::new(),
            CaseVerdict::AC { ms } => format!(" ✔ AC · {ms}ms "),
            CaseVerdict::WA { ms } => format!(" ✘ WA · {ms}ms "),
            CaseVerdict::Tle => " ⏱ TLE ".to_string(),
            CaseVerdict::RE => " ✘ RE ".to_string(),
            CaseVerdict::CE => " ✘ CE ".to_string(),
        }
    }

    pub fn is_ac(&self) -> bool {
        matches!(self, CaseVerdict::AC { .. })
    }

    pub fn is_failed(&self) -> bool {
        matches!(self, CaseVerdict::WA { .. } | CaseVerdict::Tle | CaseVerdict::RE | CaseVerdict::CE)
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
        if let Err(error) = writer.write_all(got.as_bytes()).and_then(|_| writer.flush()) {
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
            got[..position].bytes().filter(|&byte| byte == b'\n').count() + 1
        });

        Ok(Self { path, line_starts, line_count, stderr_line })
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
            let Some((text, truncated)) = read_line_limited(&mut reader, GOT_PAGE_LINE_MAX_BYTES)? else {
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
    Ok(Some((String::from_utf8_lossy(&buffer).to_string(), truncated)))
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
        self.got_view().len().max(1)
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
        self.expected_lines_uncapped().max(self.got_lines_uncapped())
    }

    fn full_height(&self) -> usize {
        1 + (self.input_lines() + 2) + (self.output_rows() + 2)
    }

    /// While editing, the active case may not grow taller than the list
    /// viewport. Pane heights are split from the content itself, never from
    /// which pane has focus, so pressing Tab does not make Input shrink.
    /// Both panes scroll internally when they need more rows.
    pub fn fit_to_height(&mut self, viewport: usize) {
        let budget = viewport.saturating_sub(5).max(1);
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
        if self.collapsed { COLLAPSED_HEIGHT } else { self.full_height() }
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
        self.collapsed = verdict.is_ac();
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
        let Some(file) = &self.got_file else { return Vec::new() };
        let Ok(page) = file.read_page(start, count) else { return Vec::new() };
        let expected = self.exp_content.lines();
        let diff = matches!(self.verdict, CaseVerdict::WA { .. });

        page.into_iter()
            .enumerate()
            .map(|(offset, line)| {
                let index = start + offset;
                let mut rendered = if file.stderr_line == Some(index) {
                    muted("── stderr ──".to_string())
                } else if diff && file.stderr_line.is_none_or(|stderr| index < stderr) {
                    let expected = expected.get(index).map(String::as_str).unwrap_or("");
                    highlight_got_inner(expected, &line.text, false)
                        .into_iter()
                        .next()
                        .unwrap_or_default()
                } else {
                    let style = if matches!(self.verdict, CaseVerdict::CE | CaseVerdict::RE) {
                        Style::new().fg(theme::RED)
                    } else {
                        Style::new()
                    };
                    Line::styled(line.text, style)
                };
                if line.truncated {
                    rendered.spans.push(Span::styled(
                        "…",
                        Style::new().fg(theme::MUTED).italic(),
                    ));
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
            return (self.got_page_lines(0, GOT_PAGE_LINES), Some(PAGED_OUTPUT_NOTE));
        }
        if self.output_too_large {
            let warning = self.got_content.as_deref().is_some_and(has_stdout).then_some(LARGE_OUTPUT_WARNING);
            return (self.got_view(), warning);
        }
        let Some(got) = self.dialog_source() else {
            return (self.got_view(), None);
        };
        if got.is_empty() {
            return (self.got_view(), None);
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
            let style = if matches!(self.verdict, CaseVerdict::CE | CaseVerdict::RE) {
                Style::new().fg(theme::RED)
            } else {
                Style::new()
            };
            let mut lines: Vec<Line<'static>> =
                stdout.lines().map(|line| Line::styled(line.to_string(), style)).collect();
            if lines.is_empty() {
                lines.push(Line::default());
            }
            lines
        };
        if let Some(stderr) = stderr {
            lines.extend(stderr.lines().map(|line| Line::from(line.to_string())));
        }
        lines
    }

    pub fn draw_tc(&mut self, buf: &mut Buffer, area: Rect, index: usize, active_box: ActiveBox, focus: CaseFocus) {
        self.area.rect = area;
        self.collapse_toggle_area.rect = Rect {
            x: area.x,
            y: area.y,
            width: area.width,
            height: 1,
        };

        let verdict_color = self.verdict.color();
        let verdict_border = if matches!(self.verdict, CaseVerdict::Pending) {
            theme::BORDER
        } else {
            verdict_color
        };
        let toggle = if self.collapsed { "▶" } else { "▼" };
        let (title_color, border_color) = match focus {
            CaseFocus::Edit => (theme::ACCENT, theme::BORDER),
            CaseFocus::Delete => (theme::RED, theme::RED),
            CaseFocus::None => (Color::Reset, theme::BORDER),
        };
        let mut tc_block = block(Some(&format!(" {toggle} TestCase {index} ")))
            .borders(Borders::TOP)
            .padding(Padding::ZERO)
            .border_type(BorderType::Rounded)
            .border_style(border_color)
            .title_style(Style::new().fg(title_color).bold());

        let chip = self.verdict.chip();
        if !chip.is_empty() {
            tc_block = tc_block.title(
                Line::from(Span::styled(
                    chip,
                    Style::new().fg(verdict_color).bold(),
                ))
                .alignment(Right),
            );
        }

        if self.collapsed {
            self.input_box.rect = Rect::default();
            self.expected_box.rect = Rect::default();
            self.got_box.rect = Rect::default();
            tc_block.render(area, buf);
            return;
        }
        let inner = tc_block.inner(area);
        let input_height = (self.input_lines() + 2) as u16;
        let output_height = (self.output_rows() + 2) as u16;
        let v = Layout::vertical([Length(input_height), Length(output_height)]).split(inner);
        let h = Layout::horizontal([Percentage(50), Percentage(50)]).split(v[1]);

        self.input_box.rect = v[0];
        self.expected_box.rect = h[0];
        self.got_box.rect = h[1];

        let is_editing = focus == CaseFocus::Edit;
        let deleting = focus == CaseFocus::Delete;
        let input_active = matches!(active_box, ActiveBox::Input) && is_editing;
        let expected_active = matches!(active_box, ActiveBox::Expected) && is_editing;

        let input_border = if deleting {
            theme::RED
        } else if input_active {
            theme::ACCENT
        } else {
            verdict_border
        };
        let expected_border = if deleting {
            theme::RED
        } else if expected_active {
            theme::ACCENT
        } else {
            verdict_border
        };
        let input_title = if deleting { theme::RED } else { theme::MUTED };
        let expected_title = if deleting { theme::RED } else { theme::MUTED };

        Self::render_pane(
            buf,
            &mut self.inp_content,
            &self.input_box,
            " [Input] ",
            input_active,
            is_editing,
            input_border,
            input_title,
            "type input here…",
        );
        Self::render_pane(
            buf,
            &mut self.exp_content,
            &self.expected_box,
            " [Expected] ",
            expected_active,
            is_editing,
            expected_border,
            expected_title,
            "no expected output",
        );

        let got_width = inner_width(self.got_box.rect);
        let got_lines: Vec<Line<'static>> =
            self.got_view().into_iter().map(|line| clip_line(line, got_width)).collect();
        let got_border = if deleting { theme::RED } else { verdict_border };
        let got_title = if deleting { theme::RED } else { theme::MUTED };
        let got_panel = Paragraph::new(Text::from(got_lines))
            .style(Style::new().fg(theme::TEXT))
            .block(
                block(Some(" [Got] "))
                    .border_style(got_border)
                    .title_style(Style::new().fg(got_title).bold())
                    .title_alignment(Left),
            );
        got_panel.render(self.got_box.rect, buf);

        tc_block.render(area, buf);
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
    fn got_view(&self) -> Vec<Line<'static>> {
        let Some(got) = &self.got_content else {
            return vec![Line::from(Span::styled(
                "run tests [r] to see the output",
                Style::new().fg(theme::MUTED).italic(),
            ))];
        };

        if got.is_empty() {
            return vec![Line::from(Span::styled(
                "(no output)",
                Style::new().fg(theme::MUTED).italic(),
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
            let style = if matches!(self.verdict, CaseVerdict::CE | CaseVerdict::RE) {
                Style::new().fg(theme::RED)
            } else {
                Style::new()
            };
            let mut lines: Vec<Line<'static>> =
                stdout.text.lines().map(|line| Line::styled(line.to_string(), style)).collect();
            if lines.is_empty() {
                lines.push(Line::default());
            }
            lines
        };

        if let Some(stderr) = &stderr {
            lines.extend(stderr.text.lines().map(|line| Line::from(line.to_string())));
        }

        let skipped_diff = matches!(self.verdict, CaseVerdict::WA { .. })
            && !diffed
            && !stdout.text.is_empty();
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

    #[allow(clippy::too_many_arguments)]
    pub fn render_pane(
        buf: &mut Buffer,
        textarea: &mut TextArea<'static>,
        state: &VisualState,
        title: &str,
        is_active: bool,
        is_editing: bool,
        border_color: Color,
        title_color: Color,
        placeholder: &str,
    ) {
        let cursor_style = if is_active {
            Style::new().add_modifier(Modifier::REVERSED)
        } else {
            Style::new()
        };

        let pane_block = block(Some(title))
            .border_style(border_color)
            .title_style(Style::new().fg(title_color).bold())
            .title_alignment(
                if title == " [Input] " { Center }
                else { Right }
            );

        // Only the box being edited is rendered as a TextArea — ratatui-textarea
        // positions the terminal cursor on render, so rendering another TextArea
        // after it would steal the cursor.
        if is_editing && is_active {
            textarea.set_style(Style::new().fg(theme::TEXT));
            textarea.set_cursor_style(cursor_style);
            textarea.set_block(pane_block);
            textarea.render(state.rect, buf);
        } else {
            let text = textarea.lines().join("\n");
            let panel = if text.trim().is_empty() {
                Paragraph::new(Line::from(Span::styled(
                    placeholder,
                    Style::new().fg(theme::MUTED).italic(),
                )))
                .block(pane_block)
            } else {
                let width = inner_width(state.rect);
                let lines: Vec<Line<'static>> =
                    pane_lines(&text).into_iter().map(|line| clip_line(line, width)).collect();
                Paragraph::new(Text::from(lines))
                    .style(Style::new().fg(theme::TEXT))
                    .block(pane_block)
            };
            panel.render(state.rect, buf);
        }
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
const LARGE_OUTPUT_WARNING: &str =
    "Output is too large to show fully. Showing a bounded preview.";
pub(crate) const PAGED_OUTPUT_NOTE: &str =
    "Large output · paged view · diff is computed per page";
/// A WA diff is only useful on a screenful. Larger previews are shown plain.
const WA_DIFF_MAX_CHARS: usize = 8_000;
const COLLAPSED_HEIGHT: usize = 2;
pub(crate) const OUTPUT_TRUNCATED_NOTE: &str = "… output truncated";
const DIFF_SKIPPED_NOTE: &str = "… diff skipped for large output";

/// Separates a case's stdout from the stderr that `helper` appends to it.
pub(crate) const STDERR_MARKER: &str = "\n── stderr ──\n";

fn extra_style() -> Style {
    Style::new().fg(theme::RED)
}

fn muted(text: String) -> Line<'static> {
    Line::from(Span::styled(text, Style::new().fg(theme::MUTED).italic()))
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
        Some(muted(format!("… {hidden} more lines hidden ({total} total)")))
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

/// Columns available inside a bordered pane: borders (2) + horizontal padding (2).
fn inner_width(rect: Rect) -> usize {
    rect.width.saturating_sub(4) as usize
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
            Some(last) if last.style == grapheme.style => last.content.to_mut().push_str(grapheme.symbol),
            _ => spans.push(Span::styled(grapheme.symbol.to_string(), grapheme.style)),
        }
    }
    spans.push(Span::styled("…", Style::new().fg(theme::MUTED)));
    Line::from(spans)
}

fn offset_rect(rect: Rect, dx: i32, dy: i32) -> Rect {
    let x = rect.x as i32 + dx;
    let y = rect.y as i32 + dy;
    if x < 0 || y < 0 {
        return Rect::default();
    }
    Rect { x: x as u16, y: y as u16, ..rect }
}

/// Appends a styled run: real newlines start new lines in the got pane.
fn push_run(lines: &mut Vec<Line<'static>>, spans: &mut Vec<Span<'static>>, text: &str, style: Style) {
    let mut parts = text.split('\n');
    if let Some(first) = parts.next()
        && !first.is_empty()
    {
        spans.push(Span::styled(first.to_string(), style));
    }
    for part in parts {
        lines.push(Line::from(std::mem::take(spans)));
        if !part.is_empty() {
            spans.push(Span::styled(part.to_string(), style));
        }
    }
}

/// The Got pane on WA: the program's output, exactly as printed. A character
/// is painted red when it sits at a position where the expected output has a
/// different character (or has nothing at all); matching characters stay
/// plain, and characters the output is missing are never invented.
pub(crate) fn highlight_got(expected: &str, got: &str) -> Vec<Line<'static>> {
    highlight_got_inner(expected, got, true)
}

/// The dialog variant: no pane cap, so a safe output keeps its complete diff.
fn highlight_got_full(expected: &str, got: &str) -> Vec<Line<'static>> {
    highlight_got_inner(expected, got, false)
}

fn highlight_got_inner(expected: &str, got: &str, capped: bool) -> Vec<Line<'static>> {
    let got = got.split(STDERR_MARKER).next().unwrap_or(got);
    let expected: Vec<char> = if capped {
        Capped::new(expected).text.chars().collect()
    } else {
        expected.chars().collect()
    };
    let got = if capped { Capped::new(got).text } else { got.to_string() };

    let mut runs: Vec<(bool, String)> = Vec::new();
    for (index, ch) in got.chars().enumerate() {
        let red = expected.get(index) != Some(&ch);
        match runs.last_mut() {
            Some((last_red, text)) if *last_red == red => text.push(ch),
            _ => runs.push((red, String::from(ch))),
        }
    }

    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut spans: Vec<Span<'static>> = Vec::new();
    for (red, text) in runs {
        push_run(&mut lines, &mut spans, &text, if red { extra_style() } else { Style::new() });
    }
    if !spans.is_empty() || lines.is_empty() {
        lines.push(Line::from(spans));
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend};

    #[test]
    fn height_tracks_content_and_collapse() {
        let mut tc = TestCase::new("1 2", "3");
        assert_eq!(tc.row_height(), 7);

        tc.inp_content.insert_str("\n4 5");
        assert_eq!(tc.row_height(), 8);

        tc.set_result(CaseVerdict::WA { ms: 1 }, "0\n1\n2".into());
        assert_eq!(tc.row_height(), 10);

        // Collapsing snaps to the padded collapsed height (title + gap row).
        tc.set_result(CaseVerdict::AC { ms: 1 }, "3".into());
        assert!(tc.collapsed);
        assert_eq!(tc.row_height(), 2);
    }

    #[test]
    fn editing_caps_a_tall_case_to_the_viewport() {
        let input = (0..100).map(|i| format!("line-{i:03}")).collect::<Vec<_>>().join("\n");
        let mut tc = TestCase::new(&input, "ok");
        let full = tc.row_height();
        assert!(full > 14);

        tc.fit_to_height(14);
        assert!(tc.row_height() <= 14, "the active case must fit the list viewport");
        tc.clear_height_caps();
        assert_eq!(tc.row_height(), full);
    }

    #[test]
    fn delete_focus_turns_every_pane_border_red() {
        let mut tc = TestCase::new("1", "2");
        let area = Rect::new(0, 0, 80, 12);
        let mut buffer = Buffer::empty(area);
        tc.draw_tc(&mut buffer, area, 1, ActiveBox::None, CaseFocus::Delete);

        assert_eq!(
            buffer[(area.width / 2, 0)].fg,
            theme::RED,
            "the case card border is red too"
        );
        for pane in [&tc.input_box, &tc.expected_box, &tc.got_box] {
            assert_eq!(
                buffer[(pane.rect.x, pane.rect.y)].fg,
                theme::RED,
                "every pane of the selected case must be red in delete mode"
            );
        }
    }

    #[test]
    fn verdict_colors_every_pane() {
        for (verdict, expected) in [
            (CaseVerdict::Pending, theme::BORDER),
            (CaseVerdict::AC { ms: 1 }, theme::GREEN),
            (CaseVerdict::WA { ms: 1 }, theme::RED),
            (CaseVerdict::Tle, theme::YELLOW),
            (CaseVerdict::RE, theme::PURPLE),
            (CaseVerdict::CE, theme::ORANGE),
        ] {
            let mut tc = TestCase::new("1", "2");
            if !matches!(verdict, CaseVerdict::Pending) {
                tc.set_result(verdict, "0".into());
                tc.collapsed = false;
            }
            let area = Rect::new(0, 0, 80, 12);
            let mut buffer = Buffer::empty(area);
            tc.draw_tc(&mut buffer, area, 1, ActiveBox::None, CaseFocus::None);

            for pane in [&tc.input_box, &tc.expected_box, &tc.got_box] {
                assert_eq!(
                    buffer[(pane.rect.x, pane.rect.y)].fg,
                    expected,
                    "{verdict:?} colors every pane"
                );
            }
        }
    }

    #[test]
    fn editing_uses_accent_focus() {
        let mut tc = TestCase::new("1", "2");
        tc.set_result(CaseVerdict::WA { ms: 1 }, "0".into());
        let area = Rect::new(0, 0, 80, 12);
        let mut buffer = Buffer::empty(area);
        tc.draw_tc(&mut buffer, area, 1, ActiveBox::Input, CaseFocus::Edit);

        assert_eq!(
            buffer[(2, 0)].fg,
            theme::ACCENT,
            "the active case title is accent blue"
        );
        assert_eq!(
            buffer[(tc.input_box.rect.x, tc.input_box.rect.y)].fg,
            theme::ACCENT,
            "the focused pane is accent blue"
        );
        assert_eq!(
            buffer[(tc.expected_box.rect.x, tc.expected_box.rect.y)].fg,
            theme::RED,
            "the other editable pane keeps its verdict color while editing"
        );
        assert_eq!(
            buffer[(tc.got_box.rect.x, tc.got_box.rect.y)].fg,
            theme::RED,
            "the Got pane keeps its verdict color"
        );
    }

    #[test]
    fn case_title_is_plain_before_a_run() {
        let mut tc = TestCase::new("1", "2");
        let area = Rect::new(0, 0, 80, 12);
        let mut buffer = Buffer::empty(area);
        tc.draw_tc(&mut buffer, area, 1, ActiveBox::None, CaseFocus::None);

        assert_eq!(
            buffer[(2, 0)].fg,
            Color::Reset,
            "the container title uses the terminal's default color"
        );
    }

    #[test]
    fn a_single_enter_grows_the_input_pane() {
        let mut tc = TestCase::new("1", "");
        let before = tc.row_height();
        tc.inp_content.insert_newline();
        assert_eq!(tc.row_height(), before + 1, "the empty line under the cursor must count");
    }

    fn red_spans(line: &Line<'static>) -> Vec<String> {
        line.spans
            .iter()
            .filter(|span| span.style.fg == Some(theme::RED))
            .map(|span| span.content.to_string())
            .collect()
    }

    fn contents<'a>(line: &'a Line<'static>) -> Vec<&'a str> {
        line.spans.iter().map(|span| span.content.as_ref()).collect()
    }

    #[test]
    fn diff_marks_got_chars_out_of_place() {
        let got = highlight_got("3 4 5", "3 9 7");
        assert_eq!(got.len(), 1);
        assert_eq!(contents(&got[0]), vec!["3 ", "9", " ", "7"]);
        assert_eq!(red_spans(&got[0]), vec!["9", "7"]);
        assert!(
            got[0].spans.iter().all(|span| span.style.bg.is_none()),
            "the diff only colors text, never background blocks"
        );
        assert!(got[0].spans.iter().all(|span| span.style.bg != Some(theme::GREEN)));
    }

    #[test]
    fn diff_marks_the_shifted_tail_after_a_missing_newline() {
        // With the line break missing, every following character sits one
        // place off — so all of it is marked.
        let got = highlight_got("hi there\nthis is kushal goyal", "hi therethis is kushal goyal");
        assert_eq!(got.len(), 1);
        assert_eq!(contents(&got[0]), vec!["hi there", "this is kushal goyal"]);
        assert_eq!(red_spans(&got[0]), vec!["this is kushal goyal"]);
    }

    #[test]
    fn diff_never_invents_missing_characters() {
        // expected "lskdjf" vs got "lkej": k/e/j sit where s/k/d should be, so
        // they are all red; the missing s/d/f never appear in the pane.
        let got = highlight_got("lskdjf", "lkej");
        assert_eq!(got.len(), 1);
        assert_eq!(contents(&got[0]), vec!["l", "kej"]);
        assert_eq!(red_spans(&got[0]), vec!["kej"]);

        // expected "lskjf" vs got "lkej": the trailing j does land correctly,
        // so only k and e are red.
        let got = highlight_got("lskjf", "lkej");
        assert_eq!(contents(&got[0]), vec!["l", "ke", "j"]);
        assert_eq!(red_spans(&got[0]), vec!["ke"]);
    }

    #[test]
    fn diff_is_positional_across_lines() {
        let got = highlight_got("a\nb\nc", "a\nx\nb\nc");
        assert_eq!(got.len(), 4);
        assert!(red_spans(&got[0]).is_empty());
        assert_eq!(red_spans(&got[1]), vec!["x"]);
        assert_eq!(red_spans(&got[2]), vec!["b"]);
        assert_eq!(red_spans(&got[3]), vec!["c"]);

        // A line the program never printed shifts the rest — marked, never invented.
        let got = highlight_got("a\nb\nc", "a\nc");
        assert_eq!(got.len(), 2);
        assert!(red_spans(&got[0]).is_empty());
        assert_eq!(red_spans(&got[1]), vec!["c"]);
    }

    #[test]
    fn diff_ignores_trailing_whitespace() {
        let got = highlight_got("3  \n4", "3\n4");
        assert!(got.iter().all(|line| red_spans(line).is_empty()));
    }

    #[test]
    fn capped_reports_hidden_lines_and_cut_characters() {
        let many = (0..500).map(|i| i.to_string()).collect::<Vec<_>>().join("\n");
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

        let many = (0..500).map(|i| i.to_string()).collect::<Vec<_>>().join("\n");
        let capped = pane_lines(&many);
        assert_eq!(capped.len(), DIFF_MAX_LINES + 1);
        assert!(capped.last().unwrap().to_string().contains("200 more lines hidden"));
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
            .map(|y| (0..area.width).map(|x| buffer[(x, y)].symbol()).collect::<String>())
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
        assert!(rows < 20, "huge output must not inflate the pane: {rows} rows");
        assert_eq!(tc.row_height(), 1 + 3 + rows + 2);
        assert!(tc.got_view().last().unwrap().to_string().contains("output truncated"));

        let mut terminal = Terminal::new(TestBackend::new(80, 40)).unwrap();
        let area = Rect::new(0, 0, 80, tc.row_height() as u16);
        terminal
            .draw(|frame| tc.draw_tc(frame.buffer_mut(), area, 1, ActiveBox::None, CaseFocus::None))
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
        assert!(!tc.wa_diff_active(), "huge WA output must bypass the diff path");

        let view = tc.got_view();
        assert!(view.len() <= DIFF_MAX_LINES + 2);
        assert!(view.iter().all(|line| red_spans(line).is_empty()));
        assert!(view.last().unwrap().to_string().contains("output truncated"));

        // Repeated redraws walk only the retained 24 KB preview, never the
        // original 2 MB string.
        for _ in 0..20 {
            assert_eq!(tc.got_view().len(), view.len());
        }
    }

    #[test]
    fn wa_previews_over_the_diff_budget_are_plain_not_diffed() {
        let mut tc = TestCase::new("x", "y");
        tc.set_result(CaseVerdict::WA { ms: 1 }, "a".repeat(WA_DIFF_MAX_CHARS + 10));

        assert!(!tc.got_truncated(), "the retention cap was not reached");
        assert!(!tc.wa_diff_active(), "large previews must not be diffed");
        let view = tc.got_view();
        assert!(view.iter().all(|line| red_spans(line).is_empty()));
        assert!(view.last().unwrap().to_string().contains("diff skipped"));
    }

    #[test]
    fn safe_large_output_is_retained_for_the_dialog_but_not_the_live_pane() {
        let mut tc = TestCase::new("x", "y");
        let line = "A".repeat(60);
        let output = (0..600).map(|_| line.as_str()).collect::<Vec<_>>().join("\n");
        assert!(output.len() > INLINE_PREVIEW_MAX_BYTES);
        assert!(output.len() <= DIALOG_SAFE_MAX_BYTES);
        tc.set_result(CaseVerdict::WA { ms: 1 }, output);

        assert!(tc.got_truncated());
        assert!(!tc.output_too_large);
        assert!(tc.got_content.as_ref().unwrap().len() <= INLINE_PREVIEW_MAX_BYTES);
        assert!(tc.got_view().last().unwrap().to_string().contains("output truncated"));

        let (lines, warning) = tc.got_dialog_view();
        assert!(warning.is_none(), "safe output must not warn");
        assert_eq!(lines.len(), 600, "the dialog keeps the full output");
        assert!(!red_spans(&lines[0]).is_empty(), "the dialog keeps the full WA diff");
    }

    #[test]
    fn output_over_the_line_budget_is_paged_instead_of_dropped() {
        let mut tc = TestCase::new("x", "y");
        let output = (0..=DIALOG_SAFE_MAX_LINES).map(|_| "A").collect::<Vec<_>>().join("\n");
        assert!(output.len() < DIALOG_SAFE_MAX_BYTES);
        tc.set_result(CaseVerdict::WA { ms: 1 }, output);

        assert!(tc.got_truncated());
        assert!(!tc.output_too_large);
        assert!(tc.got_is_paged());
        assert_eq!(tc.got_line_count(), DIALOG_SAFE_MAX_LINES + 1);

        let (lines, warning) = tc.got_dialog_view();
        assert_eq!(warning, Some(PAGED_OUTPUT_NOTE));
        assert_eq!(lines.len(), GOT_PAGE_LINES, "the dialog reads one page at a time");
        assert!(!red_spans(&lines[0]).is_empty(), "paged WA output still diffs");
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
            (0..=DIALOG_SAFE_MAX_LINES).map(|_| "e").collect::<Vec<_>>().join("\n")
        );
        tc.set_result(CaseVerdict::WA { ms: 1 }, stderr);
        assert!(tc.got_is_paged());
        assert!(
            tc.got_dialog_view().1 != Some(LARGE_OUTPUT_WARNING),
            "stderr-only output has no stdout to warn about"
        );
    }
}
