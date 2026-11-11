//! Sidebar state and input handling.
//!
//! The app never talks to tmux or git. Input becomes a [`Command`] for the
//! caller to execute, and fresh data arrives through [`App::set_groups`]. That
//! keeps everything here deterministic and snapshot-testable.

use crate::keymap::{Action, action_for};
use deaner_core::sidebar::{RepoGroup, Row, Section, WorkspaceView, WsStatus, rows};
use deaner_core::{AgentState, TabSpec};
use ratatui::crossterm::event::{
    KeyCode, KeyEvent, KeyEventKind, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::layout::Rect;
use ratatui::widgets::ListState;
use ratcn::{InputState, Theme, Toast, ToasterState};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Something the sidebar wants done.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Open(WorkspaceView),
    NewWorktree {
        repo: PathBuf,
        branch: String,
    },
    NewTab {
        ws: WorkspaceView,
        tab: TabSpec,
    },
    AddRepo(String),
    TogglePin(PathBuf),
    MovePin(PathBuf, isize),
    ToggleCollapse(PathBuf),
    /// Remove a linked worktree and its session.
    Archive(WorkspaceView),
    /// Forget a repo (files untouched).
    ForgetRepo(PathBuf),
    Park(WorkspaceView),
    Save,
    /// Rebuild workspaces from the latest snapshot.
    Restore,
    /// Open the shortcut cheat sheet popup.
    Cheatsheet,
    /// Open the project browser popup.
    BrowseRepos,
    Refresh,
    Quit,
    /// Detach from the frame; everything keeps running.
    Detach,
    /// Show the end of a tab's output (`<session>:<tab>` or `<session>`) in a popup.
    Peek(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PromptKind {
    NewWorktree { repo: PathBuf, repo_name: String },
    AddRepo,
}

pub enum Mode {
    Normal,
    Filter(InputState),
    Prompt {
        kind: PromptKind,
        input: InputState,
    },
    Picker {
        ws: Box<WorkspaceView>,
        choices: Vec<TabSpec>,
        index: usize,
    },
    Confirm {
        message: String,
        command: Box<Command>,
    },
    /// Help overlay, scrolled down by this many lines.
    Help(u16),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SelKey {
    Header(Section),
    Repo(PathBuf),
    Ws(PathBuf),
}

type TabChoices = Box<dyn Fn(&Path) -> Vec<TabSpec>>;

pub struct App {
    pub(crate) groups: Vec<RepoGroup>,
    pub(crate) rows: Vec<Row>,
    pub(crate) list: ListState,
    pub(crate) filter: String,
    pub(crate) mode: Mode,
    pub(crate) toasts: ToasterState<'static>,
    pub(crate) clock: Duration,
    pub(crate) tick: u64,
    pub(crate) list_area: Rect,
    /// Rows of the `▲ n more` and `▼ n more` lines, while the list doesn't fit.
    pub(crate) scroll_marks: Option<(u16, u16)>,
    pub(crate) loaded: bool,
    prev_agents: HashMap<PathBuf, AgentState>,
    /// Workspaces whose agent finished while they weren't on screen.
    unseen: HashSet<PathBuf>,
    /// Workspaces whose agent started waiting on you and that you haven't been to since.
    waiting_unvisited: HashSet<PathBuf>,
    pub theme: Theme,
    pub icons: &'static crate::icons::Icons,
    /// How the project shown in the terminal is marked.
    pub open_style: deaner_core::OpenStyle,
    /// Running outside the frame: `q` quits.
    pub standalone: bool,
    /// Choices for "new tab", given a repo path.
    pub tab_choices: TabChoices,
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

impl App {
    #[must_use]
    pub fn new() -> Self {
        Self {
            groups: Vec::new(),
            rows: Vec::new(),
            list: ListState::default(),
            filter: String::new(),
            mode: Mode::Normal,
            toasts: ToasterState::default(),
            clock: Duration::ZERO,
            tick: 0,
            list_area: Rect::default(),
            scroll_marks: None,
            loaded: false,
            prev_agents: HashMap::new(),
            unseen: HashSet::new(),
            waiting_unvisited: HashSet::new(),
            theme: crate::theme::deaner(),
            icons: &crate::icons::UNICODE,
            open_style: deaner_core::OpenStyle::default(),
            standalone: false,
            tab_choices: Box::new(|_| vec![TabSpec::new("shell", None)]),
        }
    }

    // ---- data in -------------------------------------------------------

    /// Replace the sidebar contents, keeping the selection on the same item.
    /// Agents that just started waiting on the user (or just finished) raise a toast.
    pub fn set_groups(&mut self, groups: Vec<RepoGroup>) {
        let key = self.selected_key();
        let mut agents = HashMap::new();
        for g in &groups {
            for w in &g.workspaces {
                let prev = self.prev_agents.get(&w.path).copied();
                if self.loaded {
                    match (prev, w.agent) {
                        (Some(p), AgentState::Awaiting) if p != AgentState::Awaiting => {
                            self.toast(Toast::warning(format!("{} needs you", w.name)));
                            self.waiting_unvisited.insert(w.path.clone());
                        }
                        (Some(AgentState::Busy), AgentState::Idle) => {
                            self.toast(Toast::success(format!("{} finished", w.name)));
                            self.unseen.insert(w.path.clone());
                        }
                        _ => {}
                    }
                } else if w.agent == AgentState::Awaiting {
                    // Already waiting when the sidebar started.
                    self.waiting_unvisited.insert(w.path.clone());
                }
                // Looking at it counts as seeing it, and a stopped agent has nothing to see.
                if w.current || w.status != WsStatus::Live {
                    self.unseen.remove(&w.path);
                    self.waiting_unvisited.remove(&w.path);
                }
                if w.agent != AgentState::Awaiting {
                    self.waiting_unvisited.remove(&w.path);
                }
                agents.insert(w.path.clone(), w.agent);
            }
        }
        self.prev_agents = agents;
        self.loaded = true;
        self.groups = groups;
        self.rebuild_rows(key);
    }

    fn rebuild_rows(&mut self, key: Option<SelKey>) {
        let boxed = self.open_style == deaner_core::OpenStyle::Box;
        self.rows = rows(&self.groups, &self.filter, boxed);
        let found = key.and_then(|k| self.rows.iter().position(|r| self.key_of(*r) == k));
        let idx = found
            .or_else(|| self.list.selected())
            .unwrap_or(0)
            .min(self.rows.len().saturating_sub(1));
        self.select_near(idx, 1);
    }

    /// Whether the cursor can stop on row `i`: workspaces, and a repo row only when
    /// it's all there is of the repo (collapsed). An expanded repo's name opens the same
    /// thing as its workspace, so the cursor goes straight between workspaces.
    fn can_select(&self, i: usize) -> bool {
        match self.rows.get(i) {
            Some(Row::Repo { group }) => !matches!(
                self.rows.get(i + 1),
                Some(Row::Workspace { group: g, .. }) if g == group
            ),
            Some(row) => row.selectable(),
            None => false,
        }
    }

    /// Select the row at `target`, or the nearest selectable one: headings and the
    /// names of expanded repos are skipped in direction `dir` first, then the other way.
    /// Reaching the first selectable row scrolls its heading back into view.
    fn select_near(&mut self, target: usize, dir: isize) {
        let selectable = |i: isize| usize::try_from(i).ok().filter(|&i| self.can_select(i));
        let search = |dir: isize| {
            let mut i = target as isize;
            while i >= 0 && (i as usize) < self.rows.len() {
                if let Some(found) = selectable(i) {
                    return Some(found);
                }
                i += dir;
            }
            None
        };
        let found = search(dir).or_else(|| search(-dir));
        let first = (0..self.rows.len() as isize).find_map(selectable);
        self.list.select(found);
        if found.is_some() && found == first {
            *self.list.offset_mut() = 0;
        }
    }

    /// An agent here is waiting on the user, or finished while they were elsewhere.
    #[must_use]
    pub fn needs_attention(&self, w: &WorkspaceView) -> bool {
        w.agent == AgentState::Awaiting || self.unseen.contains(&w.path)
    }

    /// Advance the animation/toast clock.
    pub fn advance(&mut self, dt: Duration) {
        self.clock += dt;
        self.tick = self.tick.wrapping_add(1);
        self.toasts.prune_expired(self.clock);
    }

    /// Whether `w`'s row gets the orange background: its agent started waiting on you, or
    /// finished while you were elsewhere, and you haven't been there since.
    #[must_use]
    pub fn unvisited(&self, w: &WorkspaceView) -> bool {
        self.unseen.contains(&w.path) || self.waiting_unvisited.contains(&w.path)
    }

    pub fn toast(&mut self, toast: Toast<'static>) {
        self.toasts.push(toast, self.clock);
    }

    pub fn error(&mut self, message: impl Into<String>) {
        self.toast(Toast::error(message.into()).duration(Duration::from_secs(8)));
    }

    /// Show a spinner toast for work in progress, keyed by `id`.
    pub fn progress(&mut self, id: &str, message: impl Into<String>) {
        let toast = Toast::loading(message.into())
            .persistent()
            .with_id(id.to_string());
        if !self.toasts.replace(id, toast.clone(), self.clock) {
            self.toasts.push(toast, self.clock);
        }
    }

    /// Replace the progress toast `id` with its outcome (or just remove it).
    pub fn finish(&mut self, id: &str, outcome: Option<Toast<'static>>) {
        match outcome {
            Some(toast) => {
                if !self.toasts.replace(id, toast.clone(), self.clock) {
                    self.toasts.push(toast, self.clock);
                }
            }
            None => {
                let _ = self.toasts.dismiss(id);
            }
        }
    }

    /// Ask a yes/no question; `y` runs `command`.
    pub fn confirm(&mut self, message: impl Into<String>, command: Command) {
        self.mode = Mode::Confirm {
            message: message.into(),
            command: Box::new(command),
        };
    }

    // ---- selection -----------------------------------------------------

    fn key_of(&self, row: Row) -> SelKey {
        match row {
            Row::Header { section } | Row::Gap { section } => SelKey::Header(section),
            Row::BoxEnd { group } => SelKey::Header(self.groups[group].section),
            Row::Repo { group } => SelKey::Repo(self.groups[group].path.clone()),
            Row::Workspace { group, ws } | Row::Agent { group, ws, .. } => {
                SelKey::Ws(self.groups[group].workspaces[ws].path.clone())
            }
        }
    }

    fn selected_key(&self) -> Option<SelKey> {
        self.selected_row().map(|r| self.key_of(r))
    }

    #[must_use]
    pub fn selected_row(&self) -> Option<Row> {
        self.list.selected().and_then(|i| self.rows.get(i).copied())
    }

    #[must_use]
    pub fn selected_group(&self) -> Option<&RepoGroup> {
        self.selected_row().and_then(|r| match r {
            Row::Header { .. } | Row::Gap { .. } | Row::BoxEnd { .. } | Row::Agent { .. } => None,
            Row::Repo { group } | Row::Workspace { group, .. } => Some(&self.groups[group]),
        })
    }

    /// The workspace under the cursor; on a repo row, its main checkout.
    #[must_use]
    pub fn selected_ws(&self) -> Option<&WorkspaceView> {
        match self.selected_row()? {
            Row::Header { .. } | Row::Gap { .. } | Row::BoxEnd { .. } | Row::Agent { .. } => None,
            Row::Workspace { group, ws } => Some(&self.groups[group].workspaces[ws]),
            Row::Repo { group } => {
                let g = &self.groups[group];
                g.workspaces
                    .iter()
                    .find(|w| !w.worktree)
                    .or(g.workspaces.first())
            }
        }
    }

    /// What Enter opens: the workspace under the cursor. On a collapsed repo, the
    /// workspace that needs you if there is one, since its row can't be reached.
    fn ws_to_open(&self) -> Option<&WorkspaceView> {
        if let Some(Row::Repo { group }) = self.selected_row() {
            let g = &self.groups[group];
            if g.collapsed
                && let Some(w) = g.workspaces.iter().find(|w| self.needs_attention(w))
            {
                return Some(w);
            }
        }
        self.selected_ws()
    }

    /// The workspaces a row stands for: its own, or all of a collapsed repo's.
    fn row_workspaces(&self, row: Row) -> &[WorkspaceView] {
        match row {
            Row::Workspace { group, ws } => {
                std::slice::from_ref(&self.groups[group].workspaces[ws])
            }
            Row::Repo { group } if self.groups[group].collapsed => &self.groups[group].workspaces,
            Row::Repo { .. }
            | Row::Header { .. }
            | Row::Gap { .. }
            | Row::BoxEnd { .. }
            | Row::Agent { .. } => &[],
        }
    }

    /// Step through the rows that need you: agents waiting on you first, then ones
    /// that finished while you weren't looking, each top to bottom, then around again.
    /// Returns whether it moved, for `n` to open what it moved to.
    fn next_attention(&mut self) -> bool {
        let rows_where = |wanted: &dyn Fn(&WorkspaceView) -> bool| -> Vec<usize> {
            (0..self.rows.len())
                .filter(|&i| self.row_workspaces(self.rows[i]).iter().any(wanted))
                .collect()
        };
        let mut targets = rows_where(&|w| w.agent == AgentState::Awaiting);
        for i in rows_where(&|w| self.unseen.contains(&w.path)) {
            if !targets.contains(&i) {
                targets.push(i);
            }
        }
        if !targets.is_empty() {
            let cur = self.list.selected();
            let next = targets
                .iter()
                .position(|&i| Some(i) == cur)
                .map_or(0, |k| (k + 1) % targets.len());
            self.list.select(Some(targets[next]));
            return true;
        }
        let anywhere = self
            .groups
            .iter()
            .flat_map(|g| &g.workspaces)
            .any(|w| self.needs_attention(w));
        let message = if anywhere {
            "Something needs you, but the filter hides it (Esc clears)"
        } else {
            "Nothing needs you"
        };
        self.toast(Toast::info(message));
        false
    }

    /// Half the rows the list shows, for `Ctrl-d` / `Ctrl-u` and the `▲` / `▼` lines.
    fn half_page(&self) -> isize {
        isize::try_from(self.list_area.height / 2).unwrap_or(1).max(1)
    }

    fn move_by(&mut self, delta: isize) {
        if self.rows.is_empty() {
            return;
        }
        let cur = self.list.selected().unwrap_or(0) as isize;
        let last = self.rows.len() as isize - 1;
        let target = cur.saturating_add(delta).clamp(0, last) as usize;
        self.select_near(target, delta.signum());
    }

    // ---- input ---------------------------------------------------------

    /// Handle a key press. Returns a command for the caller to run.
    pub fn handle_key(&mut self, key: KeyEvent) -> Option<Command> {
        if key.kind == KeyEventKind::Release {
            return None;
        }
        match std::mem::replace(&mut self.mode, Mode::Normal) {
            Mode::Normal => self.normal_key(key),
            Mode::Help(scroll) => {
                match key.code {
                    KeyCode::Char('j') | KeyCode::Down => {
                        self.mode = Mode::Help(scroll.saturating_add(1))
                    }
                    KeyCode::Char('k') | KeyCode::Up => {
                        self.mode = Mode::Help(scroll.saturating_sub(1))
                    }
                    _ => {}
                }
                None
            }
            Mode::Filter(input) => self.filter_key(key, input),
            Mode::Prompt { kind, input } => self.prompt_key(key, kind, input),
            Mode::Picker { ws, choices, index } => self.picker_key(key, ws, choices, index),
            Mode::Confirm { message, command } => match key.code {
                KeyCode::Char('y' | 'Y') | KeyCode::Enter => Some(*command),
                KeyCode::Char('n' | 'N') | KeyCode::Esc => None,
                _ => {
                    self.mode = Mode::Confirm { message, command };
                    None
                }
            },
        }
    }

    fn normal_key(&mut self, key: KeyEvent) -> Option<Command> {
        if key.code == KeyCode::Esc && !self.filter.is_empty() {
            self.set_filter(String::new());
            return None;
        }
        let action = action_for(&key)?;
        match action {
            Action::Down => self.move_by(1),
            Action::Up => self.move_by(-1),
            Action::Top => self.move_by(isize::MIN / 2),
            Action::Bottom => self.move_by(isize::MAX / 2),
            Action::HalfPageDown => self.move_by(self.half_page()),
            Action::HalfPageUp => self.move_by(-self.half_page()),
            Action::Open => return self.ws_to_open().cloned().map(Command::Open),
            Action::NextAttention => {
                if self.next_attention() {
                    return self.ws_to_open().cloned().map(Command::Open);
                }
            }
            Action::Peek => {
                let ws = self.ws_to_open()?;
                if ws.status != WsStatus::Live {
                    self.toast(Toast::info(format!("{} is not running", ws.name)));
                    return None;
                }
                return Some(Command::Peek(peek_target(ws)));
            }
            Action::ToggleCollapse => {
                return self
                    .selected_group()
                    .map(|g| Command::ToggleCollapse(g.path.clone()));
            }
            Action::Filter => self.mode = Mode::Filter(InputState::new(self.filter.clone())),
            Action::NewWorktree => {
                if let Some(g) = self.selected_group() {
                    self.mode = Mode::Prompt {
                        kind: PromptKind::NewWorktree {
                            repo: g.path.clone(),
                            repo_name: g.name.clone(),
                        },
                        input: InputState::default(),
                    };
                }
            }
            Action::NewTab => {
                let ws = self.selected_ws().cloned()?;
                let choices = (self.tab_choices)(&ws.repo);
                if !choices.is_empty() {
                    self.mode = Mode::Picker {
                        ws: Box::new(ws),
                        choices,
                        index: 0,
                    };
                }
            }
            // In the frame `a` browses folders in a popup; otherwise, type a path.
            Action::AddRepo if !self.standalone => return Some(Command::BrowseRepos),
            Action::AddRepo => {
                self.mode = Mode::Prompt {
                    kind: PromptKind::AddRepo,
                    input: InputState::default(),
                };
            }
            Action::TogglePin => {
                return self
                    .selected_group()
                    .map(|g| Command::TogglePin(g.path.clone()));
            }
            Action::PinUp | Action::PinDown => {
                let g = self.selected_group()?;
                if !g.pinned {
                    self.toast(Toast::info("Pin the repo first (p)"));
                    return None;
                }
                let delta = if action == Action::PinUp { -1 } else { 1 };
                return Some(Command::MovePin(g.path.clone(), delta));
            }
            Action::Archive => self.ask_archive(),
            Action::Park => {
                let ws = self.selected_ws()?.clone();
                if ws.status != WsStatus::Live {
                    self.toast(Toast::info(format!("{} is not running", ws.name)));
                    return None;
                }
                self.mode = Mode::Confirm {
                    message: format!(
                        "Park {}? It is saved and stopped; Enter brings it back.",
                        ws.name
                    ),
                    command: Box::new(Command::Park(ws)),
                };
            }
            Action::Save => return Some(Command::Save),
            Action::Refresh => return Some(Command::Refresh),
            // In the frame `?` opens the same popup as Alt-/; there's no popup to use otherwise.
            Action::Help if !self.standalone => return Some(Command::Cheatsheet),
            Action::Help => self.mode = Mode::Help(0),
            Action::Quit if self.standalone => return Some(Command::Quit),
            Action::Quit => return Some(Command::Detach),
        }
        None
    }

    fn ask_archive(&mut self) {
        let Some(row) = self.selected_row() else {
            return;
        };
        let (message, command) = match row {
            Row::Workspace { group, ws } if self.groups[group].workspaces[ws].worktree => {
                let w = self.groups[group].workspaces[ws].clone();
                (
                    format!(
                        "Archive {}? Removes the worktree at {} and kills its session.",
                        w.name,
                        w.path.display()
                    ),
                    Command::Archive(w),
                )
            }
            Row::Header { .. } | Row::Gap { .. } | Row::BoxEnd { .. } | Row::Agent { .. } => return,
            Row::Repo { group } | Row::Workspace { group, .. } => {
                let g = &self.groups[group];
                (
                    format!("Forget {}? Files on disk are not touched.", g.name),
                    Command::ForgetRepo(g.path.clone()),
                )
            }
        };
        self.mode = Mode::Confirm {
            message,
            command: Box::new(command),
        };
    }

    fn set_filter(&mut self, filter: String) {
        let key = self.selected_key();
        self.filter = filter;
        self.rebuild_rows(key);
    }

    fn filter_key(&mut self, key: KeyEvent, input: InputState) -> Option<Command> {
        match key.code {
            KeyCode::Esc => self.set_filter(String::new()),
            KeyCode::Enter => {}
            KeyCode::Down => {
                self.move_by(1);
                self.mode = Mode::Filter(input);
            }
            KeyCode::Up => {
                self.move_by(-1);
                self.mode = Mode::Filter(input);
            }
            _ => {
                let input = edit(input, key);
                self.set_filter(input.value().to_string());
                self.mode = Mode::Filter(input);
            }
        }
        None
    }

    fn prompt_key(
        &mut self,
        key: KeyEvent,
        kind: PromptKind,
        input: InputState,
    ) -> Option<Command> {
        match key.code {
            KeyCode::Esc => None,
            KeyCode::Enter => {
                let value = input.value().trim().to_string();
                if value.is_empty() {
                    return None;
                }
                Some(match kind {
                    PromptKind::NewWorktree { repo, .. } => Command::NewWorktree {
                        repo,
                        branch: value,
                    },
                    PromptKind::AddRepo => Command::AddRepo(value),
                })
            }
            _ => {
                self.mode = Mode::Prompt {
                    kind,
                    input: edit(input, key),
                };
                None
            }
        }
    }

    fn picker_key(
        &mut self,
        key: KeyEvent,
        ws: Box<WorkspaceView>,
        choices: Vec<TabSpec>,
        mut index: usize,
    ) -> Option<Command> {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => return None,
            KeyCode::Enter => {
                let tab = choices.get(index).cloned()?;
                return Some(Command::NewTab { ws: *ws, tab });
            }
            KeyCode::Char(c @ '1'..='9') => {
                let i = c as usize - '1' as usize;
                if let Some(tab) = choices.get(i).cloned() {
                    return Some(Command::NewTab { ws: *ws, tab });
                }
            }
            KeyCode::Down | KeyCode::Char('j') => index = (index + 1).min(choices.len() - 1),
            KeyCode::Up | KeyCode::Char('k') => index = index.saturating_sub(1),
            _ => {}
        }
        self.mode = Mode::Picker { ws, choices, index };
        None
    }

    /// Handle a mouse event inside the sidebar.
    pub fn handle_mouse(&mut self, ev: MouseEvent) -> Option<Command> {
        if !matches!(self.mode, Mode::Normal | Mode::Filter(_)) {
            return None;
        }
        match ev.kind {
            MouseEventKind::ScrollDown => self.move_by(1),
            MouseEventKind::ScrollUp => self.move_by(-1),
            MouseEventKind::Down(MouseButton::Left) => {
                // The `▲` / `▼` lines scroll half a page.
                if let Some((up, down)) = self.scroll_marks
                    && (ev.row == up || ev.row == down)
                {
                    let half = self.half_page();
                    self.move_by(if ev.row == up { -half } else { half });
                    return None;
                }
                let a = self.list_area;
                if ev.column < a.x
                    || ev.column >= a.x + a.width
                    || ev.row < a.y
                    || ev.row >= a.y + a.height
                {
                    return None;
                }
                let idx = self.list.offset() + usize::from(ev.row - a.y);
                let row = *self.rows.get(idx)?;
                if !self.can_select(idx) {
                    return None;
                }
                self.list.select(Some(idx));
                return match row {
                    Row::Header { .. }
                    | Row::Gap { .. }
                    | Row::BoxEnd { .. }
                    | Row::Agent { .. }
                    | Row::Repo { .. } => None,
                    Row::Workspace { .. } => self.selected_ws().cloned().map(Command::Open),
                };
            }
            _ => {}
        }
        None
    }

    #[must_use]
    pub fn mode(&self) -> &Mode {
        &self.mode
    }

    #[must_use]
    pub fn filter(&self) -> &str {
        &self.filter
    }
}

/// Which tab `v` shows: the agent waiting on you, else one at work, else any agent, else
/// whatever tab the workspace has open.
fn peek_target(ws: &WorkspaceView) -> String {
    let agents = || ws.tabs.iter().filter(|t| t.agent.is_some());
    agents()
        .find(|t| t.state == AgentState::Awaiting)
        .or_else(|| agents().find(|t| t.state == AgentState::Busy))
        .or_else(|| agents().next())
        .map_or_else(|| ws.name.clone(), |t| t.address.clone())
}

/// Apply one key to a text field using ratcn's editor bindings.
pub(crate) fn edit(input: InputState, key: KeyEvent) -> InputState {
    let Ok(k) = ratcn::runtime::KeyEvent::try_from(key) else {
        return input;
    };
    let Some(editor_input) = ratcn::text_edit::editor_input(&k) else {
        return input;
    };
    let mut editor = input.editor().clone();
    editor.input(editor_input);
    InputState::from_editor(editor)
}
