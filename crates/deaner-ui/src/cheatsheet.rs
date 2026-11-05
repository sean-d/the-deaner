//! The searchable shortcut popup: type what you want to do, see the keys.
//!
//! Entries come from the keymap table, the configured frame keys, and the user's
//! real tmux bindings, so what it shows is always what the keys actually do.

use crate::icons::Icons;
use crate::keymap::{BINDINGS, frame_bindings, keys_label, search_terms};
use deaner_core::config::FrameKeys;
use deaner_core::tmux_keys::TmuxShortcut;
use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratcn::{InputState, InputWidget, Theme};
use unicode_width::UnicodeWidthStr;

const ANYWHERE: &str = "Anywhere";
const SIDEBAR: &str = "Sidebar";
const TERMINAL: &str = "Terminal";
const ICONS: &str = "Sidebar icons";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub section: &'static str,
    pub keys: String,
    pub what: String,
    terms: String,
}

impl Entry {
    fn new(
        section: &'static str,
        keys: impl Into<String>,
        what: impl Into<String>,
        terms: &str,
    ) -> Self {
        Self {
            section,
            keys: keys.into(),
            what: what.into(),
            terms: terms.to_string(),
        }
    }

    /// How many of `words` appear in this entry.
    fn score(&self, words: &[String]) -> usize {
        let haystack = format!(
            "{} {} {} {}",
            self.section, self.what, self.terms, self.keys
        )
        .to_lowercase();
        words
            .iter()
            .filter(|w| haystack.contains(w.as_str()))
            .count()
    }
}

/// Every shortcut, in reading order. `tmux` is empty only if tmux couldn't be asked.
#[must_use]
pub fn entries(frame: &FrameKeys, tmux: &[TmuxShortcut], icons: &Icons) -> Vec<Entry> {
    let mut out: Vec<Entry> = frame_bindings(frame)
        .into_iter()
        .map(|b| Entry::new(ANYWHERE, b.key, b.help, b.terms))
        .collect();
    out.extend(
        BINDINGS
            .iter()
            .map(|b| Entry::new(SIDEBAR, keys_label(b), b.help, search_terms(b.action))),
    );
    if tmux.is_empty() {
        out.push(Entry::new(
            TERMINAL,
            "",
            "Couldn't read your tmux keys",
            "tab window pane split",
        ));
    }
    out.extend(
        tmux.iter()
            .map(|t| Entry::new(TERMINAL, t.keys.clone(), t.what, t.terms)),
    );
    out.extend(
        [
            (icons.branch, "Branch (dim when not running)"),
            (icons.detached, "Detached HEAD"),
            (icons.folder, "Plain folder"),
            (
                icons.pull_request,
                "Pull request: green open, dim draft, red closed",
            ),
            (icons.merged, "Branch's last pull request, merged"),
            (icons.changed, "Changed files"),
            ("+1 -2", "Commits ahead of / behind the upstream"),
            ("#259", "Pull request number"),
            (icons.checks_passing, "CI passing"),
            (icons.checks_failing, "CI failing"),
            (icons.checks_pending, "CI running"),
            (
                "Orange",
                "An agent needs you, or finished while you were elsewhere",
            ),
            ("Highlight", "The project shown on the right"),
            ("⠋", "Agent working"),
            (icons.awaiting, "Agent waiting on you"),
            (icons.idle, "Agent idle"),
            (icons.compacting, "Agent compacting"),
            (icons.parked, "Parked"),
        ]
        .map(|(icon, what)| Entry::new(ICONS, icon, what, "legend symbol badge status")),
    );
    out
}

/// Words that don't help find anything, so "how do i hide the sidebar" works.
const FILLER: &[&str] = &[
    "a", "an", "and", "can", "do", "for", "how", "i", "in", "is", "my", "of", "on", "or", "the",
    "to", "what", "with",
];

