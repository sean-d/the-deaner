//! The workspace engine: creates, lists, snapshots and restores deaner's tmux
//! sessions. Every CLI command and sidebar action ends up here.

use crate::paths::{Paths, expand, write_atomic};
use anyhow::{Context, Result, anyhow, bail};
use deaner_core::agent::AgentState;
use deaner_core::config::{Config, RepoConfig, TabKind, TabSpec};
use deaner_core::names::{session_name, unique};
use deaner_core::pane::{
    OPT_AGENT, OPT_AGENT_SESSION, OPT_BRANCH, OPT_LAUNCH, OPT_PATH, OPT_REPO, OPT_WORKTREE,
    PaneInfo,
};
use deaner_core::sidebar::{self, RepoGroup, TabView, WorkspaceView, WsStatus};
use deaner_core::snapshot::{Snapshot, WorkspaceSnap, restore_command};
use deaner_core::state::{RepoEntry, State};
use deaner_tmux::{Scope, Tmux, window_target};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// How long to wait for a new pane's shell to draw its prompt.
const PROMPT_WAIT: Duration = Duration::from_secs(3);

/// tmux socket for workspaces (`tmux -L deaner`). `DEANER_SOCKET` overrides.
#[must_use]
pub fn work_socket() -> String {
    std::env::var("DEANER_SOCKET").unwrap_or_else(|_| "deaner".into())
}

/// Serializes `state.toml` updates within this process.
static STATE_LOCK: Mutex<()> = Mutex::new(());

#[must_use]
pub fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

#[derive(Debug, Clone)]
pub struct Engine {
    pub tmux: Tmux,
    pub config: Config,
    pub paths: Paths,
}

/// Outcome of a restore.
#[derive(Debug, Default)]
pub struct RestoreReport {
    pub restored: Vec<String>,
    /// `(name, reason)`
    pub skipped: Vec<(String, String)>,
}

impl Engine {
    /// Engine for the real environment: config from disk, workspaces on the `deaner` socket.
    ///
    /// # Errors
    /// When the config file exists but is invalid.
    pub fn from_env() -> Result<Self> {
        let paths = Paths::from_env();
        let config = load_config(&paths)?;
        Ok(Self {
            tmux: Tmux::named(work_socket()),
            config,
            paths,
        })
    }

    #[must_use]
    pub fn new(tmux: Tmux, config: Config, paths: Paths) -> Self {
        Self {
            tmux,
            config,
            paths,
        }
    }

    // ---- state ---------------------------------------------------------

    #[must_use]
    pub fn state(&self) -> State {
        std::fs::read_to_string(self.paths.state_file())
            .ok()
            .and_then(|t| State::from_toml(&t).ok())
            .unwrap_or_default()
    }

    /// Load, change, save.
    ///
    /// # Errors
    /// When the state file cannot be written.
    pub fn update_state<R>(&self, f: impl FnOnce(&mut State) -> R) -> Result<R> {
        // Commands run on several threads; serialize read-modify-write so none is lost.
        let _guard = STATE_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut state = self.state();
        let out = f(&mut state);
        write_atomic(&self.paths.state_file(), &state.to_toml())
            .with_context(|| format!("writing {}", self.paths.state_file().display()))?;
        Ok(out)
    }

    fn repo_config(&self, repo: &Path) -> Option<RepoConfig> {
        let text = std::fs::read_to_string(repo.join(".deaner.toml")).ok()?;
        RepoConfig::from_toml(&text).ok()
    }

    /// Choices for "new tab" in `repo`.
    #[must_use]
    pub fn tab_choices(&self, repo: &Path) -> Vec<TabSpec> {
        self.config.new_tab_choices(self.repo_config(repo).as_ref())
    }

    // ---- repos ---------------------------------------------------------

