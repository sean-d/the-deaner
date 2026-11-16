//! Reports: every claude's latest answer in one popup, kept current while it's open.
//!
//! The binary collects the entries (from tmux and Claude's transcripts) and refreshes them;
//! this draws them and handles keys. No IO.

use crate::icons::Icons;
use crate::markdown::{Rendered, render};
use crate::theme::colors;
use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use the_deaner_core::AgentState;
use unicode_width::UnicodeWidthStr;

/// One agent and what it last said.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReportEntry {
    /// `<session>:<tab>`, as `the-deaner ls` shows it.
    pub address: String,
    pub session: String,
    pub window: u32,
    pub state: AgentState,
    /// What it's working on (its conversation title), if known.
    pub title: Option<String>,
    /// Its latest finished answer (markdown), if it has given one.
    pub answer: Option<String>,
    /// When that answer was written (unix seconds).
    pub at: Option<u64>,
}

/// What the caller should do after a key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReportsAction {
    None,
    Close,
    /// Go to this agent's tab: session and window index.
    Go(String, u32),
}

pub struct Reports {
    entries: Vec<ReportEntry>,
    selected: usize,
    scroll: usize,
    /// The selected answer drawn at a width: address, answer time, width, lines.
    drawn: Option<(String, Option<u64>, u16, Rendered)>,
    now: u64,
    body_height: usize,
    icons: &'static Icons,
    version: Option<String>,
}

/// `2m ago`, `3h ago`.
#[must_use]
pub fn ago(now: u64, then: u64) -> String {
    let s = now.saturating_sub(then);
    match s {
        0..60 => "just now".into(),
        60..3_600 => format!("{}m ago", s / 60),
        3_600..86_400 => format!("{}h ago", s / 3_600),
        _ => format!("{}d ago", s / 86_400),
    }
}

impl Reports {
    #[must_use]
    pub fn new(icons: &'static Icons) -> Self {
        Self {
            entries: Vec::new(),
            selected: 0,
            scroll: 0,
            drawn: None,
            now: 0,
            body_height: 20,
            icons,
            version: None,
        }
    }

    #[must_use]
    pub fn with_version(mut self, version: impl Into<String>) -> Self {
        self.version = Some(version.into());
        self
    }

    /// New data: newest answer first, agents without one last. The selection stays on the
    /// same agent; the answer it shows is redrawn if it changed.
    pub fn set_entries(&mut self, mut entries: Vec<ReportEntry>, now: u64) {
        let keep = self.selected().map(|e| e.address.clone());
        entries.sort_by(|a, b| b.at.cmp(&a.at).then_with(|| a.address.cmp(&b.address)));
        self.entries = entries;
        self.now = now;
        if let Some(addr) = keep
            && let Some(i) = self.entries.iter().position(|e| e.address == addr)
        {
            self.selected = i;
        }
        self.selected = self.selected.min(self.entries.len().saturating_sub(1));
    }

    #[must_use]
    pub fn selected(&self) -> Option<&ReportEntry> {
        self.entries.get(self.selected)
    }

    #[must_use]
    pub fn entries(&self) -> &[ReportEntry] {
        &self.entries
    }

    fn choose(&mut self, i: usize) {
        if i != self.selected {
            self.selected = i;
            self.scroll = 0;
        }
    }

    fn lines(&self) -> usize {
        self.drawn.as_ref().map_or(0, |d| d.3.lines.len())
    }

    fn scroll_by(&mut self, delta: isize) {
        let max = self.lines().saturating_sub(self.body_height);
        self.scroll = self.scroll.saturating_add_signed(delta).min(max);
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> ReportsAction {
        if key.kind == KeyEventKind::Release {
            return ReportsAction::None;
        }
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let half = isize::try_from(self.body_height / 2).unwrap_or(10).max(1);
        let last = self.entries.len().saturating_sub(1);
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => return ReportsAction::Close,
            KeyCode::Char('c') if ctrl => return ReportsAction::Close,
            KeyCode::Down | KeyCode::Char('j') => self.choose((self.selected + 1).min(last)),
            KeyCode::Up | KeyCode::Char('k') => self.choose(self.selected.saturating_sub(1)),
            KeyCode::Char('g') | KeyCode::Home => self.choose(0),
            KeyCode::Char('G') | KeyCode::End => self.choose(last),
            KeyCode::Char('d') if ctrl => self.scroll_by(half),
            KeyCode::Char('u') if ctrl => self.scroll_by(-half),
            KeyCode::Char(' ') | KeyCode::PageDown => self.scroll_by(half * 2 - 2),
            KeyCode::PageUp => self.scroll_by(-(half * 2 - 2)),
            KeyCode::Enter => {
                if let Some(e) = self.selected() {
                    return ReportsAction::Go(e.session.clone(), e.window);
                }
            }
            _ => {}
        }
        ReportsAction::None
    }

