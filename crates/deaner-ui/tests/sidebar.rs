//! Sidebar behaviour and rendering, driven through the public API only.

use deaner_core::sidebar::{Layout, RepoGroup, TabView, WorkspaceView, WsStatus, build};
use deaner_core::{AgentState, Checks, GitStatus, PrState, PrStatus, RepoEntry, TabSpec};
use deaner_ui::{App, Command, Mode, draw};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::path::PathBuf;

fn ws(repo: &str, path: &str, branch: &str, agent: AgentState) -> WorkspaceView {
    WorkspaceView {
        repo: PathBuf::from(repo),
        path: PathBuf::from(path),
        name: if repo == path {
            repo.rsplit('/').next().unwrap().to_string()
        } else {
            format!("{}/{branch}", repo.rsplit('/').next().unwrap())
        },
        branch: Some(branch.to_string()),
        worktree: repo != path,
        status: WsStatus::Live,
        agent,
        git: None,
        pr: None,
        tabs: vec![TabView {
            index: 0,
            name: "claude".into(),
            state: agent,
            agent: Some("claude".into()),
            address: "x:claude".into(),
        }],
        current: false,
    }
}

fn fixture(agent_on_feat: AgentState) -> Vec<RepoGroup> {
    let repos: Vec<RepoEntry> = ["shop-api", "deaner", "dotfiles"]
        .iter()
        .map(|n| RepoEntry {
            path: PathBuf::from(format!("/dev/{n}")),
            name: (*n).to_string(),
        })
        .collect();
    let pins = vec![PathBuf::from("/dev/deaner"), PathBuf::from("/dev/shop-api")];
    let mut main = ws("/dev/shop-api", "/dev/shop-api", "main", AgentState::Busy);
    main.git = Some(GitStatus {
        branch: Some("main".into()),
        modified: 3,
        ahead: 1,
        ..GitStatus::default()
    });
    main.current = true;
    let mut feat = ws(
        "/dev/shop-api",
        "/dev/shop-api.worktrees/sec-bump",
        "sec-bump",
        agent_on_feat,
    );
    feat.pr = Some(PrStatus {
        number: 42,
        state: PrState::Open,
        title: "Bump deps".into(),
        url: String::new(),
        checks: Checks::Passing,
        review: None,
    });
    let deaner = ws("/dev/deaner", "/dev/deaner", "main", AgentState::Idle);
    let mut dot = ws("/dev/dotfiles", "/dev/dotfiles", "main", AgentState::None);
    dot.status = WsStatus::Parked;
    build(
        &Layout {
            repos: &repos,
            pins: &pins,
            collapsed: &[],
            float_attention: true,
        },
        vec![main, feat, deaner, dot],
    )
}

fn render(app: &mut App, w: u16, h: u16) -> String {
    let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
    term.draw(|f| draw(f, app)).unwrap();
    term.backend().to_string()
}

fn key(c: KeyCode) -> KeyEvent {
    KeyEvent::new(c, KeyModifiers::NONE)
}

fn ch(c: char) -> KeyEvent {
    key(KeyCode::Char(c))
}

fn type_str(app: &mut App, s: &str) {
    for c in s.chars() {
        assert_eq!(app.handle_key(ch(c)), None);
    }
}

#[test]
fn renders_pinned_groups_and_status() {
    let mut app = App::new();
    app.set_groups(fixture(AgentState::Awaiting));
    insta::assert_snapshot!(render(&mut app, 34, 12));
}

#[test]
fn help_overlay() {
    let mut app = App::new();
    app.set_groups(fixture(AgentState::Idle));
    app.handle_key(ch('?'));
    assert!(matches!(app.mode(), Mode::Help(0)));
    app.handle_key(ch('j'));
    assert!(matches!(app.mode(), Mode::Help(1)), "j scrolls help");
    app.handle_key(ch('k'));
    insta::assert_snapshot!(render(&mut app, 40, 30));
    app.handle_key(ch('x'));
    assert!(matches!(app.mode(), Mode::Normal), "any key closes help");
}

#[test]
fn empty_state_hint() {
    let mut app = App::new();
    app.set_groups(vec![]);
    insta::assert_snapshot!(render(&mut app, 34, 8));
}

#[test]
fn enter_opens_main_checkout_from_repo_row() {
    let mut app = App::new();
    app.set_groups(fixture(AgentState::Idle));
    // first row is the pinned "deaner" repo
    match app.handle_key(key(KeyCode::Enter)) {
        Some(Command::Open(w)) => assert_eq!(w.path, PathBuf::from("/dev/deaner")),
        other => panic!("expected open, got {other:?}"),
    }
}