    /// Register a repo (or plain folder). A path inside a worktree registers its main checkout.
    ///
    /// # Errors
    /// When the path does not exist.
    pub fn add_repo(&self, raw: &str) -> Result<RepoEntry> {
        let path = expand(raw);
        if !path.is_dir() {
            bail!("{} is not a directory", path.display());
        }
        let root = if deaner_git::is_repo(&path) {
            deaner_git::main_root(&path)?.canonicalize().unwrap_or(path)
        } else {
            path
        };
        let name = root.file_name().map_or_else(
            || root.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        );
        self.update_state(|s| s.add_repo(root, name).clone())
    }

    /// Forget a repo. Its sessions keep running; files are untouched.
    ///
    /// # Errors
    /// When state cannot be written.
    pub fn forget_repo(&self, path: &Path) -> Result<bool> {
        self.update_state(|s| s.remove_repo(path))
    }

    // ---- listing -------------------------------------------------------

    /// Panes in deaner-managed sessions.
    #[must_use]
    pub fn managed_panes(&self) -> Vec<PaneInfo> {
        self.tmux
            .panes()
            .into_iter()
            .filter(|p| p.workspace_path.is_some())
            .collect()
    }

    /// Every workspace: known repos and their worktrees, live sessions, parked ones.
    /// Git and PR fields are left empty (the caller fills them on its own schedule).
    #[must_use]
    pub fn workspaces(&self) -> Vec<WorkspaceView> {
        self.workspaces_with(&checkouts)
    }

    /// Like [`Engine::workspaces`], with the worktree list of each repo supplied by
    /// the caller (the sidebar passes cached lists so a slow repo can't stall it).
    #[must_use]
    pub fn workspaces_with(
        &self,
        checkouts: &dyn Fn(&RepoEntry) -> Vec<Checkout>,
    ) -> Vec<WorkspaceView> {
        let state = self.state();
        let panes = self.managed_panes();
        let parked = self.parked();
        let attached: Vec<String> = self.tmux.clients().into_iter().map(|(_, s)| s).collect();

        let mut views: Vec<WorkspaceView> = Vec::new();
        let push = |views: &mut Vec<WorkspaceView>, mut v: WorkspaceView| {
            if !views.iter().any(|x| x.path == v.path) {
                fill_live(&mut v, &panes, &attached);
                views.push(v);
            }
        };

        // A deleted folder isn't a workspace (a session still running in one is added below).
        for repo in state.repos.iter().filter(|r| r.path.is_dir()) {
            for (path, branch, worktree) in checkouts(repo) {
                let name = session_name(
                    &repo.name,
                    worktree.then_some(branch.as_deref().unwrap_or("detached")),
                );
                let status = if parked.iter().any(|p| p.path == path) {
                    WsStatus::Parked
                } else {
                    WsStatus::Stopped
                };
                push(
                    &mut views,
                    WorkspaceView {
                        repo: repo.path.clone(),
                        path,
                        name,
                        branch,
                        worktree,
                        status,
                        agent: AgentState::None,
                        git: None,
                        pr: None,
                        checks: deaner_core::Checks::None,
                        tabs: Vec::new(),
                        current: false,
                    },
                );
            }
        }
        // Live sessions outside known repos.
        for p in &panes {
            let path = p.workspace_path.clone().expect("managed");
            push(
                &mut views,
                WorkspaceView {
                    repo: p.repo.clone().unwrap_or_else(|| path.clone()),
                    path,
                    name: p.session.clone(),
                    branch: p.branch.clone(),
                    worktree: p.worktree,
                    status: WsStatus::Stopped,
                    agent: AgentState::None,
                    git: None,
                    pr: None,
                    checks: deaner_core::Checks::None,
                    tabs: Vec::new(),
                    current: false,
                },
            );
        }
        // Parked workspaces of forgotten repos.
        for ws in parked {
            push(
                &mut views,
                WorkspaceView {
                    repo: ws.repo.clone(),
                    path: ws.path.clone(),
                    name: ws.name.clone(),
                    branch: ws.branch.clone(),
                    worktree: ws.worktree,
                    status: WsStatus::Parked,
                    agent: AgentState::None,
                    git: None,
                    pr: None,
                    checks: deaner_core::Checks::None,
                    tabs: Vec::new(),
                    current: false,
                },
            );
        }
        views
    }

