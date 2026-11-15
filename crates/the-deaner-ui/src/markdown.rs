//! Markdown to styled terminal lines, for the built-in guide.
//!
//! Handles what the docs use: headings, paragraphs, emphasis, `code`, code blocks, lists,
//! tables (wrapped to fit), block quotes, rules, links, images (as `[image: …]`), a little
//! HTML, and mermaid diagrams, drawn as text with mmdflux. No IO.

use crate::theme::colors;
use pulldown_cmark::{Alignment, CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthStr;

/// A page drawn for one width.
#[derive(Debug, Clone, Default)]
pub struct Rendered {
    pub lines: Vec<Line<'static>>,
}

impl Rendered {
    /// Each line as plain text (for search).
    #[must_use]
    pub fn plain(&self) -> Vec<String> {
        self.lines
            .iter()
            .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect())
            .collect()
    }
}

/// Draw `md` for a screen `width` columns wide.
#[must_use]
pub fn render(md: &str, width: u16) -> Rendered {
    let width = usize::from(width.max(20));
    let mut r = Renderer::new(width);
    let opts = Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH;
    for event in Parser::new_ext(md, opts) {
        r.event(event);
    }
    r.flush_text();
    while r.out.last().is_some_and(|l| l.width() == 0) {
        r.out.pop();
    }
    Rendered { lines: r.out }
}

const TEXT: Style = Style::new().fg(colors::FG);
const DIM: Style = Style::new().fg(colors::DIM);
const CODE: Style = Style::new().fg(colors::CYAN);
const LINK: Style = Style::new().fg(colors::BLUE);
const RULE: Style = Style::new().fg(colors::LINE);

#[derive(Clone, Copy)]
enum ListKind {
    Bullet,
    Number(u64),
}

#[derive(Default)]
struct Table {
    aligns: Vec<Alignment>,
    rows: Vec<Vec<Vec<(String, Style)>>>,
    head_rows: usize,
    in_head: bool,
}

struct Renderer {
    width: usize,
    out: Vec<Line<'static>>,
    /// Inline text waiting to be wrapped: (text, style).
    text: Vec<(String, Style)>,
    styles: Vec<Style>,
    heading: Option<HeadingLevel>,
    lists: Vec<ListKind>,
    /// The marker for the next line of the current list item (`• `, `3. `).
    marker: Option<String>,
    quote: usize,
    code: Option<(String, String)>,
    table: Option<Table>,
    image: bool,
}

impl Renderer {
    fn new(width: usize) -> Self {
        Self {
            width,
            out: Vec::new(),
            text: Vec::new(),
            styles: vec![TEXT],
            heading: None,
            lists: Vec::new(),
            marker: None,
            quote: 0,
            code: None,
            table: None,
            image: false,
        }
    }

    fn style(&self) -> Style {
        *self.styles.last().unwrap_or(&TEXT)
    }

    fn push_style(&mut self, add: Style) {
        let s = self.style().patch(add);
        self.styles.push(s);
    }

    fn pop_style(&mut self) {
        if self.styles.len() > 1 {
            self.styles.pop();
        }
    }

    fn blank(&mut self) {
        if self.out.last().is_some_and(|l| l.width() > 0) {
            self.out.push(Line::raw(""));
        }
    }

    /// What goes in front of every line here: block quote bars and list indentation.
    fn indent(&self) -> String {
        let mut s = "│ ".repeat(self.quote);
        s.push_str(&"   ".repeat(self.lists.len().saturating_sub(1)));
        s
    }

    fn inline(&mut self, text: &str, style: Style) {
        if let Some(table) = &mut self.table {
            if let Some(cell) = table.rows.last_mut().and_then(|r| r.last_mut()) {
                cell.push((text.to_string(), style));
            }
            return;
        }
        self.text.push((text.to_string(), style));
    }

    /// Wrap the waiting inline text into lines.
    fn flush_text(&mut self) {
        if self.text.is_empty() {
            return;
        }
        let text = std::mem::take(&mut self.text);
        let base = self.indent();
        let (first, rest) = match self.marker.take() {
            Some(m) => {
                let pad = " ".repeat(m.width());
                (format!("{base}{m}"), format!("{base}{pad}"))
            }
            None => (base.clone(), base),
        };
        let wrapped = wrap(&text, self.width, &first, &rest);
        self.out.extend(wrapped);
    }

