//! End-to-end engine tests against an isolated tmux server and temp git repos.
//! A stub script stands in for the agent and logs the arguments it was started with.

use deaner::engine::Engine;
use deaner::hooks;
use deaner::paths::Paths;
use deaner_core::agent::{AgentState, HookEvent, HookPayload};
use deaner_core::config::{AgentDef, Config, TabSpec};
use deaner_core::sidebar::WsStatus;
use deaner_tmux::TestServer;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

struct Fixture {
    srv: TestServer,
    engine: Engine,
    repo: PathBuf,
    agent_log: PathBuf,
    _tmp: tempfile::TempDir,
}

fn git(dir: &Path, args: &[&str]) {
    let ok = Command::new("git")
        .current_dir(dir)
        .args(args)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@t")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@t")
        .status()
        .unwrap()
        .success();
    assert!(ok, "git {args:?}");
}

fn fixture() -> Fixture {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    let repo = root.join("api");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    std::fs::write(repo.join("README.md"), "hi").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-qm", "init"]);

    let agent_log = root.join("agent.log");
    let agent = root.join("fake-agent");
    std::fs::write(
        &agent,
        format!(
            "#!/bin/sh\necho \"start $*\" >> '{}'\nexec sleep 300\n",
            agent_log.display()
        ),
    )
    .unwrap();
    Command::new("chmod")
        .arg("+x")
        .arg(&agent)
        .status()
        .unwrap();

    let agent_cmd = agent.display().to_string();
    let mut config = Config::default();
    config.agents.clear();
    config.agents.insert(
        "fake".into(),
        AgentDef {
            command: agent_cmd.clone(),
            resume: Some(format!("{agent_cmd} --resume {{session_id}}")),
        },
    );
    config.tabs = vec![
        TabSpec::new("agent", Some(&agent_cmd)),
        TabSpec::new("shell", None),
    ];
    config.shells.push("sleep".into()); // never relaunch the shell pane's leftovers

    let srv = TestServer::new();
    let engine = Engine::new(srv.tmux.clone(), config, Paths::under(&root.join("home")));
    Fixture {
        srv,
        engine,
        repo,
        agent_log,
        _tmp: tmp,
    }
}