    /// Group and order workspaces for the sidebar.
    #[must_use]
    pub fn groups(&self, views: Vec<WorkspaceView>) -> Vec<RepoGroup> {
        let state = self.state();
        // Repos whose folder is gone drop out of the sidebar. They stay in the state file,
        // so a project on an unplugged drive, with its pin, comes back with the drive.
        let repos: Vec<RepoEntry> = state
            .repos
            .iter()
            .filter(|r| r.path.is_dir() || views.iter().any(|w| w.repo == r.path))
            .cloned()
            .collect();
        sidebar::build(
            &sidebar::Layout {
                repos: &repos,
                pins: &state.pins,
                collapsed: &state.collapsed,
                float_attention: self.config.float_attention,
            },
            views,
        )
    }

    /// Find a workspace by session name, path, or repo name (its main checkout).
    ///
    /// # Errors
    /// When nothing matches.
    pub fn find(&self, target: &str) -> Result<WorkspaceView> {
        let views = self.workspaces();
        let path = expand(target);
        views
            .iter()
            .find(|w| w.name == target)
            .or_else(|| views.iter().find(|w| w.path == path))
            .or_else(|| {
                views.iter().find(|w| {
                    !w.worktree
                        && w.repo
                            .file_name()
                            .is_some_and(|n| n.to_string_lossy() == target)
                })
            })
            .cloned()
            .ok_or_else(|| anyhow!("no workspace matches `{target}` (see `deaner ls`)"))
    }

    // ---- sessions ------------------------------------------------------

    /// Make sure `ws` has a running session (creating or unparking it) and return its name.
    ///
    /// # Errors
    /// When tmux or git fail.
    pub fn ensure_session(&self, ws: &WorkspaceView) -> Result<String> {
        // tmux would quietly start somewhere else, and agents would run in the wrong place.
        if ws.status != WsStatus::Live && !ws.path.is_dir() {
            bail!("{} no longer exists", ws.path.display());
        }
        match ws.status {
            WsStatus::Live => Ok(ws.name.clone()),
            WsStatus::Parked => self.unpark(&ws.path),
            WsStatus::Stopped => {
                let state = self.state();
                let repo = state.repo(&ws.repo).cloned().unwrap_or_else(|| RepoEntry {
                    path: ws.repo.clone(),
                    name: ws
                        .repo
                        .file_name()
                        .map_or_else(|| "repo".into(), |n| n.to_string_lossy().into_owned()),
                });
                self.create_session(&repo, &ws.path, ws.branch.as_deref(), ws.worktree)
            }
        }
    }

    /// Create a session for a checkout with the configured starting tabs.
    ///
    /// # Errors
    /// When tmux fails.
    pub fn create_session(
        &self,
        repo: &RepoEntry,
        path: &Path,
        branch: Option<&str>,
        worktree: bool,
    ) -> Result<String> {
        let base = session_name(&repo.name, worktree.then_some(branch.unwrap_or("detached")));
        let name = unique(&base, &self.tmux.sessions());
        let tabs = self.config.tabs_for(self.repo_config(&repo.path).as_ref());
        let (first, rest) = tabs.split_first().map_or_else(
            || (TabSpec::new("shell", None), Vec::new()),
            |(f, r)| (f.clone(), r.to_vec()),
        );

        let created = self.tmux.new_session(&name, path, &first.name)?;
        self.tmux_server_defaults();
        self.mark_session(&name, repo, path, branch, worktree)?;
        let mut typed = Vec::new();
        typed.extend(self.launch(&created.pane_id, first.command.as_deref())?);
        for tab in rest {
            let c = self.tmux.new_window(&name, path, &tab.name)?;
            typed.extend(self.launch(&c.pane_id, tab.command.as_deref())?);
        }
        self.type_when_ready(&typed)?;
        let _ = self
            .tmux
            .select_window(&window_target(&name, created.window_index));
        Ok(name)
    }