    fn event(&mut self, event: Event<'_>) {
        if let Some((_, buf)) = &mut self.code {
            match event {
                Event::Text(t) => buf.push_str(&t),
                Event::End(TagEnd::CodeBlock) => self.end_code(),
                _ => {}
            }
            return;
        }
        match event {
            Event::Start(tag) => self.start(tag),
            Event::End(tag) => self.end(tag),
            Event::Text(t) => {
                if self.image {
                    self.inline(&t, DIM);
                } else {
                    let s = self.style();
                    self.inline(&t, s);
                }
            }
            Event::Code(t) => self.inline(&t, CODE),
            Event::SoftBreak => self.inline(" ", TEXT),
            Event::HardBreak => {
                self.flush_text();
            }
            Event::Rule => {
                self.flush_text();
                self.blank();
                let w = self.width.saturating_sub(2);
                self.out
                    .push(Line::styled(format!(" {}", "─".repeat(w)), RULE));
                self.out.push(Line::raw(""));
            }
            Event::Html(h) | Event::InlineHtml(h) => self.html(&h),
            Event::TaskListMarker(done) => self.inline(if done { "[x] " } else { "[ ] " }, DIM),
            _ => {}
        }
    }

    fn start(&mut self, tag: Tag<'_>) {
        match tag {
            Tag::Paragraph => {}
            Tag::Heading { level, .. } => {
                self.flush_text();
                self.blank();
                self.heading = Some(level);
                let style = match level {
                    HeadingLevel::H1 | HeadingLevel::H2 => {
                        Style::new().fg(colors::GOLD).add_modifier(Modifier::BOLD)
                    }
                    _ => Style::new().fg(colors::FG).add_modifier(Modifier::BOLD),
                };
                self.styles.push(style);
            }
            Tag::BlockQuote(_) => {
                self.flush_text();
                self.quote += 1;
                self.push_style(DIM);
            }
            Tag::CodeBlock(kind) => {
                self.flush_text();
                let lang = match kind {
                    CodeBlockKind::Fenced(l) => l.to_string(),
                    CodeBlockKind::Indented => String::new(),
                };
                self.code = Some((lang, String::new()));
            }
            Tag::List(start) => {
                self.flush_text();
                if self.lists.is_empty() {
                    self.blank();
                }
                self.lists.push(match start {
                    Some(n) => ListKind::Number(n),
                    None => ListKind::Bullet,
                });
            }
            Tag::Item => {
                self.flush_text();
                let marker = match self.lists.last_mut() {
                    Some(ListKind::Number(n)) => {
                        let m = format!("{n}. ");
                        *n += 1;
                        m
                    }
                    _ => "• ".to_string(),
                };
                self.marker = Some(marker);
            }
            Tag::Emphasis => self.push_style(Style::new().add_modifier(Modifier::ITALIC)),
            Tag::Strong => self.push_style(Style::new().add_modifier(Modifier::BOLD)),
            Tag::Strikethrough => self.push_style(Style::new().add_modifier(Modifier::CROSSED_OUT)),
            Tag::Link { .. } => self.push_style(LINK.add_modifier(Modifier::UNDERLINED)),
            Tag::Image { .. } => {
                self.image = true;
                self.inline("[image: ", DIM);
            }
            Tag::Table(aligns) => {
                self.flush_text();
                self.blank();
                self.table = Some(Table {
                    aligns,
                    ..Table::default()
                });
            }
            Tag::TableHead => {
                if let Some(t) = &mut self.table {
                    t.in_head = true;
                    t.rows.push(Vec::new());
                }
            }
            Tag::TableRow => {
                if let Some(t) = &mut self.table {
                    t.rows.push(Vec::new());
                }
            }
            Tag::TableCell => {
                if let Some(t) = &mut self.table
                    && let Some(row) = t.rows.last_mut()
                {
                    row.push(Vec::new());
                }
            }
            _ => {}
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph => {
                self.flush_text();
                if self.lists.is_empty() {
                    self.out.push(Line::raw(""));
                }
            }
            TagEnd::Heading(level) => {
                self.flush_text();
                self.pop_style();
                if level == HeadingLevel::H1 {
                    let w = self.out.last().map_or(0, |l| l.width()).min(self.width);
                    self.out
                        .push(Line::styled("═".repeat(w), Style::new().fg(colors::GOLD)));
                }
                self.heading = None;
                self.out.push(Line::raw(""));
            }
            TagEnd::BlockQuote(_) => {
                self.flush_text();
                self.quote = self.quote.saturating_sub(1);
                self.pop_style();
                self.blank();
            }
            TagEnd::List(_) => {
                self.flush_text();
                self.lists.pop();
                if self.lists.is_empty() {
                    self.out.push(Line::raw(""));
                }
            }
            TagEnd::Item => self.flush_text(),
            TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough | TagEnd::Link => {
                self.pop_style();
            }
            TagEnd::Image => {
                self.inline("]", DIM);
                self.image = false;
            }
            TagEnd::TableHead => {
                if let Some(t) = &mut self.table {
                    t.in_head = false;
                    t.head_rows = t.rows.len();
                }
            }
            TagEnd::Table => {
                if let Some(t) = self.table.take() {
                    let lines = draw_table(&t, self.width);
                    self.out.extend(lines);
                    self.out.push(Line::raw(""));
                }
            }
            _ => {}
        }
    }