#[test]
fn selection_survives_refresh_and_reorder() {
    let mut app = App::new();
    app.set_groups(fixture(AgentState::Idle));
    // move to the sec-bump worktree row: deaner, deaner/main, shop-api, main, sec-bump
    for _ in 0..4 {
        app.handle_key(ch('j'));
    }
    assert_eq!(
        app.selected_ws().unwrap().branch.as_deref(),
        Some("sec-bump")
    );
    app.set_groups(fixture(AgentState::Busy));
    assert_eq!(
        app.selected_ws().unwrap().branch.as_deref(),
        Some("sec-bump")
    );
}

#[test]
fn awaiting_transition_raises_toast() {
    let mut app = App::new();
    app.set_groups(fixture(AgentState::Busy));
    app.set_groups(fixture(AgentState::Awaiting));
    let screen = render(&mut app, 34, 12);
    assert!(screen.contains("sec-bump needs"), "{screen}");
}

#[test]
fn new_worktree_prompt_emits_command() {
    let mut app = App::new();
    app.set_groups(fixture(AgentState::Idle));
    app.handle_key(ch('j'));
    app.handle_key(ch('j')); // shop-api repo row
    app.handle_key(ch('w'));
    assert!(matches!(app.mode(), Mode::Prompt { .. }));
    type_str(&mut app, "sec/fix");
    insta::assert_snapshot!(render(&mut app, 34, 12));
    assert_eq!(
        app.handle_key(key(KeyCode::Enter)),
        Some(Command::NewWorktree {
            repo: PathBuf::from("/dev/shop-api"),
            branch: "sec/fix".into()
        })
    );
}

#[test]
fn filter_narrows_rows_and_esc_clears() {
    let mut app = App::new();
    app.set_groups(fixture(AgentState::Idle));
    app.handle_key(ch('/'));
    type_str(&mut app, "sec");
    app.handle_key(key(KeyCode::Enter));
    assert_eq!(app.filter(), "sec");
    let screen = render(&mut app, 34, 8);
    assert!(
        screen.contains("sec-bump") && !screen.contains("dotfiles"),
        "{screen}"
    );
    app.handle_key(key(KeyCode::Esc));
    assert_eq!(app.filter(), "");
}

#[test]
fn pin_moves_require_a_pin_and_archive_asks() {
    let mut app = App::new();
    app.set_groups(fixture(AgentState::Idle));
    assert_eq!(
        app.handle_key(ch('J')),
        Some(Command::MovePin(PathBuf::from("/dev/deaner"), 1))
    );
    app.handle_key(key(KeyCode::End)); // dotfiles/main (unpinned, parked)
    assert_eq!(
        app.handle_key(ch('K')),
        None,
        "unpinned repos cannot be moved"
    );

    // archive on a main checkout = forget repo, after confirmation
    app.handle_key(ch('x'));
    assert!(matches!(app.mode(), Mode::Confirm { .. }));
    assert_eq!(
        app.handle_key(ch('y')),
        Some(Command::ForgetRepo(PathBuf::from("/dev/dotfiles")))
    );
}

#[test]
fn new_tab_picker() {
    let mut app = App::new();
    app.tab_choices = Box::new(|_| {
        vec![
            TabSpec::new("claude", Some("claude")),
            TabSpec::new("nvim", Some("nvim")),
            TabSpec::new("shell", None),
        ]
    });
    app.set_groups(fixture(AgentState::Idle));
    app.handle_key(ch('t'));
    app.handle_key(ch('j'));
    insta::assert_snapshot!(render(&mut app, 34, 12));
    match app.handle_key(key(KeyCode::Enter)) {
        Some(Command::NewTab { tab, ws }) => {
            assert_eq!(tab.name, "nvim");
            assert_eq!(ws.path, PathBuf::from("/dev/deaner"));
        }
        other => panic!("{other:?}"),
    }
    app.handle_key(ch('t'));
    assert!(
        matches!(app.handle_key(ch('3')), Some(Command::NewTab { tab, .. }) if tab.name == "shell")
    );
}

#[test]
fn park_only_live_workspaces() {
    let mut app = App::new();
    app.set_groups(fixture(AgentState::Idle));
    app.handle_key(key(KeyCode::End)); // parked dotfiles
    assert_eq!(app.handle_key(ch('P')), None);
    assert!(matches!(app.mode(), Mode::Normal));
    app.handle_key(key(KeyCode::Home));
    app.handle_key(ch('P'));
    assert!(matches!(app.handle_key(ch('y')), Some(Command::Park(w)) if w.name == "deaner"));
}