fn search_words(query: &str) -> Vec<String> {
    query
        .split_whitespace()
        .map(str::to_lowercase)
        .filter(|w| !FILLER.contains(&w.as_str()))
        // "tabs" should find "tab", and "panes" should find "pane".
        .map(|w| match w.strip_suffix('s') {
            Some(stem) if stem.len() >= 3 => stem.to_string(),
            _ => w,
        })
        .collect()
}

pub struct Cheatsheet {
    entries: Vec<Entry>,
    query: InputState,
    scroll: u16,
    theme: Theme,
}

impl Cheatsheet {
    #[must_use]
    pub fn new(entries: Vec<Entry>) -> Self {
        Self {
            entries,
            query: InputState::default(),
            scroll: 0,
            theme: crate::theme::terminal(),
        }
    }

    /// Entries matching what has been typed: those with the most words in common.
    /// Questions are worded loosely, so not every word has to match.
    #[must_use]
    pub fn visible(&self) -> Vec<&Entry> {
        let words = search_words(self.query.value());
        if words.is_empty() {
            return self.entries.iter().collect();
        }
        let scores: Vec<usize> = self.entries.iter().map(|e| e.score(&words)).collect();
        let best = scores.iter().copied().max().unwrap_or(0);
        if best == 0 {
            return Vec::new();
        }
        self.entries
            .iter()
            .zip(scores)
            .filter(|(_, score)| *score == best)
            .map(|(e, _)| e)
            .collect()
    }

    /// Handle a key. Returns `true` when the popup should close.
    pub fn handle_key(&mut self, key: KeyEvent) -> bool {
        if key.kind == KeyEventKind::Release {
            return false;
        }
        let ctrl_c =
            key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c');
        let alt_slash = key.modifiers.contains(KeyModifiers::ALT) && key.code == KeyCode::Char('/');
        match key.code {
            KeyCode::Esc | KeyCode::Enter => return true,
            _ if ctrl_c || alt_slash => return true,
            KeyCode::Down => self.scroll = self.scroll.saturating_add(1),
            KeyCode::Up => self.scroll = self.scroll.saturating_sub(1),
            KeyCode::PageDown => self.scroll = self.scroll.saturating_add(10),
            KeyCode::PageUp => self.scroll = self.scroll.saturating_sub(10),
            _ => {
                self.query = crate::app::edit(std::mem::take(&mut self.query), key);
                self.scroll = 0;
            }
        }
        false
    }

    /// Draw the whole popup.
    pub fn draw(&mut self, frame: &mut Frame<'_>) {
        let [search, list, hint] = Layout::vertical([
            Constraint::Length(2),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .areas(frame.area());
        let muted = Style::new().fg(self.theme.muted_foreground);
        frame.render_widget(
            InputWidget::new(&self.query)
                .themed(&self.theme)
                .focused(true)
                .prefix(Span::styled(
                    " How do I… ",
                    Style::new().fg(self.theme.accent),
                ))
                .placeholder("hide the sidebar, new tab, split…"),
            search,
        );

        let visible = self.visible();
        let width = visible.iter().map(|e| e.keys.width()).max().unwrap_or(0);
        let mut lines: Vec<Line<'_>> = Vec::new();
        let mut section = "";
        for e in &visible {
            if e.section != section {
                if !lines.is_empty() {
                    lines.push(Line::raw(""));
                }
                section = e.section;
                lines.push(Line::styled(
                    format!(" {section}"),
                    Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                ));
            }
            lines.push(Line::from(vec![
                Span::styled(
                    format!("   {:<width$}  ", e.keys),
                    Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                ),
                Span::raw(e.what.clone()),
            ]));
        }
        if lines.is_empty() {
            lines.push(Line::styled(" Nothing matches. Try another word.", muted));
        }

        // Keep the last line reachable, and no further.
        let max = u16::try_from(lines.len())
            .unwrap_or(u16::MAX)
            .saturating_sub(list.height);
        self.scroll = self.scroll.min(max);
        frame.render_widget(Paragraph::new(lines).scroll((self.scroll, 0)), list);
        frame.render_widget(
            Line::styled(" type to search · ↑↓ scroll · Esc closes", muted),
            hint,
        );
    }
}
