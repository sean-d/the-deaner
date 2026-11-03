//! Runs the sidebar TUI.
//!
//! Nothing slow ever runs on the drawing thread, and no single slow thing holds
//! up anything else:
//!
//! - **Refresher**: reads tmux once a second (or when kicked) and builds the
//!   sidebar from cached data. Git status, worktree lists and PR status are
//!   fetched by [`Poller`]s, one job per repo with several in parallel, so a huge
//!   or hung repo only delays its own badge.
//! - **Executor**: quick state changes (pins, collapse, add/forget) run in order
//!   on one lane; anything that can take a while (open, new worktree, archive,
//!   park, restore) runs on its own thread, at most one per workspace.

use crate::engine::{Checkout, Engine, checkouts, now_unix};
use crate::frame::{Switcher, in_frame};
use crate::poller::Poller;
use anyhow::Result;
use deaner_core::config::Config;
use deaner_core::sidebar::{RepoGroup, WsStatus};
use deaner_core::{BranchStatus, GitStatus};
use deaner_ui::{App, Command, Toast};
use ratatui::crossterm::event::{self, DisableMouseCapture, EnableMouseCapture, Event};
use ratatui::crossterm::execute;
use std::collections::HashSet;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// What a finished command wants to tell the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Ok(String),
    Info(String),
    Err(String),
    Silent,
}

impl Outcome {
    fn toast(self) -> Option<Toast<'static>> {
        match self {
            Self::Ok(t) => Some(Toast::success(t)),
            Self::Info(t) => Some(Toast::info(t)),
            Self::Err(t) => Some(Toast::error(t).duration(Duration::from_secs(8))),
            Self::Silent => None,
        }
    }
}

#[derive(Debug)]
enum Msg {
    Groups(Vec<RepoGroup>),
    Progress { id: String, text: String },
    Done { id: String, outcome: Outcome },
}

/// Run the sidebar until quit (standalone) or forever (inside the frame).
///
/// # Errors
/// When the terminal cannot be set up.
pub fn run(engine: Engine, standalone: bool) -> Result<()> {
    let (msg_tx, msg_rx) = mpsc::channel::<Msg>();
    let (kick_tx, kick_rx) = mpsc::channel::<()>();

    spawn_refresher(engine.clone(), msg_tx.clone(), kick_tx.clone(), kick_rx);
    let switcher = Arc::new(Switcher::detect(&engine));
    let runner_engine = engine.clone();
    let executor = Executor::new(
        Arc::new(move |c| execute(&runner_engine, &switcher, c)),
        engine.config.clone(),
        msg_tx,
        kick_tx.clone(),
    );

    let mut app = App::new();
    app.standalone = standalone || !in_frame();
    app.icons = deaner_ui::icons::for_style(engine.config.icons);
    let choices_engine = engine.clone();
    app.tab_choices = Box::new(move |repo| choices_engine.tab_choices(repo));
    offer_restore(&engine, &mut app);

    let mut terminal = ratatui::init();
    let _ = execute!(std::io::stdout(), EnableMouseCapture);
    let result = event_loop(
        &engine,
        &mut terminal,
        &mut app,
        &msg_rx,
        &executor,
        &kick_tx,
    );
    let _ = execute!(std::io::stdout(), DisableMouseCapture);
    ratatui::restore();
    result
}

fn offer_restore(engine: &Engine, app: &mut App) {
    if !engine.managed_panes().is_empty() {
        return;
    }
    let Some(snap) = engine.latest_snapshot() else {
        return;
    };
    if snap.workspaces.is_empty() {
        return;
    }
    app.confirm(
        format!(
            "Restore {} workspace(s) saved {}?",
            snap.workspaces.len(),
            ago(now_unix().saturating_sub(snap.created))
        ),
        Command::Restore,
    );
}

fn ago(secs: u64) -> String {
    match secs {
        0..60 => "just now".into(),
        60..3600 => format!("{} min ago", secs / 60),
        3600..86_400 => format!("{} h ago", secs / 3600),
        _ => format!("{} days ago", secs / 86_400),
    }
}

