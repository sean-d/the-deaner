//! The project browser popup: walk folders, pick a repo or folder to open.
//!
//! No IO here: the caller supplies a function that lists a directory.

use crate::icons::Icons;
use crate::theme::colors;
use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Modifier, Style};
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
    /// Add this folder to the favorites, or remove it.
    ToggleFavorite(PathBuf),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Row {
    /// A section title, or a blank line when empty. Can't be selected.
    Heading(&'static str),
    /// Jump to a favorite folder.
    Favorite(usize),
    /// Pick the folder being browsed.
    ThisFolder,
    /// Go up to the parent folder.
    Up,
    Item(usize),
}

/// Lists the folders in a directory; `None` when it can't be read or doesn't exist.
type Lister = Box<dyn Fn(&Path) -> Option<Vec<DirItem>>>;

pub struct Browser {
    /// The folder listed.
    cwd: PathBuf,
    /// Where a typed relative path starts: the folder you were in before typing it.
    base: PathBuf,
    /// A typed path names a folder that doesn't exist (or can't be read): list nothing.
    missing: bool,
    home: Option<PathBuf>,
    items: Vec<DirItem>,
    favorites: Vec<PathBuf>,
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
            base: start.clone(),
            missing: false,
            cwd: start,
            home,
            items: Vec::new(),
            favorites: Vec::new(),
            query: InputState::default(),
            list: ListState::default(),
            lister,
            icons,
            theme: crate::theme::the_deaner(),
            message: None,
        };
        b.load(None);
        b
    }

    /// Set the favorites, leaving out folders that don't exist right now (they may be on a
    /// drive that's unplugged). The cursor stays on the same folder.
    pub fn set_favorites(&mut self, favorites: Vec<PathBuf>) {
        let selected = self.selected().and_then(|r| self.row_path(r));
        self.favorites = favorites
            .into_iter()
            .filter(|f| (self.lister)(f).is_some())
            .collect();
        let found = selected.and_then(|path| {
            self.rows()
                .iter()
                .position(|r| self.row_path(*r).as_ref() == Some(&path))
        });
        if found.is_some() {
            self.list.select(found);
        } else {
            self.select_first();
        }
    }

    /// The favorites currently listed.
    #[must_use]
    pub fn visible_favorites(&self) -> Vec<PathBuf> {
        self.rows()
            .into_iter()
            .filter_map(|r| match r {
                Row::Favorite(i) => Some(self.favorites[i].clone()),
                _ => None,
            })
            .collect()
    }

    /// The folder a row stands for.
    fn row_path(&self, row: Row) -> Option<PathBuf> {
        match row {
            Row::Heading(_) | Row::Up => None,
            Row::Favorite(i) => self.favorites.get(i).cloned(),
            Row::ThisFolder => Some(self.cwd.clone()),
            Row::Item(i) => self.items.get(i).map(|item| item.path.clone()),
        }
    }

    /// Show an error without closing, so another folder can be picked.
    pub fn set_message(&mut self, message: impl Into<String>) {
        self.message = Some(message.into());
    }

    #[must_use]
    pub fn cwd(&self) -> &Path {
        &self.cwd
    }

    /// What's in the box.
    #[must_use]
    pub fn typed(&self) -> &str {
        self.query.value()
    }

    /// Read the current folder, selecting `focus` if it's there. Keeps what's typed.
    fn load(&mut self, focus: Option<&Path>) {
        self.items = (self.lister)(&self.cwd).unwrap_or_default();
        self.items.sort_by_key(|i| i.name.to_lowercase());
        self.select_first();
        if let Some(i) = focus.and_then(|f| {
            self.rows()
                .iter()
                .position(|r| matches!(r, Row::Item(i) if self.items[*i].path == f))
        }) {
            self.list.select(Some(i));
        }
    }

    /// What's typed, as a path: anything with a `/` in it.
    fn typed_path(&self) -> Option<&str> {
        let typed = self.query.value().trim();
        typed.contains('/').then_some(typed)
    }

    /// The text that narrows the list: everything typed, or what's after a path's last `/`.
    fn narrowing(&self) -> String {
        let typed = self.query.value().trim();
        let leaf = match typed.rsplit_once('/') {
            Some((_, leaf)) => leaf,
            None => typed,
        };
        leaf.to_lowercase()
    }

    fn rows(&self) -> Vec<Row> {
        let query = self.narrowing();
        let path_mode = self.typed_path().is_some();
        let mut rows = Vec::new();
        if path_mode && self.missing {
            return rows;
        }
        // While a path is typed, the list is that folder; favorites would only get in the way.
        let favorites: Vec<Row> = (0..self.favorites.len())
            .filter(|i| {
                !path_mode
                    && self
                        .display(&self.favorites[*i])
                        .to_lowercase()
                        .contains(&query)
            })
            .map(Row::Favorite)
            .collect();
        if !favorites.is_empty() {
            rows.push(Row::Heading("Favorites"));
            rows.extend(favorites);
            rows.push(Row::Heading(""));
        }
        if query.is_empty() {
            if self.cwd.parent().is_some() {
                rows.push(Row::Up);
            }
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
        let any = rows.iter().position(|r| !matches!(r, Row::Heading(_)));
        self.list.select(first.or(any));
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
                _ => None,
            })
            .collect()
    }

    /// Into a folder. While a path is typed, the path grows to match, so the box always
    /// says where you are; otherwise the box starts empty.
    fn enter(&mut self, path: PathBuf) {
        self.message = None;
        if self.typed_path().is_some() {
            self.query = InputState::new(self.path_text(&path));
            self.follow_typed_path();
            return;
        }
        self.query = InputState::default();
        self.cwd = path.clone();
        self.base = path;
        self.missing = false;
        self.load(None);
    }

    fn go_up(&mut self) {
        let Some(parent) = self.cwd.parent().map(Path::to_path_buf) else {
            return;
        };
        self.message = None;
        if self.typed_path().is_some() {
            self.query = InputState::new(self.path_text(&parent));
            self.follow_typed_path();
            return;
        }
        self.query = InputState::default();
        let came_from = std::mem::replace(&mut self.cwd, parent);
        self.base = self.cwd.clone();
        self.missing = false;
        self.load(Some(&came_from));
    }

    /// A folder as you'd type it, ending in `/`: `~/dev/`, `/tmp/`, `/`.
    fn path_text(&self, path: &Path) -> String {
        let shown = self.display(path);
        if shown.ends_with('/') {
            shown
        } else {
            format!("{shown}/")
        }
    }

    /// The folder named by a typed path's part before its last `/`: from `/`, from `~`,
    /// or from where you were before you started typing. `.` and `..` work as in a shell.
    fn typed_dir(&self, typed: &str) -> Option<PathBuf> {
        let (dir, _) = typed.rsplit_once('/')?;
        let (root, rest) = if typed.starts_with('/') {
            (PathBuf::from("/"), dir)
        } else if dir == "~" || dir.starts_with("~/") {
            (self.home.clone()?, dir.trim_start_matches('~'))
        } else {
            (self.base.clone(), dir)
        };
        let mut out = root;
        for part in rest.split('/') {
            match part {
                "" | "." => {}
                ".." => {
                    out.pop();
                }
                name => out.push(name),
            }
        }
        Some(out)
    }

    /// Typing a path works like a shell: the box keeps what you type, the list shows the
    /// folder up to the last `/`, and what comes after narrows it. So `/tmp/cl`, `~/dev/`
    /// and `../` all work, and deleting back past a `/` goes back out of that folder.
    fn follow_typed_path(&mut self) {
        let Some(typed) = self.typed_path().map(str::to_string) else {
            // No path any more: back to plain narrowing where you started.
            if self.missing || self.cwd != self.base {
                self.missing = false;
                self.cwd = self.base.clone();
                self.load(None);
            }
            return;
        };
        match self
            .typed_dir(&typed)
            .filter(|t| (self.lister)(t).is_some())
        {
            Some(dir) => {
                self.missing = false;
                if dir == self.cwd {
                    self.select_first();
                } else {
                    self.cwd = dir;
                    self.load(None);
                }
            }
            None => {
                self.missing = true;
                self.list.select(None);
            }
        }
    }

    fn step(&mut self, delta: isize) {
        let rows = self.rows();
        let len = rows.len() as isize;
        if len == 0 {
            return;
        }
        let cur = self.list.selected().unwrap_or(0) as isize;
        let selectable = |i: isize| !matches!(rows[i as usize], Row::Heading(_));
        // Past a heading in the direction of travel; at the end of the list, stay nearby.
        let mut target = (cur + delta).clamp(0, len - 1);
        let dir = delta.signum();
        while (0..len).contains(&target) && !selectable(target) {
            target += dir;
        }
        if !(0..len).contains(&target) {
            target = cur;
        }
        self.list.select(Some(target as usize));
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> BrowserAction {
        if key.kind == KeyEventKind::Release {
            return BrowserAction::None;
        }
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Esc => return BrowserAction::Close,
            KeyCode::Char('c') if ctrl => return BrowserAction::Close,
            KeyCode::Char('f') if ctrl => {
                if let Some(path) = self.selected().and_then(|r| self.row_path(r)) {
                    return BrowserAction::ToggleFavorite(path);
                }
            }
            KeyCode::Down => self.step(1),
            KeyCode::Up => self.step(-1),
            KeyCode::Char('n' | 'j') if ctrl => self.step(1),
            KeyCode::Char('p' | 'k') if ctrl => self.step(-1),
            KeyCode::PageDown => self.step(10),
            KeyCode::PageUp => self.step(-10),
            // Left moves through what's typed; with nothing typed, it goes up a folder.
            KeyCode::Left | KeyCode::Backspace if self.query.value().is_empty() => self.go_up(),
            KeyCode::Right | KeyCode::Tab => match self.selected() {
                Some(Row::Up) => self.go_up(),
                Some(Row::Item(i)) => self.enter(self.items[i].path.clone()),
                Some(Row::Favorite(i)) => self.enter(self.favorites[i].clone()),
                _ => {}
            },
            KeyCode::Enter => match self.selected() {
                Some(Row::ThisFolder) => return BrowserAction::Pick(self.cwd.clone()),
                Some(Row::Up) => self.go_up(),
                Some(Row::Favorite(i)) => self.enter(self.favorites[i].clone()),
                Some(Row::Item(i)) => {
                    let item = &self.items[i];
                    if item.is_repo || item.added {
                        return BrowserAction::Pick(item.path.clone());
                    }
                    self.enter(item.path.clone());
                }
                Some(Row::Heading(_)) | None => {}
            },
            _ => {
                self.query = crate::app::edit(std::mem::take(&mut self.query), key);
                self.select_first();
                self.follow_typed_path();
            }
        }
        BrowserAction::None
    }

    /// A path with the home directory shortened to `~`.
    fn display(&self, path: &Path) -> String {
        if let Some(home) = &self.home
            && let Ok(rest) = path.strip_prefix(home)
        {
            return if rest.as_os_str().is_empty() {
                "~".to_string()
            } else {
                format!("~/{}", rest.display())
            };
        }
        path.display().to_string()
    }

    fn row_line<'a>(&self, row: Row, width: usize) -> Line<'a> {
        let muted = Style::new().fg(self.theme.muted_foreground);
        match row {
            Row::Heading(title) => Line::styled(
                format!(" {title}"),
                Style::new().fg(colors::GOLD).add_modifier(Modifier::BOLD),
            ),
            Row::Favorite(i) => Line::from(vec![
                Span::styled(
                    format!(" {} ", self.icons.pinned),
                    Style::new().fg(colors::GOLD),
                ),
                Span::raw(self.display(&self.favorites[i])),
            ]),
            Row::ThisFolder => Line::from(vec![
                Span::styled(" + ", Style::new().fg(colors::GREEN)),
                Span::raw("Use this folder"),
            ]),
            Row::Up => {
                let parent = self.cwd.parent().unwrap_or(&self.cwd);
                Line::from(vec![
                    Span::styled(" ↑ ", Style::new().fg(colors::GOLD)),
                    Span::raw("Up to "),
                    Span::styled(self.display(parent), muted),
                ])
            }
            Row::Item(i) => {
                let item = &self.items[i];
                let (icon, style) = if item.is_repo {
                    (self.icons.branch, Style::new().fg(colors::GREEN))
                } else {
                    (self.icons.folder, Style::new().fg(colors::BLUE))
                };
                let favorite = self.favorites.contains(&item.path);
                let note = match (favorite, item.added) {
                    (true, true) => "favorite · in sidebar ",
                    (true, false) => "favorite ",
                    (false, true) => "in sidebar ",
                    (false, false) => "",
                };
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
        let full = frame.area();
        frame.buffer_mut().set_style(full, crate::theme::base());
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
                    self.display(&self.cwd),
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
                List::new(items).highlight_style(Style::new().bg(colors::SEL)),
                list,
                &mut self.list,
            );
        }
        let bottom = self.message.as_ref().map_or_else(
            || {
                Line::styled(
                    " Enter open · Tab/→ into folder · ← up · ↑↓ move · Ctrl-f favorite · Esc close",
                    muted,
                )
            },
            |m| Line::styled(format!(" {m}"), Style::new().fg(self.theme.destructive)),
        );
        frame.render_widget(bottom, hint);
    }
}