    fn state_icon(&self, state: AgentState) -> (&'static str, Style) {
        match state {
            AgentState::Awaiting => (self.icons.awaiting, Style::new().fg(colors::ATTENTION)),
            AgentState::Busy => ("⠋", Style::new().fg(colors::BUSY)),
            AgentState::Compacting => (self.icons.compacting, Style::new().fg(colors::BUSY)),
            _ => (self.icons.idle, Style::new().fg(colors::DIM)),
        }
    }

    pub fn draw(&mut self, frame: &mut Frame<'_>) {
        let full = frame.area();
        frame.buffer_mut().set_style(full, crate::theme::base());
        let muted = Style::new().fg(colors::DIM);
        let gold = Style::new().fg(colors::GOLD).add_modifier(Modifier::BOLD);
        let [title, body, hint] = Layout::vertical([
            Constraint::Length(2),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .areas(full);
        frame.render_widget(Line::styled(" Reports", gold), title);
        let mut right = String::from("live");
        if let Some(v) = &self.version {
            right = format!("live · the-deaner {v}");
        }
        frame.render_widget(
            Line::styled(format!("{right} "), muted).right_aligned(),
            title,
        );
        frame.render_widget(
            Line::styled(
                " j/k choose · Ctrl-d/u scroll · Enter go to it · Esc close",
                muted,
            ),
            hint,
        );
        self.body_height = usize::from(body.height);

        if self.entries.is_empty() {
            frame.render_widget(
                Paragraph::new(vec![
                    Line::raw(""),
                    Line::styled(
                        " No claudes running. Their answers show up here as they finish.",
                        muted,
                    ),
                ]),
                body,
            );
            return;
        }

        let list_w = self
            .entries
            .iter()
            .map(|e| e.address.width() + 14)
            .max()
            .unwrap_or(30)
            .clamp(24, usize::from(body.width) * 2 / 5);
        let [list, rule, answer] = Layout::horizontal([
            Constraint::Length(u16::try_from(list_w).unwrap_or(30)),
            Constraint::Length(1),
            Constraint::Min(10),
        ])
        .areas(body);

        let rows: Vec<Line<'_>> = self
            .entries
            .iter()
            .enumerate()
            .map(|(i, e)| {
                let (icon, icon_style) = self.state_icon(e.state);
                let when = match (e.state, e.at) {
                    (AgentState::Busy, _) => "working".to_string(),
                    (_, Some(at)) => ago(self.now, at),
                    (_, None) => "no answer yet".to_string(),
                };
                let used = 3 + e.address.width() + when.width() + 1;
                let pad = list_w.saturating_sub(used);
                let base = if i == self.selected {
                    Style::new().bg(colors::SEL)
                } else {
                    Style::new()
                };
                Line::from(vec![
                    Span::styled(format!(" {icon} "), icon_style.patch(base)),
                    Span::styled(
                        e.address.clone(),
                        base.fg(colors::FG).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(" ".repeat(pad), base),
                    Span::styled(format!("{when} "), base.fg(colors::DIM)),
                ])
            })
            .collect();
        frame.render_widget(Paragraph::new(rows), list);
        let bar: Vec<Line<'_>> = (0..body.height)
            .map(|_| Line::styled("│", Style::new().fg(colors::LINE)))
            .collect();
        frame.render_widget(Paragraph::new(bar), rule);

        let Some(e) = self.entries.get(self.selected).cloned() else {
            return;
        };
        let width = answer.width.saturating_sub(2);
        let fresh = matches!(&self.drawn, Some((a, at, w, _)) if *a == e.address && *at == e.at && *w == width);
        if !fresh {
            let mut md = String::new();
            if let Some(t) = &e.title {
                md.push_str(&format!("## {t}\n\n"));
            }
            match (&e.answer, e.state) {
                (Some(a), AgentState::Busy) => {
                    md.push_str("*Working on something new; this is its previous answer.*\n\n");
                    md.push_str(a);
                }
                (Some(a), _) => md.push_str(a),
                (None, _) => md.push_str("*No finished answer yet.*"),
            }
            self.drawn = Some((e.address.clone(), e.at, width, render(&md, width)));
            self.scroll = 0;
        }
        self.scroll = self
            .scroll
            .min(self.lines().saturating_sub(self.body_height));
        if let Some((_, _, _, r)) = &self.drawn {
            let lines: Vec<Line<'_>> = r
                .lines
                .iter()
                .skip(self.scroll)
                .take(usize::from(answer.height))
                .map(|l| {
                    let mut l = l.clone();
                    l.spans.insert(0, Span::raw(" "));
                    l
                })
                .collect();
            frame.render_widget(Paragraph::new(lines), answer);
        }
    }
}