fn event_loop(
    engine: &Engine,
    terminal: &mut ratatui::DefaultTerminal,
    app: &mut App,
    msgs: &Receiver<Msg>,
    executor: &Executor,
    kick: &Sender<()>,
) -> Result<()> {
    let mut last = Instant::now();
    let mut pending_width: Option<(u16, Instant)> = None;
    loop {
        while let Ok(msg) = msgs.try_recv() {
            match msg {
                Msg::Groups(g) => app.set_groups(g),
                Msg::Progress { id, text } => app.progress(&id, text),
                Msg::Done { id, outcome } => app.finish(&id, outcome.toast()),
            }
        }
        let now = Instant::now();
        app.advance(now - last);
        last = now;
        terminal.draw(|f| deaner_ui::draw(f, app))?;

        if let Some((w, at)) = pending_width
            && at.elapsed() > Duration::from_secs(1)
        {
            let _ = engine.update_state(|s| s.sidebar_width = Some(w));
            pending_width = None;
        }

        if !event::poll(Duration::from_millis(120))? {
            continue;
        }
        let command = match event::read()? {
            Event::Key(k) => app.handle_key(k),
            Event::Mouse(m) => app.handle_mouse(m),
            Event::Resize(w, _) => {
                if in_frame() {
                    pending_width = Some((w, Instant::now()));
                }
                None
            }
            _ => None,
        };
        match command {
            Some(Command::Quit) => return Ok(()),
            Some(Command::Refresh) => {
                let _ = kick.send(());
            }
            Some(c) => executor.submit(c),
            None => {}
        }
    }
}

// ---- refresher -------------------------------------------------------------

const GH_UNKNOWN: u8 = 0;
const GH_YES: u8 = 1;
const GH_NO: u8 = 2;