fn wait_for(what: &str, mut check: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(8);
    while Instant::now() < deadline {
        if check() {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("timed out waiting for {what}");
}

fn log(f: &Fixture) -> String {
    std::fs::read_to_string(&f.agent_log).unwrap_or_default()
}

fn agent_pane(f: &Fixture, session: &str) -> String {
    f.engine
        .managed_panes()
        .into_iter()
        .find(|p| p.session == session && p.window_name == "agent")
        .expect("agent pane")
        .pane_id
}

#[test]
fn full_day_create_status_save_restore_park_archive() {
    let f = fixture();
    let e = &f.engine;

    // add + open the main checkout
    let entry = e.add_repo(&f.repo.display().to_string()).unwrap();
    assert_eq!(entry.name, "api");
    let views = e.workspaces();
    assert_eq!(views.len(), 1);
    assert_eq!(views[0].status, WsStatus::Stopped);
    let main = e.ensure_session(&views[0]).unwrap();
    assert_eq!(main, "api");
    wait_for("agent start", || log(&f).contains("start"));

    // agent reports status through the hook handler
    let pane = agent_pane(&f, &main);
    let payload = HookPayload::parse(r#"{"session_id":"conv-1"}"#);
    hooks::handle(
        &f.srv.tmux,
        Some(&pane),
        "fake",
        HookEvent::SessionStart,
        &payload,
    );
    hooks::handle(
        &f.srv.tmux,
        Some(&pane),
        "fake",
        HookEvent::Busy,
        &HookPayload::default(),
    );
    let live = e.find("api").unwrap();
    assert_eq!(live.status, WsStatus::Live);
    assert_eq!(live.agent, AgentState::Busy);
    assert_eq!(
        live.tabs
            .iter()
            .map(|t| t.name.as_str())
            .collect::<Vec<_>>(),
        ["agent", "shell"]
    );
    assert_eq!(e.active_agents().len(), 1);

    // a worktree for a parallel branch
    let wt = e.new_worktree(&f.repo, "sec/bump").unwrap();
    assert_eq!(wt, "api/sec/bump");
    assert_eq!(e.workspaces().len(), 2);

    // save, "reboot", restore
    let snap = e.snapshot();
    assert_eq!(snap.workspaces.len(), 2);
    e.save_snapshot(&snap).unwrap();
    f.srv.tmux.kill_server();
    assert!(e.managed_panes().is_empty());
    assert!(
        e.autosave().unwrap().is_none(),
        "never autosave an empty server over a snapshot"
    );

    std::fs::write(&f.agent_log, "").unwrap();
    let report = e.restore(&e.latest_snapshot().unwrap()).unwrap();
    assert_eq!(report.restored.len(), 2, "skipped: {:?}", report.skipped);
    wait_for("agent resume", || log(&f).contains("--resume conv-1"));
    // the worktree's agent had no conversation id, so it starts fresh
    wait_for("fresh agent", || {
        log(&f).lines().any(|l| l.trim() == "start")
    });
    let restored = e.find("api").unwrap();
    assert_eq!(restored.status, WsStatus::Live);
    assert_eq!(restored.tabs.len(), 2);

    // restoring again skips what is running
    let again = e.restore(&e.latest_snapshot().unwrap()).unwrap();
    assert!(again.restored.is_empty() && again.skipped.len() == 2);

    // park the worktree, then open it again
    e.park("api/sec/bump").unwrap();
    let parked = e.find("api/sec/bump").unwrap();
    assert_eq!(parked.status, WsStatus::Parked);
    assert_eq!(e.parked().len(), 1);
    let back = e.ensure_session(&parked).unwrap();
    assert_eq!(back, "api/sec/bump");
    assert!(e.parked().is_empty());

    // archive it: session gone, worktree removed
    let ws = e.find("api/sec/bump").unwrap();
    std::fs::write(ws.path.join("scratch.txt"), "x").unwrap();
    assert!(
        e.archive(&ws, false).is_err(),
        "dirty worktree needs --force"
    );
    e.archive(&ws, true).unwrap();
    assert!(!ws.path.exists());
    assert_eq!(e.workspaces().len(), 1);

    // the main checkout cannot be archived
    assert!(e.archive(&e.find("api").unwrap(), true).is_err());
}

#[test]
fn send_and_tabs() {
    let f = fixture();
    let e = &f.engine;
    e.add_repo(&f.repo.display().to_string()).unwrap();
    let name = e.ensure_session(&e.workspaces()[0]).unwrap();
    assert_eq!(
        f.srv
            .tmux
            .show_option(deaner_tmux::Scope::Server, "@continuum-save-interval")
            .as_deref(),
        Some("0"),
        "continuum autosave is off on deaner's server"
    );

    let marker = f.repo.join("got-message");
    let addr = e
        .send(
            &format!("{name}:shell"),
            &format!("echo ok > '{}'", marker.display()),
            true,
        )
        .unwrap();
    assert_eq!(addr, "api:shell");
    wait_for("message delivered", || marker.exists());
    assert!(e.send("api:nope", "x", true).is_err());
    assert!(e.send("ghost", "x", true).is_err());

    e.new_tab(&name, &f.repo, &TabSpec::new("shell", None))
        .unwrap();
    let tabs = e.find("api").unwrap().tabs;
    assert_eq!(
        tabs.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(),
        ["agent", "shell", "shell-2"]
    );
}

#[test]
fn pins_order_groups_and_survive_forget() {
    let f = fixture();
    let e = &f.engine;
    let other = f.repo.parent().unwrap().join("zeta");
    std::fs::create_dir_all(&other).unwrap(); // plain folder, not a repo
    e.add_repo(&f.repo.display().to_string()).unwrap();
    e.add_repo(&other.display().to_string()).unwrap();

    let names = || {
        e.groups(e.workspaces())
            .iter()
            .map(|g| g.name.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(names(), ["api", "zeta"]);
    e.update_state(|s| s.toggle_pin(&other)).unwrap();
    assert_eq!(names(), ["zeta", "api"]);

    // plain folders get a session too
    let ws = e.find("zeta").unwrap();
    assert!(!ws.worktree && ws.branch.is_none());
    assert_eq!(e.ensure_session(&ws).unwrap(), "zeta");

    assert!(e.forget_repo(&other).unwrap());
    assert!(e.state().pins.is_empty());
}

#[test]
fn concurrent_state_updates_are_not_lost() {
    let f = fixture();
    let handles: Vec<_> = (0..32)
        .map(|i| {
            let e = f.engine.clone();
            std::thread::spawn(move || {
                e.update_state(|s| s.toggle_pin(Path::new(&format!("/repo/{i}"))))
                    .unwrap();
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
    assert_eq!(f.engine.state().pins.len(), 32, "every pin survived");
}