    fn mark_session(
        &self,
        name: &str,
        repo: &RepoEntry,
        path: &Path,
        branch: Option<&str>,
        worktree: bool,
    ) -> Result<()> {
        let s = Scope::Session(name);
        self.tmux.set_option(s, OPT_PATH, &path.to_string_lossy())?;
        self.tmux
            .set_option(s, OPT_REPO, &repo.path.to_string_lossy())?;
        if let Some(b) = branch {
            self.tmux.set_option(s, OPT_BRANCH, b)?;
        }
        self.tmux
            .set_option(s, OPT_WORKTREE, if worktree { "1" } else { "0" })?;
        Ok(())
    }

    /// Server options deaner relies on for its own socket.
    fn tmux_server_defaults(&self) {
        // Killing the shown session moves the client to another one instead of detaching.
        let _ = self
            .tmux
            .set_option(Scope::Server, "detach-on-destroy", "off");
        // deaner snapshots its own server; tmux-continuum autosaving it too would write
        // deaner sessions into the resurrect file shared with the user's normal tmux.
        let _ = self
            .tmux
            .set_option(Scope::Server, "@continuum-save-interval", "0");
    }

    /// Record a pane's launch command, and return what [`Self::type_when_ready`] should type.
    fn launch(&self, pane: &str, command: Option<&str>) -> Result<Option<(String, String)>> {
        let Some(cmd) = command.filter(|c| !c.trim().is_empty()) else {
            return Ok(None);
        };
        let p = Scope::Pane(pane);
        self.tmux.set_option(p, OPT_LAUNCH, cmd)?;
        if let TabKind::Agent(agent) = self.config.classify(Some(cmd)) {
            self.tmux.set_option(p, OPT_AGENT, &agent)?;
        }
        Ok(Some((pane.to_string(), cmd.to_string())))
    }

    /// Type each `(pane, command)` once its shell has drawn a prompt, all at the same time.
    ///
    /// Typing sooner works, but the terminal echoes the text before the shell is ready and
    /// then the shell shows it again, leaving a stray copy above the prompt.
    fn type_when_ready(&self, commands: &[(String, String)]) -> Result<()> {
        std::thread::scope(|s| {
            let jobs: Vec<_> = commands
                .iter()
                .map(|(pane, cmd)| {
                    s.spawn(move || {
                        self.wait_for_prompt(pane);
                        self.tmux.send_text(pane, cmd, true)
                    })
                })
                .collect();
            for job in jobs {
                job.join().expect("typing into a pane doesn't panic")?;
            }
            Ok(())
        })
    }

    /// Wait until a pane shows something and has stopped changing. A shell that never
    /// draws a prompt just gets typed into after the deadline.
    fn wait_for_prompt(&self, pane: &str) {
        let deadline = Instant::now() + PROMPT_WAIT;
        let mut last = String::new();
        while Instant::now() < deadline {
            let screen = self.tmux.capture_pane(pane).unwrap_or_default();
            if !screen.trim().is_empty() && screen == last {
                return;
            }
            last = screen;
            std::thread::sleep(Duration::from_millis(60));
        }
    }

    /// Add a tab to a running session and show it.
    ///
    /// # Errors
    /// When the session does not exist.
    pub fn new_tab(&self, session: &str, cwd: &Path, tab: &TabSpec) -> Result<()> {
        let taken: Vec<String> = self
            .managed_panes()
            .into_iter()
            .filter(|p| p.session == session)
            .map(|p| p.window_name)
            .collect();
        let name = unique(&tab.name, &taken);
        let c = self.tmux.new_window(session, cwd, &name)?;
        let typed: Vec<_> = self
            .launch(&c.pane_id, tab.command.as_deref())?
            .into_iter()
            .collect();
        self.type_when_ready(&typed)?;
        let _ = self
            .tmux
            .select_window(&window_target(session, c.window_index));
        Ok(())
    }