fn spawn_refresher(engine: Engine, tx: Sender<Msg>, kick_tx: Sender<()>, kick_rx: Receiver<()>) {
    std::thread::spawn(move || {
        let git: Poller<GitStatus> = Poller::new(8);
        let trees: Poller<Vec<Checkout>> = Poller::new(4);
        let branches: Poller<BranchStatus> = Poller::new(4);
        let fetches: Poller<()> = Poller::new(4);
        let cfg = engine.config.clone();

        // Probing for gh can itself hang (it is a network tool); do it off-thread.
        let gh = Arc::new(AtomicU8::new(if cfg.pr_poll_seconds == 0 {
            GH_NO
        } else {
            GH_UNKNOWN
        }));
        if cfg.pr_poll_seconds > 0 {
            let gh = Arc::clone(&gh);
            std::thread::spawn(move || {
                let ok = deaner_git::gh_available();
                gh.store(if ok { GH_YES } else { GH_NO }, Ordering::Relaxed);
            });
        }

        let kicker = |k: &Sender<()>| {
            let k = k.clone();
            move || {
                let _ = k.send(());
            }
        };
        let mut last_autosave = Instant::now();
        let autosave = Duration::from_secs(cfg.autosave_minutes * 60);

        loop {
            let mut views = engine.workspaces_with(&|repo| {
                let r = repo.clone();
                trees.request(
                    &repo.path,
                    Duration::from_secs(5),
                    move || Ok::<_, ()>(checkouts(&r)),
                    kicker(&kick_tx),
                );
                trees
                    .get(&repo.path)
                    .unwrap_or_else(|| vec![(repo.path.clone(), None, false)])
            });

            for v in &mut views {
                let live = v.status == WsStatus::Live;
                let every = if live {
                    Duration::from_secs(cfg.git_poll_seconds.max(1))
                } else {
                    Duration::from_secs(30)
                };
                let path = v.path.clone();
                git.request(
                    &v.path,
                    every,
                    move || deaner_git::status(&path),
                    kicker(&kick_tx),
                );
                v.git = git.get(&v.path);
                if v.branch.is_none() {
                    v.branch = v.git.as_ref().and_then(|g| g.branch.clone());
                }

                // Projects that aren't running get their badges too, just less often.
                if let (Some(branch), GH_YES) = (v.branch.clone(), gh.load(Ordering::Relaxed)) {
                    let path = v.path.clone();
                    let every = cfg.pr_poll_seconds.max(30) * if live { 1 } else { 5 };
                    branches.request(
                        &v.path,
                        Duration::from_secs(every),
                        move || deaner_git::branch_status(&path, &branch),
                        kicker(&kick_tx),
                    );
                }
                if let Some(status) = branches.get(&v.path) {
                    v.pr = status.pr;
                    v.checks = status.checks;
                }

                // Worktrees share their repo's remote refs, so this is per repo.
                if live && cfg.fetch_seconds > 0 {
                    let repo = v.repo.clone();
                    fetches.request(
                        &v.repo,
                        Duration::from_secs(cfg.fetch_seconds.max(30)),
                        move || deaner_git::fetch(&repo),
                        kicker(&kick_tx),
                    );
                }
            }

            if tx.send(Msg::Groups(engine.groups(views))).is_err() {
                return;
            }
            if !autosave.is_zero() && last_autosave.elapsed() >= autosave {
                last_autosave = Instant::now();
                let _ = engine.autosave();
            }
            match kick_rx.recv_timeout(Duration::from_secs(1)) {
                Ok(()) => {
                    // Coalesce bursts (many repos finishing at once) into one refresh.
                    std::thread::sleep(Duration::from_millis(40));
                    while kick_rx.try_recv().is_ok() {}
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return,
            }
        }
    });
}

// ---- executor --------------------------------------------------------------

type Runner = Arc<dyn Fn(Command) -> Result<Outcome> + Send + Sync>;

/// Key that marks a restore in flight; a restore runs alone.
const RESTORE_KEY: &str = "restore";

#[derive(Debug, PartialEq, Eq)]
enum Lane {
    /// Fast state changes, applied in order on one thread.
    Quick,
    /// May take a while: its own thread, one per `key` at a time.
    Background {
        key: String,
        name: String,
        progress: Option<String>,
    },
}

fn lane(cmd: &Command, cfg: &Config) -> Lane {
    let bg = |key: String, name: &str, progress: Option<String>| Lane::Background {
        key,
        name: name.to_string(),
        progress,
    };
    match cmd {
        Command::Open(ws) => bg(
            ws.path.display().to_string(),
            &ws.name,
            match ws.status {
                WsStatus::Live => None,
                WsStatus::Parked => Some(format!("Unparking {}…", ws.name)),
                WsStatus::Stopped => Some(format!("Starting {}…", ws.name)),
            },
        ),
        Command::NewTab { ws, .. } => bg(ws.path.display().to_string(), &ws.name, None),
        Command::Park(ws) => bg(
            ws.path.display().to_string(),
            &ws.name,
            Some(format!("Parking {}…", ws.name)),
        ),
        Command::Archive(ws) => bg(
            ws.path.display().to_string(),
            &ws.name,
            Some(format!("Archiving {}…", ws.name)),
        ),
        Command::NewWorktree { repo, branch } => bg(
            cfg.worktree_path(repo, branch).display().to_string(),
            branch,
            Some(format!("Creating worktree {branch}…")),
        ),
        Command::Restore => bg(
            RESTORE_KEY.into(),
            "restore",
            Some("Restoring workspaces…".into()),
        ),
        Command::AddRepo(_)
        | Command::TogglePin(_)
        | Command::MovePin(..)
        | Command::ToggleCollapse(_)
        | Command::ForgetRepo(_)
        | Command::Save
        | Command::Cheatsheet
        | Command::BrowseRepos
        | Command::Refresh
        | Command::Quit => Lane::Quick,
    }
}

struct Executor {
    run: Runner,
    cfg: Config,
    busy: Arc<Mutex<HashSet<String>>>,
    quick: Sender<Command>,
    tx: Sender<Msg>,
    kick: Sender<()>,
}

fn report(tx: &Sender<Msg>, kick: &Sender<()>, id: String, result: Result<Outcome>) {
    let outcome = result.unwrap_or_else(|e| Outcome::Err(format!("{e:#}")));
    let _ = tx.send(Msg::Done { id, outcome });
    let _ = kick.send(());
}

impl Executor {
    fn new(run: Runner, cfg: Config, tx: Sender<Msg>, kick: Sender<()>) -> Self {
        let (quick, quick_rx) = mpsc::channel::<Command>();
        let (r, t, k) = (Arc::clone(&run), tx.clone(), kick.clone());
        std::thread::spawn(move || {
            for (n, cmd) in quick_rx.into_iter().enumerate() {
                report(&t, &k, format!("quick-{n}"), r(cmd));
            }
        });
        Self {
            run,
            cfg,
            busy: Arc::default(),
            quick,
            tx,
            kick,
        }
    }

    /// Queue a command. Never blocks.
    fn submit(&self, cmd: Command) {
        let (key, name, progress) = match lane(&cmd, &self.cfg) {
            Lane::Quick => {
                let _ = self.quick.send(cmd);
                return;
            }
            Lane::Background {
                key,
                name,
                progress,
            } => (key, name, progress),
        };
        let refusal = {
            let mut busy = self
                .busy
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if busy.contains(RESTORE_KEY) {
                Some("Restoring workspaces; try again in a moment".to_string())
            } else if key == RESTORE_KEY && !busy.is_empty() {
                Some("Wait for running actions to finish, then restore".to_string())
            } else if !busy.insert(key.clone()) {
                Some(format!("Still working on {name}"))
            } else {
                None
            }
        };
        if let Some(text) = refusal {
            let _ = self.tx.send(Msg::Done {
                id: format!("refused-{key}"),
                outcome: Outcome::Info(text),
            });
            return;
        }
        if let Some(text) = progress {
            let _ = self.tx.send(Msg::Progress {
                id: key.clone(),
                text,
            });
        }
        let (run, busy, tx, kick) = (
            Arc::clone(&self.run),
            Arc::clone(&self.busy),
            self.tx.clone(),
            self.kick.clone(),
        );
        std::thread::spawn(move || {
            let result = run(cmd);
            busy.lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .remove(&key);
            report(&tx, &kick, key, result);
        });
    }
}

fn execute(engine: &Engine, switcher: &Switcher, command: Command) -> Result<Outcome> {
    let shown = |name: &str| -> Result<Outcome> {
        Ok(switcher
            .show(engine, name)?
            .map_or(Outcome::Silent, Outcome::Info))
    };
    Ok(match command {
        Command::Open(ws) => return shown(&engine.ensure_session(&ws)?),
        Command::NewWorktree { repo, branch } => {
            let name = engine.new_worktree(&repo, &branch)?;
            switcher.show(engine, &name)?;
            Outcome::Ok(format!("Created {name}"))
        }
        Command::NewTab { ws, tab } => {
            let name = engine.ensure_session(&ws)?;
            engine.new_tab(&name, &ws.path, &tab)?;
            return shown(&name);
        }
        Command::AddRepo(path) => Outcome::Ok(format!("Added {}", engine.add_repo(&path)?.name)),
        Command::TogglePin(path) => {
            if engine.update_state(|s| s.toggle_pin(&path))? {
                Outcome::Silent
            } else {
                Outcome::Info("Unpinned".into())
            }
        }
        Command::MovePin(path, delta) => {
            engine.update_state(|s| s.move_pin(&path, delta))?;
            Outcome::Silent
        }
        Command::ToggleCollapse(path) => {
            engine.update_state(|s| s.toggle_collapsed(&path))?;
            Outcome::Silent
        }
        Command::Archive(ws) => {
            engine.archive(&ws, false)?;
            Outcome::Ok(format!("Archived {}", ws.name))
        }
        Command::ForgetRepo(path) => {
            engine.forget_repo(&path)?;
            Outcome::Info(format!("Forgot {}", path.display()))
        }
        Command::Park(ws) => {
            engine.park(&ws.name)?;
            Outcome::Ok(format!("Parked {}", ws.name))
        }
        Command::Save => {
            let snap = engine.snapshot();
            let n = snap.workspaces.len();
            engine.save_snapshot(&snap)?;
            Outcome::Ok(format!("Saved {n} workspace(s)"))
        }
        Command::Restore => {
            let snap = engine
                .latest_snapshot()
                .ok_or_else(|| anyhow::anyhow!("no snapshot to restore"))?;
            let report = engine.restore(&snap)?;
            if let Some(first) = report.restored.first() {
                switcher.show(engine, first)?;
            }
            let mut text = format!("Restored {}", report.restored.len());
            if !report.skipped.is_empty() {
                text.push_str(&format!(", skipped {}", report.skipped.len()));
            }
            Outcome::Ok(text)
        }
        Command::Cheatsheet => {
            if let Switcher::Frame(frame) = switcher {
                crate::frame::open_popup(frame, "cheatsheet")?;
            }
            Outcome::Silent
        }
        Command::BrowseRepos => {
            if let Switcher::Frame(frame) = switcher {
                crate::frame::open_popup(frame, "browse")?;
            }
            Outcome::Silent
        }
        Command::Refresh | Command::Quit => Outcome::Silent,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use deaner_core::AgentState;
    use deaner_core::sidebar::WorkspaceView;
    use std::path::PathBuf;

    fn ws(name: &str, status: WsStatus) -> WorkspaceView {
        WorkspaceView {
            repo: PathBuf::from("/r"),
            path: PathBuf::from(format!("/r/{name}")),
            name: name.into(),
            branch: None,
            worktree: false,
            status,
            agent: AgentState::None,
            git: None,
            pr: None,
            checks: deaner_core::Checks::None,
            tabs: vec![],
            current: false,
        }
    }

    /// Executor with a fake runner: `Open` blocks until released, everything else is instant.
    fn executor() -> (Executor, Receiver<Msg>, Sender<()>, Receiver<()>) {
        let (tx, rx) = mpsc::channel();
        let (kick, kick_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel::<()>();
        let release_rx = Arc::new(Mutex::new(release_rx));
        let run: Runner = Arc::new(move |cmd| {
            Ok(match cmd {
                Command::Open(ws) => {
                    let _ = release_rx.lock().unwrap().recv();
                    Outcome::Ok(format!("opened {}", ws.name))
                }
                Command::TogglePin(p) => Outcome::Ok(format!("pinned {}", p.display())),
                Command::Restore => Outcome::Ok("restored".into()),
                other => Outcome::Info(format!("{other:?}")),
            })
        });
        (
            Executor::new(run, Config::default(), tx, kick),
            rx,
            release_tx,
            kick_rx,
        )
    }

    fn next_done(rx: &Receiver<Msg>) -> Outcome {
        loop {
            match rx.recv_timeout(Duration::from_secs(5)).expect("a message") {
                Msg::Done { outcome, .. } => return outcome,
                Msg::Progress { .. } | Msg::Groups(_) => {}
            }
        }
    }

    #[test]
    fn slow_open_does_not_block_quick_commands_or_other_workspaces() {
        let (ex, rx, release, _kicks) = executor();
        ex.submit(Command::Open(ws("api", WsStatus::Stopped)));
        assert!(matches!(
            rx.recv_timeout(Duration::from_secs(5)).unwrap(),
            Msg::Progress { .. }
        ));
        ex.submit(Command::TogglePin(PathBuf::from("/r")));
        assert_eq!(
            next_done(&rx),
            Outcome::Ok("pinned /r".into()),
            "pin lands while open is stuck"
        );

        // same workspace again: refused, not queued behind it
        ex.submit(Command::Open(ws("api", WsStatus::Stopped)));
        assert_eq!(next_done(&rx), Outcome::Info("Still working on api".into()));

        // a different workspace runs in parallel
        ex.submit(Command::Open(ws("web", WsStatus::Live)));
        release.send(()).unwrap();
        release.send(()).unwrap();
        let mut done = vec![next_done(&rx), next_done(&rx)];
        done.sort_by_key(|o| format!("{o:?}"));
        assert_eq!(
            done,
            [
                Outcome::Ok("opened api".into()),
                Outcome::Ok("opened web".into())
            ]
        );
    }

    #[test]
    fn restore_runs_alone() {
        let (ex, rx, release, _kicks) = executor();
        ex.submit(Command::Open(ws("api", WsStatus::Stopped)));
        ex.submit(Command::Restore);
        assert_eq!(
            next_done(&rx),
            Outcome::Info("Wait for running actions to finish, then restore".into())
        );
        release.send(()).unwrap();
        assert_eq!(next_done(&rx), Outcome::Ok("opened api".into()));
        // wait until the open's busy slot is released
        let deadline = Instant::now() + Duration::from_secs(5);
        while !ex.busy.lock().unwrap().is_empty() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        ex.submit(Command::Restore);
        assert_eq!(next_done(&rx), Outcome::Ok("restored".into()));
    }

    #[test]
    fn lanes() {
        let cfg = Config::default();
        assert_eq!(lane(&Command::Save, &cfg), Lane::Quick);
        assert_eq!(
            lane(&Command::MovePin(PathBuf::from("/r"), 1), &cfg),
            Lane::Quick
        );
        assert!(matches!(
            lane(&Command::Open(ws("api", WsStatus::Live)), &cfg),
            Lane::Background { progress: None, .. }
        ));
        assert!(matches!(
            lane(
                &Command::NewWorktree {
                    repo: PathBuf::from("/r/api"),
                    branch: "x".into()
                },
                &cfg
            ),
            Lane::Background { key, progress: Some(_), .. } if key == "/r/api.worktrees/x"
        ));
    }
}
