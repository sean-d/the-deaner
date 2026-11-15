//! The guide: the-deaner's docs, built into the binary, in a popup you can read and search.
//!
//! Pages are markdown (the same files as on GitHub); [`crate::markdown`] draws them. No IO.

use crate::markdown::{Rendered, render};
use crate::theme::colors;
use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratcn::{InputState, InputWidget, Theme};

/// One page of the guide.
#[derive(Debug, Clone, Copy)]
pub struct GuidePage {
    /// For `the-deaner guide <slug>`.
    pub slug: &'static str,
    pub title: &'static str,
    pub source: &'static str,
}

/// What the caller should do after a key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuideAction {
    None,
    Close,
    /// Switch to the key cheat sheet.
    Keys,
}

enum View {
    List,
    Page(usize),
}

pub struct Guide {
    pages: Vec<GuidePage>,
    view: View,
    selected: usize,
    scroll: usize,
    /// The open page drawn at a width, so it isn't redrawn every frame.
    drawn: Option<(usize, u16, Rendered, Vec<String>)>,
    /// `/` typing a search.
    searching: Option<InputState>,
    query: String,
    /// The line of the current match.
    found: Option<usize>,
    body_height: usize,
    version: Option<String>,
    theme: Theme,
}

impl Guide {
    #[must_use]
    pub fn new(pages: Vec<GuidePage>) -> Self {
        Self {
            pages,
            view: View::List,
            selected: 0,
            scroll: 0,
            drawn: None,
            searching: None,
            query: String::new(),
            found: None,
            body_height: 20,
            version: None,
            theme: crate::theme::the_deaner(),
        }
    }

    /// Show the running version in the corner.
    #[must_use]
    pub fn with_version(mut self, version: impl Into<String>) -> Self {
        self.version = Some(version.into());
        self
    }

    /// Open a page by slug or the start of its title (`workflows`, `getting`). `false`
    /// when nothing matches.
    pub fn open(&mut self, name: &str) -> bool {
        let name = name.to_lowercase();
        let found = self.pages.iter().position(|p| p.slug == name).or_else(|| {
            self.pages.iter().position(|p| {
                p.title.to_lowercase().starts_with(&name) || p.slug.starts_with(&name)
            })
        });
        if let Some(i) = found {
            self.open_page(i);
        }
        found.is_some()
    }