    fn end_code(&mut self) {
        let Some((lang, body)) = self.code.take() else {
            return;
        };
        self.blank();
        let indent = self.indent();
        if lang == "mermaid" {
            let room = self.width.saturating_sub(indent.width() + 2);
            for line in diagram(&body, room) {
                let mut spans = vec![Span::raw(format!("{indent}  "))];
                spans.extend(line.spans);
                self.out.push(Line::from(spans));
            }
        } else {
            for line in body.trim_end_matches('\n').lines() {
                self.out
                    .push(Line::styled(format!("{indent}  {line}"), CODE));
            }
        }
        self.out.push(Line::raw(""));
    }

    /// The docs use HTML for a centered picture and some bold lines in the README: show the
    /// words, turn `<img alt>` into `[image: …]`, and skip comments.
    fn html(&mut self, html: &str) {
        let h = html.trim();
        if h.starts_with("<!--") {
            return;
        }
        if h.eq_ignore_ascii_case("<br>") || h.eq_ignore_ascii_case("<br/>") {
            self.flush_text();
            return;
        }
        for raw in html.lines() {
            if let Some(alt) = attr(raw, "alt") {
                self.flush_text();
                self.inline(&format!("[image: {alt}]"), DIM);
                self.flush_text();
                continue;
            }
            let text = strip_tags(raw);
            let text = text.trim();
            if !text.is_empty() {
                let s = self.style().add_modifier(Modifier::BOLD);
                self.inline(text, s);
                self.flush_text();
            }
        }
        if html.ends_with('\n') {
            self.blank();
        }
    }
}

fn attr(tag: &str, name: &str) -> Option<String> {
    let at = tag.find(&format!("{name}=\""))? + name.len() + 2;
    let end = tag[at..].find('"')? + at;
    Some(tag[at..end].to_string())
}

fn strip_tags(s: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            c if !in_tag => out.push(c),
            _ => {}
        }
    }
    out
}

/// Word-wrap styled text to `width`, with `first` in front of the first line and `rest`
/// in front of the others. A word longer than a line is split.
fn wrap(text: &[(String, Style)], width: usize, first: &str, rest: &str) -> Vec<Line<'static>> {
    // Words, each a run of styled pieces, separated by spaces.
    let mut words: Vec<Vec<(String, Style)>> = vec![Vec::new()];
    for (t, style) in text {
        for (i, part) in t.split(' ').enumerate() {
            if i > 0 {
                words.push(Vec::new());
            }
            if !part.is_empty()
                && let Some(w) = words.last_mut()
            {
                w.push((part.to_string(), *style));
            }
        }
    }
    words.retain(|w| !w.is_empty());

    let mut lines = Vec::new();
    let mut spans: Vec<Span<'static>> = vec![Span::raw(first.to_string())];
    let mut used = first.width();
    let mut start = true;
    for word in words {
        let ww: usize = word.iter().map(|(t, _)| t.width()).sum();
        if !start && used + 1 + ww > width {
            lines.push(Line::from(std::mem::take(&mut spans)));
            spans.push(Span::raw(rest.to_string()));
            used = rest.width();
            start = true;
        }
        if !start {
            spans.push(Span::raw(" "));
            used += 1;
        }
        for (t, style) in word {
            // A single word wider than the line: split it.
            let mut piece = String::new();
            for c in t.chars() {
                let cw = c.to_string().width();
                if used + piece.width() + cw > width && !(start && piece.is_empty()) {
                    spans.push(Span::styled(std::mem::take(&mut piece), style));
                    lines.push(Line::from(std::mem::take(&mut spans)));
                    spans.push(Span::raw(rest.to_string()));
                    used = rest.width();
                }
                piece.push(c);
            }
            used += piece.width();
            spans.push(Span::styled(piece, style));
        }
        start = false;
    }
    if spans.iter().any(|s| !s.content.trim().is_empty()) {
        lines.push(Line::from(spans));
    }
    lines
}