    /// Create a worktree for `branch` and a session for it.
    ///
    /// # Errors
    /// When git refuses or the repo is unknown and not a git repo.
    pub fn new_worktree(&self, repo_path: &Path, branch: &str) -> Result<String> {
        let branch = branch.trim();
        if branch.is_empty() || branch.contains(char::is_whitespace) {
            bail!("`{branch}` is not a valid branch name");
        }
        let state = self.state();
        let repo = match state.repo(repo_path) {
            Some(r) => r.clone(),
            None => self.add_repo(&repo_path.to_string_lossy())?,
        };
        if !deaner_git::is_repo(&repo.path) {
            bail!("{} is not a git repository", repo.path.display());
        }
        let path = self.config.worktree_path(&repo.path, branch);
        deaner_git::add_worktree(&repo.path, &path, branch)?;
        self.create_session(&repo, &path, Some(branch), true)
    }

    /// Kill the session of a worktree and remove the worktree.
    ///
    /// # Errors
    /// When git refuses (uncommitted changes without `force`).
    pub fn archive(&self, ws: &WorkspaceView, force: bool) -> Result<()> {
        if !ws.worktree {
            bail!(
                "{} is a main checkout; use `deaner forget` to drop the repo",
                ws.name
            );
        }
        if !force
            && let Ok(s) = deaner_git::status(&ws.path)
            && !s.is_clean()
        {
            bail!(
                "{} has {} uncommitted change(s); commit them or archive with --force",
                ws.name,
                s.changed()
            );
        }
        if let Some(session) = self.session_for(&ws.path) {
            self.tmux.kill_session(&session)?;
        }
        let _ = std::fs::remove_file(self.parked_file(&ws.name));
        deaner_git::remove_worktree(&ws.repo, &ws.path, force)?;
        Ok(())
    }

    /// Name of the live session for a workspace path.
    #[must_use]
    pub fn session_for(&self, path: &Path) -> Option<String> {
        self.managed_panes()
            .into_iter()
            .find(|p| p.workspace_path.as_deref() == Some(path))
            .map(|p| p.session)
    }

    // ---- snapshots -----------------------------------------------------

    #[must_use]
    pub fn snapshot(&self) -> Snapshot {
        let shell = self
            .tmux
            .show_option(Scope::Server, "default-shell")
            .map(|s| deaner_core::config::program_name(&s).to_string());
        Snapshot::from_panes(&self.tmux.panes(), now_unix(), shell.as_deref())
    }

