use ratatui::{
    Frame,
    crossterm::event::{MouseButton, MouseEvent, MouseEventKind},
    layout::{Constraint::{Length, Percentage}, HorizontalAlignment::Right, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{BorderType, Borders, Padding, Paragraph},
};
use ratatui_textarea::TextArea;

use crate::components::{VisualState, block, theme};

#[derive(Debug, Clone, Copy)]
pub enum ActiveBox {
    Input,
    Expected,
    None,
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

pub struct TestCase {
    pub inp_content: TextArea<'static>,
    pub exp_content: TextArea<'static>,
    pub got_content: Option<String>,
    pub input_box: VisualState,
    pub expected_box: VisualState,
    pub got_box: VisualState,
    pub collapse_toggle_area: VisualState,
    pub area: VisualState,
    pub verdict: CaseVerdict,
    pub collapsed: bool,
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
            got_box: VisualState::new(),
            expected_box: VisualState::new(),
            input_box: VisualState::new(),
            collapse_toggle_area: VisualState::new(),
            area: VisualState::new(),
            verdict: CaseVerdict::Pending,
            collapsed: false,
        }
    }

    fn input_lines(&self) -> usize {
        self.inp_content.lines().len().max(1)
    }

    fn expected_lines(&self) -> usize {
        self.exp_content.lines().len().max(1)
    }

    fn got_lines(&self) -> usize {
        match &self.got_content {
            Some(text) if !text.is_empty() => text.lines().count().max(1),
            _ => 1,
        }
    }

    fn full_height(&self) -> usize {
        1 + (self.input_lines() + 2) + (self.output_rows() + 2)
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
        self.verdict = CaseVerdict::Pending;
    }

    pub fn set_result(&mut self, verdict: CaseVerdict, got: String) {
        self.got_content = Some(got);
        self.verdict = verdict;
        self.collapsed = verdict.is_ac();
    }

    pub fn draw_tc(&mut self, frame: &mut Frame, area: Rect, index: usize, active_box: ActiveBox, is_editing: bool) {
        self.area.rect = area;
        self.collapse_toggle_area.rect = Rect {
            x: area.x,
            y: area.y,
            width: area.width,
            height: 1,
        };

        let verdict_color = self.verdict.color();
        let toggle = if self.collapsed { "▶" } else { "▼" };
        let title_color = if is_editing { theme::ACCENT } else { Color::Reset };
        let mut tc_block = block(Some(&format!(" {toggle} TestCase {index} ")))
            .borders(Borders::TOP)
            .padding(Padding::ZERO)
            .border_type(BorderType::Rounded)
            .border_style(theme::BORDER)
            .title_style(Style::new().fg(title_color).bold());

        let chip = self.verdict.chip();
        if !chip.is_empty() {
            tc_block = tc_block.title(
                Line::from(Span::styled(chip, Style::new().fg(verdict_color).bold())).alignment(Right),
            );
        }

        if self.collapsed {
            self.input_box.rect = Rect::default();
            self.expected_box.rect = Rect::default();
            self.got_box.rect = Rect::default();
            frame.render_widget(tc_block, area);
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

        let input_active = matches!(active_box, ActiveBox::Input) && is_editing;
        let expected_active = matches!(active_box, ActiveBox::Expected) && is_editing;

        Self::render_pane(
            frame,
            &mut self.inp_content,
            &self.input_box,
            " Input ",
            input_active,
            is_editing,
            "type input here…",
        );
        Self::render_pane(
            frame,
            &mut self.exp_content,
            &self.expected_box,
            " Expected ",
            expected_active,
            is_editing,
            "no expected output",
        );

        let got_panel = Paragraph::new(self.got_text())
            .style(Style::new().fg(theme::TEXT))
            .block(
            block(Some(" Got "))
                .border_style(if matches!(self.verdict, CaseVerdict::Pending) {
                    theme::BORDER
                } else {
                    verdict_color
                })
                .title_alignment(Right),
        );
        frame.render_widget(got_panel, self.got_box.rect);

        frame.render_widget(tc_block, area);
    }

    fn wa_diff_active(&self) -> bool {
        matches!(self.verdict, CaseVerdict::WA { .. })
            && self.got_content.as_deref().is_some_and(|got| !got.is_empty())
    }

    fn output_rows(&self) -> usize {
        self.expected_lines().max(self.got_lines())
    }

    fn got_text(&self) -> Text<'static> {
        let Some(got) = &self.got_content else {
            return Text::from(Line::from(Span::styled(
                "run tests [r] to see the output",
                Style::new().fg(theme::MUTED).italic(),
            )));
        };

        if got.is_empty() {
            return Text::from(Line::from(Span::styled(
                "(no output)",
                Style::new().fg(theme::MUTED).italic(),
            )));
        }

        if self.wa_diff_active() {
            let expected = self.exp_content.lines().join("\n");
            let mut got_lines = highlight_got(&expected, got);
            if let Some((_, stderr)) = got.split_once("\n── stderr ──\n") {
                got_lines.extend(stderr.lines().map(|line| Line::from(line.to_string())));
            }
            return Text::from(got_lines);
        }

        let style = if matches!(self.verdict, CaseVerdict::CE | CaseVerdict::RE) {
            Style::new().fg(theme::RED)
        } else {
            Style::new()
        };
        Text::from(got.lines().map(|line| Line::styled(line.to_string(), style)).collect::<Vec<_>>())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn render_pane(
        frame: &mut Frame,
        textarea: &mut TextArea<'static>,
        state: &VisualState,
        title: &str,
        is_active: bool,
        is_editing: bool,
        placeholder: &str,
    ) {
        let (border_color, cursor_style) = if is_active {
            (theme::ACCENT, Style::new().add_modifier(Modifier::REVERSED))
        } else {
            (theme::BORDER, Style::new())
        };

        let pane_block = block(Some(title)).border_style(border_color).title_alignment(Right);

        // Only the box being edited is rendered as a TextArea — ratatui-textarea
        // positions the terminal cursor on render, so rendering another TextArea
        // after it would steal the cursor.
        if is_editing && is_active {
            textarea.set_cursor_style(cursor_style);
            textarea.set_block(pane_block);
            frame.render_widget(textarea as &TextArea, state.rect);
        } else {
            let text = textarea.lines().join("\n");
            let panel = if text.trim().is_empty() {
                Paragraph::new(Line::from(Span::styled(
                    placeholder,
                    Style::new().fg(theme::MUTED).italic(),
                )))
                .block(pane_block)
            } else {
                Paragraph::new(text).style(Style::new().fg(theme::TEXT)).block(pane_block)
            };
            frame.render_widget(panel, state.rect);
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
}

const DIFF_MAX_LINES: usize = 300;
const DIFF_MAX_CHARS: usize = 24_000;
const COLLAPSED_HEIGHT: usize = 2;

fn extra_style() -> Style {
    Style::new().fg(theme::RED)
}

/// Normalized diff input: at most `DIFF_MAX_LINES` lines with trailing
/// whitespace stripped, capped to `DIFF_MAX_CHARS` characters.
fn normalize(text: &str) -> String {
    let mut out = String::new();
    for (i, line) in text.lines().take(DIFF_MAX_LINES).enumerate() {
        if i > 0 {
            out.push('\n');
        }
        out.push_str(line.trim_end());
    }
    out.chars().take(DIFF_MAX_CHARS).collect()
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
    let got = got.split("\n── stderr ──\n").next().unwrap_or(got);
    let expected: Vec<char> = normalize(expected).chars().collect();
    let got = normalize(got);

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
}