    /// Slugs, for an error message.
    #[must_use]
    pub fn slugs(&self) -> Vec<&'static str> {
        self.pages.iter().map(|p| p.slug).collect()
    }

    /// The page being read, if any (tests).
    #[must_use]
    pub fn page(&self) -> Option<&'static str> {
        match self.view {
            View::Page(i) => Some(self.pages[i].slug),
            View::List => None,
        }
    }

    #[must_use]
    pub fn scroll(&self) -> usize {
        self.scroll
    }

    fn open_page(&mut self, i: usize) {
        self.selected = i;
        self.view = View::Page(i);
        self.scroll = 0;
        self.found = None;
        self.searching = None;
    }

    fn lines(&self) -> usize {
        self.drawn.as_ref().map_or(0, |d| d.2.lines.len())
    }

    fn max_scroll(&self) -> usize {
        self.lines().saturating_sub(self.body_height)
    }

    fn scroll_by(&mut self, delta: isize) {
        self.scroll = self
            .scroll
            .saturating_add_signed(delta)
            .min(self.max_scroll());
    }

    /// Move to the next (or previous) line containing the query, from just past the last
    /// match, wrapping around the page.
    fn find(&mut self, forward: bool) {
        let Some((_, _, _, plain)) = &self.drawn else {
            return;
        };
        let q = self.query.to_lowercase();
        if q.is_empty() || plain.is_empty() {
            return;
        }
        let n = plain.len();
        let from = self.found.unwrap_or(if forward {
            self.scroll.wrapping_sub(1)
        } else {
            self.scroll
        });
        let hit = (1..=n).find_map(|k| {
            let i = if forward {
                from.wrapping_add(k) % n
            } else {
                (from + n - (k % n)) % n
            };
            plain[i].to_lowercase().contains(&q).then_some(i)
        });
        self.found = hit;
        if let Some(i) = hit {
            // A couple of lines of context above the match.
            self.scroll = i.saturating_sub(2).min(self.max_scroll());
        }
    }

    /// Handle a key.
    pub fn handle_key(&mut self, key: KeyEvent) -> GuideAction {
        if key.kind == KeyEventKind::Release {
            return GuideAction::None;
        }
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        if ctrl && key.code == KeyCode::Char('c') {
            return GuideAction::Close;
        }
        if key.modifiers.contains(KeyModifiers::ALT) && key.code == KeyCode::Char('/') {
            return GuideAction::Close;
        }
        if key.code == KeyCode::Tab {
            return GuideAction::Keys;
        }
        if let Some(input) = self.searching.take() {
            match key.code {
                KeyCode::Esc => {}
                KeyCode::Enter => {
                    self.query = input.value().trim().to_string();
                    self.found = None;
                    self.find(true);
                }
                _ => self.searching = Some(crate::app::edit(input, key)),
            }
            return GuideAction::None;
        }
        match self.view {
            View::List => match key.code {
                KeyCode::Esc | KeyCode::Char('q') => return GuideAction::Close,
                KeyCode::Down | KeyCode::Char('j') => {
                    self.selected = (self.selected + 1).min(self.pages.len().saturating_sub(1));
                }
                KeyCode::Up | KeyCode::Char('k') => self.selected = self.selected.saturating_sub(1),
                KeyCode::Char('g') => self.selected = 0,
                KeyCode::Char('G') => self.selected = self.pages.len().saturating_sub(1),
                KeyCode::Enter | KeyCode::Right | KeyCode::Char('l') => {
                    self.open_page(self.selected);
                }
                _ => {}
            },
            View::Page(i) => {
                let half = isize::try_from(self.body_height / 2).unwrap_or(10).max(1);
                let full = isize::try_from(self.body_height).unwrap_or(20).max(1) - 2;
                match key.code {
                    KeyCode::Esc | KeyCode::Char('q' | 'h') | KeyCode::Left => {
                        self.view = View::List;
                    }
                    KeyCode::Down | KeyCode::Char('j') => self.scroll_by(1),
                    KeyCode::Up | KeyCode::Char('k') => self.scroll_by(-1),
                    KeyCode::Char('d') if ctrl => self.scroll_by(half),
                    KeyCode::Char('u') if ctrl => self.scroll_by(-half),
                    KeyCode::Char(' ') | KeyCode::PageDown => self.scroll_by(full),
                    KeyCode::Char('b') | KeyCode::PageUp => self.scroll_by(-full),
                    KeyCode::Char('g') | KeyCode::Home => self.scroll = 0,
                    KeyCode::Char('G') | KeyCode::End => self.scroll = self.max_scroll(),
                    KeyCode::Char(']') if i + 1 < self.pages.len() => self.open_page(i + 1),
                    KeyCode::Char('[') if i > 0 => self.open_page(i - 1),
                    KeyCode::Char('/') => self.searching = Some(InputState::default()),
                    KeyCode::Char('n') => self.find(true),
                    KeyCode::Char('N') => self.find(false),
                    _ => {}
                }
            }
        }
        GuideAction::None
    }

    /// Draw the whole popup.
    pub fn draw(&mut self, frame: &mut Frame<'_>) {
        let full = frame.area();
        frame.buffer_mut().set_style(full, crate::theme::base());
        let [title, body, hint] = Layout::vertical([
            Constraint::Length(2),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .areas(full);
        let muted = Style::new().fg(self.theme.muted_foreground);
        let gold = Style::new().fg(colors::GOLD).add_modifier(Modifier::BOLD);
        self.body_height = usize::from(body.height);

        let heading = match self.view {
            View::List => " Guide".to_string(),
            View::Page(i) => format!(" Guide · {}", self.pages[i].title),
        };
        frame.render_widget(Line::styled(heading, gold), title);
        if let Some(v) = &self.version {
            frame.render_widget(
                Line::styled(format!("the-deaner {v} "), muted).right_aligned(),
                title,
            );
        }

        match self.view {
            View::List => {
                let mut lines = vec![Line::styled(
                    " The docs for the version you're running. Enter opens a page.",
                    muted,
                )];
                lines.push(Line::raw(""));
                for (i, p) in self.pages.iter().enumerate() {
                    let style = if i == self.selected {
                        Style::new()
                            .bg(colors::SEL)
                            .fg(colors::FG)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::new().fg(colors::FG)
                    };
                    lines.push(Line::from(vec![
                        Span::styled(format!("   {:<20}", p.title), style),
                        Span::styled(format!("  {}", p.slug), muted),
                    ]));
                }
                frame.render_widget(Paragraph::new(lines), body);
                frame.render_widget(
                    Line::styled(" ↑↓ choose · Enter open · Tab keys · Esc close", muted),
                    hint,
                );
            }
            View::Page(i) => {
                let width = body.width.saturating_sub(2);
                let stale = !matches!(&self.drawn, Some((p, w, ..)) if *p == i && *w == width);
                if stale {
                    let r = render(self.pages[i].source, width);
                    let plain = r.plain();
                    self.drawn = Some((i, width, r, plain));
                    self.scroll = self.scroll.min(self.max_scroll());
                }
                let found = self.found;
                let scroll = self.scroll;
                if let Some((_, _, r, _)) = &self.drawn {
                    let lines: Vec<Line<'_>> = r
                        .lines
                        .iter()
                        .enumerate()
                        .skip(scroll)
                        .take(usize::from(body.height))
                        .map(|(n, l)| {
                            let mut l = l.clone();
                            l.spans.insert(0, Span::raw(" "));
                            if Some(n) == found {
                                l = l.style(Style::new().bg(colors::SEL));
                            }
                            l
                        })
                        .collect();
                    frame.render_widget(Paragraph::new(lines), body);
                }
                match &self.searching {
                    Some(input) => frame.render_widget(
                        InputWidget::new(input)
                            .themed(&self.theme)
                            .focused(true)
                            .prefix(Span::styled(" / ", Style::new().fg(colors::GOLD))),
                        hint,
                    ),
                    None => {
                        let total = self.lines().max(1);
                        let pct =
                            ((self.scroll + usize::from(body.height)).min(total) * 100) / total;
                        let mut text = String::from(
                            " j/k scroll · Ctrl-d/u half page · / search · n next · [ ] page · h back · Tab keys",
                        );
                        if !self.query.is_empty() && self.found.is_none() {
                            text = format!(" no match for \"{}\" · {text}", self.query);
                        }
                        frame.render_widget(Line::styled(text, muted), hint);
                        frame.render_widget(
                            Line::styled(format!("{pct}% "), muted).right_aligned(),
                            hint,
                        );
                    }
                }
            }
        }
    }
}