fn plain_width(cell: &[(String, Style)]) -> usize {
    cell.iter().map(|(t, _)| t.width()).sum()
}

/// A table with columns shrunk to fit and cell text wrapped inside them.
fn draw_table(t: &Table, width: usize) -> Vec<Line<'static>> {
    let cols = t.rows.iter().map(Vec::len).max().unwrap_or(0);
    if cols == 0 {
        return Vec::new();
    }
    let mut natural = vec![1usize; cols];
    for row in &t.rows {
        for (i, cell) in row.iter().enumerate() {
            natural[i] = natural[i].max(plain_width(cell));
        }
    }
    // Room for text: the width minus " │ " between columns and a space at each end.
    let room = width.saturating_sub(3 * (cols - 1) + 2).max(cols * 4);
    let mut widths = natural.clone();
    while widths.iter().sum::<usize>() > room {
        // Shrink the widest column, never below 8 (or its natural width).
        let Some((i, _)) = widths
            .iter()
            .enumerate()
            .filter(|&(i, w)| *w > 8.min(natural[i]))
            .max_by_key(|&(_, w)| *w)
        else {
            break;
        };
        widths[i] -= 1;
    }

    let sep = |left: &str, mid: &str| {
        let parts: Vec<String> = widths.iter().map(|w| "─".repeat(*w)).collect();
        Line::styled(format!("{left}{}─", parts.join(mid)), RULE)
    };
    let mut out = Vec::new();
    for (ri, row) in t.rows.iter().enumerate() {
        let head = ri < t.head_rows;
        let cells: Vec<Vec<Line<'static>>> = (0..cols)
            .map(|i| {
                let cell: Vec<(String, Style)> = row
                    .get(i)
                    .cloned()
                    .unwrap_or_default()
                    .into_iter()
                    .map(|(s, st)| {
                        (
                            s,
                            if head {
                                st.add_modifier(Modifier::BOLD)
                            } else {
                                st
                            },
                        )
                    })
                    .collect();
                wrap(&cell, widths[i], "", "")
            })
            .collect();
        let height = cells.iter().map(Vec::len).max().unwrap_or(1).max(1);
        for li in 0..height {
            let mut spans = vec![Span::raw(" ")];
            for (i, cell) in cells.iter().enumerate() {
                if i > 0 {
                    spans.push(Span::styled(" │ ", RULE));
                }
                let line = cell.get(li).cloned().unwrap_or_default();
                let pad = widths[i].saturating_sub(line.width());
                let (before, after) = match t.aligns.get(i) {
                    Some(Alignment::Right) => (pad, 0),
                    Some(Alignment::Center) => (pad / 2, pad - pad / 2),
                    _ => (0, pad),
                };
                spans.push(Span::raw(" ".repeat(before)));
                spans.extend(line.spans);
                spans.push(Span::raw(" ".repeat(after)));
            }
            out.push(Line::from(spans));
        }
        if ri + 1 < t.rows.len() {
            out.push(sep(" ", "─┼─"));
        }
    }
    out
}