    /// Write a snapshot as `latest.toml` plus a timestamped copy, pruning old copies.
    ///
    /// # Errors
    /// On IO failure.
    pub fn save_snapshot(&self, snap: &Snapshot) -> Result<PathBuf> {
        let dir = self.paths.snapshots_dir();
        let text = snap.to_toml();
        let stamped = dir.join(format!("snapshot-{}.toml", snap.created));
        write_atomic(&stamped, &text)?;
        write_atomic(&self.paths.latest_snapshot(), &text)?;
        let mut old: Vec<PathBuf> = std::fs::read_dir(&dir)?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| {
                p.file_name()
                    .is_some_and(|n| n.to_string_lossy().starts_with("snapshot-"))
            })
            .collect();
        old.sort();
        let excess = old.len().saturating_sub(self.config.snapshots_keep.max(1));
        for p in old.into_iter().take(excess) {
            let _ = std::fs::remove_file(p);
        }
        Ok(self.paths.latest_snapshot())
    }

    /// Snapshot everything, unless nothing is running (never overwrite a good
    /// snapshot with an empty one after a crash or reboot).
    ///
    /// # Errors
    /// On IO failure.
    pub fn autosave(&self) -> Result<Option<PathBuf>> {
        let snap = self.snapshot();
        if snap.workspaces.is_empty() {
            return Ok(None);
        }
        self.save_snapshot(&snap).map(Some)
    }

    #[must_use]
    pub fn latest_snapshot(&self) -> Option<Snapshot> {
        load_snapshot(&self.paths.latest_snapshot())
    }

    /// Rebuild every workspace in `snap` that is not already running.
    ///
    /// # Errors
    /// Only for unexpected failures; per-workspace problems land in the report.
    pub fn restore(&self, snap: &Snapshot) -> Result<RestoreReport> {
        let mut report = RestoreReport::default();
        let live: Vec<PathBuf> = self
            .managed_panes()
            .into_iter()
            .filter_map(|p| p.workspace_path)
            .collect();
        for ws in &snap.workspaces {
            if live.contains(&ws.path) {
                report
                    .skipped
                    .push((ws.name.clone(), "already running".into()));
            } else if !ws.path.is_dir() {
                report.skipped.push((
                    ws.name.clone(),
                    format!("{} no longer exists", ws.path.display()),
                ));
            } else {
                match self.restore_workspace(ws) {
                    Ok(name) => report.restored.push(name),
                    Err(e) => report.skipped.push((ws.name.clone(), format!("{e:#}"))),
                }
            }
        }
        Ok(report)
    }

    /// Rebuild one workspace: windows, splits, layouts, and what each pane was running.
    ///
    /// # Errors
    /// When tmux fails.
    pub fn restore_workspace(&self, ws: &WorkspaceSnap) -> Result<String> {
        let name = unique(&ws.name, &self.tmux.sessions());
        let state = self.state();
        let repo = state.repo(&ws.repo).cloned().unwrap_or_else(|| RepoEntry {
            path: ws.repo.clone(),
            name: ws.name.split('/').next().unwrap_or(&ws.name).to_string(),
        });
        let mut windows = ws.windows.clone();
        if windows.is_empty() {
            return self.create_session(&repo, &ws.path, ws.branch.as_deref(), ws.worktree);
        }
        windows.sort_by_key(|w| w.index);

        let mut active_window = None;
        let mut active_pane = None;
        let mut typed = Vec::new();
        for (wi, win) in windows.iter().enumerate() {
            let cwd = |i: usize| {
                win.panes
                    .get(i)
                    .map(|p| p.cwd.clone())
                    .filter(|c| c.is_dir())
                    .unwrap_or_else(|| ws.path.clone())
            };
            let first = if wi == 0 {
                let c = self.tmux.new_session(&name, &cwd(0), &win.name)?;
                self.tmux_server_defaults();
                self.mark_session(&name, &repo, &ws.path, ws.branch.as_deref(), ws.worktree)?;
                c
            } else {
                self.tmux.new_window(&name, &cwd(0), &win.name)?
            };
            let mut pane_ids = vec![first.pane_id.clone()];
            for i in 1..win.panes.len() {
                let c = self
                    .tmux
                    .split(pane_ids.last().expect("non-empty"), &cwd(i))?;
                pane_ids.push(c.pane_id);
            }
            let target = window_target(&name, first.window_index);
            if win.panes.len() > 1 {
                let _ = self.tmux.select_layout(&target, &win.layout);
            }
            for (pane, id) in win.panes.iter().zip(&pane_ids) {
                let scope = Scope::Pane(id);
                if let Some(agent) = &pane.agent {
                    self.tmux.set_option(scope, OPT_AGENT, agent)?;
                }
                if let (Some(agent), Some(session)) = (&pane.agent, &pane.agent_session)
                    && crate::hooks::conversation_exists(agent, session)
                {
                    self.tmux.set_option(scope, OPT_AGENT_SESSION, session)?;
                }
                if let Some(launch) = &pane.launch {
                    self.tmux.set_option(scope, OPT_LAUNCH, launch)?;
                }
                if let Some(cmd) =
                    restore_command(pane, &self.config, &crate::hooks::conversation_exists)
                {
                    typed.push((id.clone(), cmd));
                }
                if win.active && pane.active {
                    active_pane = Some(id.clone());
                }
            }
            if win.active {
                active_window = Some(target);
            }
        }
        self.type_when_ready(&typed)?;
        if let Some(w) = active_window {
            let _ = self.tmux.select_window(&w);
        }
        if let Some(p) = active_pane {
            let _ = self.tmux.select_pane(&p);
        }
        Ok(name)
    }

    // ---- park ----------------------------------------------------------

    fn parked_file(&self, name: &str) -> PathBuf {
        let safe: String = name
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || "-_.".contains(c) {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        self.paths.parked_dir().join(format!("{safe}.toml"))
    }

    /// Parked workspaces.
    #[must_use]
    pub fn parked(&self) -> Vec<WorkspaceSnap> {
        let Ok(entries) = std::fs::read_dir(self.paths.parked_dir()) else {
            return Vec::new();
        };
        let mut out: Vec<WorkspaceSnap> = entries
            .filter_map(|e| e.ok())
            .filter_map(|e| load_snapshot(&e.path()))
            .flat_map(|s| s.workspaces)
            .collect();
        out.sort_by(|a, b| a.name.cmp(&b.name));
        out
    }

    /// Save one session and stop it.
    ///
    /// # Errors
    /// When the session is not running or the file cannot be written.
    pub fn park(&self, session: &str) -> Result<()> {
        let mut snap = self.snapshot();
        snap.workspaces.retain(|w| w.name == session);
        if snap.workspaces.is_empty() {
            bail!("no running workspace named `{session}`");
        }
        write_atomic(&self.parked_file(session), &snap.to_toml())?;
        self.tmux.kill_session(session)?;
        Ok(())
    }

    /// Bring a parked workspace back.
    ///
    /// # Errors
    /// When nothing is parked at `path` or the restore fails.
    pub fn unpark(&self, path: &Path) -> Result<String> {
        let entries = std::fs::read_dir(self.paths.parked_dir()).context("nothing is parked")?;
        for entry in entries.filter_map(|e| e.ok()) {
            let Some(snap) = load_snapshot(&entry.path()) else {
                continue;
            };
            if let Some(ws) = snap.workspaces.iter().find(|w| w.path == path) {
                let name = self.restore_workspace(ws)?;
                std::fs::remove_file(entry.path())?;
                return Ok(name);
            }
        }
        bail!("nothing parked for {}", path.display())
    }

    // ---- agents --------------------------------------------------------

    /// Panes whose agent is mid-turn or blocked on the user.
    #[must_use]
    pub fn active_agents(&self) -> Vec<PaneInfo> {
        self.managed_panes()
            .into_iter()
            .filter(|p| {
                matches!(
                    p.state,
                    AgentState::Busy | AgentState::Compacting | AgentState::Awaiting
                )
            })
            .collect()
    }

    /// Resolve `<session>[:<tab>]` to a pane id (the tab's active pane).
    ///
    /// # Errors
    /// When the address does not match.
    pub fn resolve(&self, addr: &str) -> Result<PaneInfo> {
        let (session, tab) = match addr.rsplit_once(':') {
            Some((s, t)) => (s, Some(t)),
            None => (addr, None),
        };
        let panes: Vec<PaneInfo> = self
            .tmux
            .panes()
            .into_iter()
            .filter(|p| p.session == session)
            .collect();
        if panes.is_empty() {
            bail!("no session `{session}` (see `deaner ls`)");
        }
        let in_tab = |p: &PaneInfo| match tab {
            None => p.window_active,
            Some(t) => p.window_name == t || p.window_index.to_string() == t,
        };
        panes
            .iter()
            .filter(|p| in_tab(p))
            .max_by_key(|p| p.pane_active)
            .cloned()
            .ok_or_else(|| anyhow!("no tab `{}` in `{session}`", tab.unwrap_or_default()))
    }

    /// Type a message into another agent's pane and submit it.
    ///
    /// # Errors
    /// When the address does not resolve.
    pub fn send(&self, addr: &str, text: &str, enter: bool) -> Result<String> {
        let pane = self.resolve(addr)?;
        self.tmux.send_text(&pane.pane_id, text, false)?;
        if enter {
            // Let the agent's input settle so Enter is not read as part of a paste.
            std::thread::sleep(Duration::from_millis(150));
            self.tmux.send_keys(&pane.pane_id, &["Enter"])?;
        }
        Ok(pane.address())
    }
}

