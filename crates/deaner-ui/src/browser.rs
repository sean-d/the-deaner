//! The project browser popup: walk folders, pick a repo or folder to open.
//!
//! No IO here: the caller supplies a function that lists a directory.

use crate::icons::Icons;
use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{List, ListItem, ListState};
use ratcn::{InputState, InputWidget, Theme};
use std::path::{Path, PathBuf};
use unicode_width::UnicodeWidthStr;

/// A folder inside the current directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirItem {
    pub name: String,
    pub path: PathBuf,
    pub is_repo: bool,
    /// Already in the sidebar.
    pub added: bool,
}

/// What the user chose.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BrowserAction {
    None,
    Close,
    Pick(PathBuf),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Row {
    /// Pick the folder being browsed.
    ThisFolder,
    Item(usize),
}

/// Lists the folders in a directory; `None` when it can't be read or doesn't exist.
type Lister = Box<dyn Fn(&Path) -> Option<Vec<DirItem>>>;

pub struct Browser {
    cwd: PathBuf,
    home: Option<PathBuf>,
    items: Vec<DirItem>,
    query: InputState,
    list: ListState,
    lister: Lister,
    icons: &'static Icons,
    theme: Theme,
    message: Option<String>,
}

impl Browser {
    #[must_use]
    pub fn new(
        start: PathBuf,
        home: Option<PathBuf>,
        icons: &'static Icons,
        lister: Lister,
    ) -> Self {
        let mut b = Self {
            cwd: start,
            home,
            items: Vec::new(),
            query: InputState::default(),
            list: ListState::default(),
            lister,
            icons,
            theme: crate::theme::terminal(),
            message: None,
        };
        b.load(None);
        b
    }

    /// Show an error without closing, so another folder can be picked.
    pub fn set_message(&mut self, message: impl Into<String>) {
        self.message = Some(message.into());
    }

    #[must_use]
    pub fn cwd(&self) -> &Path {
        &self.cwd
    }

    /// Read the current folder, selecting `focus` if it's there.
    fn load(&mut self, focus: Option<&Path>) {
        self.items = (self.lister)(&self.cwd).unwrap_or_default();
        self.items.sort_by_key(|i| i.name.to_lowercase());
        self.query = InputState::default();
        self.select_first();
        if let Some(i) = focus.and_then(|f| {
            self.rows()
                .iter()
                .position(|r| matches!(r, Row::Item(i) if self.items[*i].path == f))
        }) {
            self.list.select(Some(i));
        }
    }

    fn rows(&self) -> Vec<Row> {
        let query = self.query.value().trim().to_lowercase();
        let mut rows = Vec::new();
        if query.is_empty() {
            rows.push(Row::ThisFolder);
        }
        rows.extend(self.items.iter().enumerate().filter_map(|(i, item)| {
            let name = item.name.to_lowercase();
            // Dotfolders stay out of the way until asked for.
            let visible = if query.is_empty() {
                !name.starts_with('.')
            } else {
                name.contains(&query) && (!name.starts_with('.') || query.starts_with('.'))
            };
            visible.then_some(Row::Item(i))
        }));
        rows
    }

    /// Start on the first folder rather than "this folder".
    fn select_first(&mut self) {
        let rows = self.rows();
        let first = rows.iter().position(|r| matches!(r, Row::Item(_)));
        self.list.select(first.or((!rows.is_empty()).then_some(0)));
    }

    fn selected(&self) -> Option<Row> {
        self.list
            .selected()
            .and_then(|i| self.rows().get(i).copied())
    }

    /// The folders currently listed, for tests and callers.
    #[must_use]
    pub fn visible_names(&self) -> Vec<String> {
        self.rows()
            .into_iter()
            .filter_map(|r| match r {
                Row::Item(i) => Some(self.items[i].name.clone()),
                Row::ThisFolder => None,
            })
            .collect()
    }

    fn enter(&mut self, path: PathBuf) {
        self.cwd = path;
        self.message = None;
        self.load(None);
    }

    fn go_up(&mut self) {
        let Some(parent) = self.cwd.parent().map(Path::to_path_buf) else {
            return;
        };
        let came_from = std::mem::replace(&mut self.cwd, parent);
        self.message = None;
        self.load(Some(&came_from));
    }

    /// Typing a path works like a shell: anything ending in `/` that exists is entered,
    /// so `/`, `~/`, `../` and `go/` all jump, and a pasted path arrives step by step.
    fn follow_typed_path(&mut self) {
        let typed = self.query.value().trim().to_string();
        let Some(path) = typed.strip_suffix('/') else {
            return;
        };
        let target = if typed == "/" {
            PathBuf::from("/")
        } else if path == ".." {
            self.go_up();
            return;
        } else if path == "~" {
            let Some(home) = self.home.clone() else {
                return;
            };
            home
        } else if let Some(rest) = path.strip_prefix("~/") {
            let Some(home) = &self.home else { return };
            home.join(rest)
        } else {
            self.cwd.join(path)
        };
        if (self.lister)(&target).is_some() {
            self.enter(target);
        }
    }