/// A mermaid diagram drawn as text: as written; top-to-bottom if left-to-right doesn't
/// fit; otherwise its source, so nothing is ever missing.
fn diagram(src: &str, width: usize) -> Vec<Line<'static>> {
    let draw = |s: &str| {
        mmdflux::render_diagram(
            s,
            mmdflux::OutputFormat::Text,
            &mmdflux::RenderConfig::default(),
        )
        .ok()
    };
    let fits = |t: &String| t.lines().map(UnicodeWidthStr::width).max().unwrap_or(0) <= width;
    let sideways = src.contains("flowchart LR") || src.contains("graph LR");
    let drawn = draw(src).filter(fits).or_else(|| {
        sideways
            .then(|| draw(&src.replacen(" LR", " TB", 1)))
            .flatten()
            .filter(fits)
    });
    match drawn {
        Some(text) => text
            .trim_end_matches('\n')
            .lines()
            .map(|l| Line::styled(l.trim_end().to_string(), TEXT))
            .collect(),
        None => {
            let mut lines = vec![Line::styled(
                "(a diagram too wide to draw here; its mermaid source:)",
                DIM,
            )];
            lines.extend(src.lines().map(|l| Line::styled(l.to_string(), CODE)));
            lines
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(r: &Rendered) -> String {
        r.plain().join("\n")
    }

    #[test]
    fn headings_paragraphs_and_inline_styles() {
        let r = render("# Title\n\nSome **bold** and `code` here.\n\n## Next\n", 60);
        let t = text(&r);
        assert!(
            t.starts_with("Title\n═════\n\nSome bold and code here.\n\nNext"),
            "{t}"
        );
        let code = r.lines[3]
            .spans
            .iter()
            .find(|s| s.content == "code")
            .unwrap();
        assert_eq!(code.style.fg, Some(colors::CYAN));
    }

    #[test]
    fn paragraphs_wrap_to_the_width() {
        let r = render(&"word ".repeat(40), 30);
        assert!(r.plain().iter().all(|l| l.width() <= 30));
        assert!(r.lines.len() > 5);
    }

    #[test]
    fn lists_get_markers_and_hang_their_wrapped_lines() {
        let r = render(
            "- one two three four five six seven\n- b\n\n1. x\n2. y\n",
            20,
        );
        let t = text(&r);
        assert!(
            t.contains("• one two three four\n  five six seven\n• b"),
            "{t}"
        );
        assert!(t.contains("1. x\n2. y"), "{t}");
    }

    #[test]
    fn tables_fit_and_wrap_their_cells() {
        let md = "| Key | What it does |\n|---|---|\n| `x` | Remove a worktree or a project, stopping its tmux sessions |\n";
        let r = render(md, 40);
        assert!(r.plain().iter().all(|l| l.width() <= 40), "{}", text(&r));
        let t = text(&r);
        assert!(t.contains("Key │ What it does"), "{t}");
        assert!(t.contains("─┼─"), "{t}");
        assert!(t.contains("stopping its tmux"), "{t}");
    }

    #[test]
    fn code_blocks_stay_as_written() {
        let r = render("```sh\nthe-deaner upgrade --check\n```\n", 20);
        assert!(text(&r).contains("  the-deaner upgrade --check"));
    }

    #[test]
    fn mermaid_is_drawn_as_a_diagram() {
        let r = render(
            "```mermaid\nflowchart LR\n  a[Alpha] --> b[Beta]\n```\n",
            80,
        );
        let t = text(&r);
        assert!(
            t.contains("Alpha") && t.contains("Beta") && t.contains('┌'),
            "{t}"
        );
        assert!(!t.contains("flowchart"), "drawn, not shown as source: {t}");
    }

    #[test]
    fn a_wide_sideways_diagram_is_redrawn_top_to_bottom() {
        let src = "```mermaid\nflowchart LR\n  a[Alpha alpha alpha] --> b[Beta beta beta] --> c[Gamma gamma gamma] --> d[Delta delta delta]\n```\n";
        let r = render(src, 40);
        let t = text(&r);
        assert!(
            t.contains("Delta delta delta") && !t.contains("flowchart"),
            "{t}"
        );
        assert!(r.plain().iter().all(|l| l.width() <= 40), "{t}");
    }

    #[test]
    fn readme_html_becomes_words_and_an_image_note() {
        let md = "<p align=\"center\">\n  <img src=\"x.jpg\" alt=\"Deaner\" width=\"260\">\n</p>\n\n<!-- hidden -->\n\n![a screenshot](docs/x.png)\n";
        let t = text(&render(md, 60));
        assert!(t.contains("[image: Deaner]"), "{t}");
        assert!(t.contains("[image: a screenshot]"), "{t}");
        assert!(!t.contains("hidden") && !t.contains('<'), "{t}");
    }
}