/// Load `config.toml`, or defaults if it does not exist.
///
/// # Errors
/// When the file exists but is not valid.
pub fn load_config(paths: &Paths) -> Result<Config> {
    match std::fs::read_to_string(paths.config_file()) {
        Ok(text) => Config::from_toml(&text)
            .with_context(|| format!("invalid config {}", paths.config_file().display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Config::default()),
        Err(e) => Err(e).with_context(|| format!("reading {}", paths.config_file().display())),
    }
}

fn load_snapshot(path: &Path) -> Option<Snapshot> {
    Snapshot::from_toml(&std::fs::read_to_string(path).ok()?).ok()
}

/// Whether this is a checkout an agent made for itself. Claude Code puts sub-agents in
/// `.claude/worktrees/`, and leaves them there once they hold commits. They aren't projects
/// anyone chose to open, and they stay around after the work is done, so they aren't listed.
/// A session started in one still is: anything running stays visible.
#[must_use]
pub fn is_agent_worktree(path: &Path) -> bool {
    path.components()
        .map(|c| c.as_os_str())
        .collect::<Vec<_>>()
        .windows(2)
        .any(|pair| pair[0] == ".claude" && pair[1] == "worktrees")
}

/// A checkout of a repo: `(path, branch, is_worktree)`.
pub type Checkout = (PathBuf, Option<String>, bool);

/// Checkouts of a repo, main first. Plain folders (and repos git can't read) yield just themselves.
#[must_use]
pub fn checkouts(repo: &RepoEntry) -> Vec<Checkout> {
    match deaner_git::worktrees(&repo.path) {
        Ok(list) if !list.is_empty() => list
            .into_iter()
            .filter(|w| !w.bare && !w.prunable && !is_agent_worktree(&w.path))
            .map(|w| {
                let main = same_path(&w.path, &repo.path);
                (
                    if main { repo.path.clone() } else { w.path },
                    w.branch,
                    !main,
                )
            })
            .collect(),
        _ => vec![(repo.path.clone(), None, false)],
    }
}

fn same_path(a: &Path, b: &Path) -> bool {
    a == b
        || a.canonicalize()
            .ok()
            .zip(b.canonicalize().ok())
            .is_some_and(|(x, y)| x == y)
}

/// Fill live-session fields from panes.
fn fill_live(v: &mut WorkspaceView, panes: &[PaneInfo], attached: &[String]) {
    let mine: Vec<&PaneInfo> = panes
        .iter()
        .filter(|p| {
            p.workspace_path
                .as_deref()
                .is_some_and(|wp| same_path(wp, &v.path))
        })
        .collect();
    let Some(first) = mine.first() else { return };
    v.status = WsStatus::Live;
    v.name = first.session.clone();
    if v.branch.is_none() {
        v.branch = first.branch.clone();
    }
    v.current = attached.contains(&first.session);
    v.agent = AgentState::rollup(mine.iter().map(|p| p.state));
    let mut tabs: Vec<TabView> = Vec::new();
    for p in &mine {
        match tabs.iter_mut().find(|t| t.index == p.window_index) {
            Some(t) => {
                if p.state.urgency() > t.state.urgency() {
                    t.state = p.state;
                }
                if t.agent.is_none() {
                    t.agent = p.agent.clone();
                }
            }
            None => tabs.push(TabView {
                index: p.window_index,
                name: p.window_name.clone(),
                state: p.state,
                agent: p.agent.clone(),
                address: p.address(),
            }),
        }
    }
    tabs.sort_by_key(|t| t.index);
    v.tabs = tabs;
}