    fn step(&mut self, delta: isize) {
        let len = self.rows().len() as isize;
        if len == 0 {
            return;
        }
        let cur = self.list.selected().unwrap_or(0) as isize;
        self.list
            .select(Some((cur + delta).clamp(0, len - 1) as usize));
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> BrowserAction {
        if key.kind == KeyEventKind::Release {
            return BrowserAction::None;
        }
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Esc => return BrowserAction::Close,
            KeyCode::Char('c') if ctrl => return BrowserAction::Close,
            KeyCode::Down => self.step(1),
            KeyCode::Up => self.step(-1),
            KeyCode::Char('n' | 'j') if ctrl => self.step(1),
            KeyCode::Char('p' | 'k') if ctrl => self.step(-1),
            KeyCode::PageDown => self.step(10),
            KeyCode::PageUp => self.step(-10),
            KeyCode::Left => self.go_up(),
            KeyCode::Backspace if self.query.value().is_empty() => self.go_up(),
            KeyCode::Right | KeyCode::Tab => {
                if let Some(Row::Item(i)) = self.selected() {
                    self.enter(self.items[i].path.clone());
                }
            }
            KeyCode::Enter => match self.selected() {
                Some(Row::ThisFolder) => return BrowserAction::Pick(self.cwd.clone()),
                Some(Row::Item(i)) => {
                    let item = &self.items[i];
                    if item.is_repo || item.added {
                        return BrowserAction::Pick(item.path.clone());
                    }
                    self.enter(item.path.clone());
                }
                None => {}
            },
            _ => {
                self.query = crate::app::edit(std::mem::take(&mut self.query), key);
                self.select_first();
                self.follow_typed_path();
            }
        }
        BrowserAction::None
    }

    fn display_path(&self) -> String {
        if let Some(home) = &self.home
            && let Ok(rest) = self.cwd.strip_prefix(home)
        {
            return if rest.as_os_str().is_empty() {
                "~".to_string()
            } else {
                format!("~/{}", rest.display())
            };
        }
        self.cwd.display().to_string()
    }

    fn row_line<'a>(&self, row: Row, width: usize) -> Line<'a> {
        let muted = Style::new().fg(self.theme.muted_foreground);
        match row {
            Row::ThisFolder => Line::from(vec![
                Span::styled(" + ", Style::new().fg(Color::Green)),
                Span::raw("Use this folder"),
            ]),
            Row::Item(i) => {
                let item = &self.items[i];
                let (icon, style) = if item.is_repo {
                    (self.icons.branch, Style::new().fg(Color::Green))
                } else {
                    (self.icons.folder, Style::new().fg(Color::Blue))
                };
                let note = if item.added { "in sidebar " } else { "" };
                let used = 3 + item.name.width() + note.width();
                Line::from(vec![
                    Span::styled(format!(" {icon} "), style),
                    Span::raw(item.name.clone()),
                    Span::raw(" ".repeat(width.saturating_sub(used))),
                    Span::styled(note, muted),
                ])
            }
        }
    }

    /// Draw the whole popup.
    pub fn draw(&mut self, frame: &mut Frame<'_>) {
        let [title, search, list, hint] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(2),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .areas(frame.area());
        let muted = Style::new().fg(self.theme.muted_foreground);
        frame.render_widget(
            Line::from(vec![
                Span::styled(
                    " Open a project  ",
                    Style::new()
                        .fg(self.theme.accent)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    self.display_path(),
                    Style::new().add_modifier(Modifier::BOLD),
                ),
            ]),
            title,
        );
        frame.render_widget(
            InputWidget::new(&self.query)
                .themed(&self.theme)
                .focused(true)
                .prefix(Span::styled(" Filter ", muted))
                .placeholder("type to narrow, or a path like /tmp/ or ~/"),
            search,
        );

        let rows = self.rows();
        if rows.is_empty() {
            frame.render_widget(Line::styled(" Nothing matches.", muted), list);
        } else {
            let width = usize::from(list.width);
            let items: Vec<ListItem<'_>> = rows
                .iter()
                .map(|r| ListItem::new(self.row_line(*r, width)))
                .collect();
            frame.render_stateful_widget(
                List::new(items).highlight_style(Style::new().bg(Color::DarkGray)),
                list,
                &mut self.list,
            );
        }
        let bottom = self.message.as_ref().map_or_else(
            || {
                Line::styled(
                    " Enter open · → into folder · ← up · ↑↓ move · Esc close",
                    muted,
                )
            },
            |m| Line::styled(format!(" {m}"), Style::new().fg(self.theme.destructive)),
        );
        frame.render_widget(bottom, hint);
    }
}
