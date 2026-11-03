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
    app.standalone = true; // inside the frame, ? opens the popup instead
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

#[test]
fn selection_skips_section_headings() {
    let mut app = App::new();
    app.set_groups(fixture(AgentState::Idle));
    // Rows: PINNED, deaner, main, shop-api, main, sec-bump, INACTIVE, dotfiles, main.
    assert_eq!(
        app.selected_group().unwrap().name,
        "deaner",
        "starts below the heading"
    );

    app.handle_key(ch('k'));
    assert_eq!(
        app.selected_group().unwrap().name,
        "deaner",
        "can't move onto the top heading"
    );

    for _ in 0..5 {
        app.handle_key(ch('j'));
    }
    assert_eq!(
        app.selected_group().unwrap().name,
        "dotfiles",
        "j steps over INACTIVE"
    );
    app.handle_key(ch('k'));
    assert_eq!(
        app.selected_ws().unwrap().branch.as_deref(),
        Some("sec-bump"),
        "k steps back over it"
    );

    app.handle_key(key(KeyCode::End));
    assert_eq!(app.selected_group().unwrap().name, "dotfiles");
    app.handle_key(key(KeyCode::Home));
    assert_eq!(app.selected_group().unwrap().name, "deaner");
}

#[test]
fn clicking_a_heading_does_nothing() {
    use ratatui::crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
    let mut app = App::new();
    app.set_groups(fixture(AgentState::Idle));
    render(&mut app, 34, 12); // records where the list is
    let click = |row| MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 3,
        row,
        modifiers: KeyModifiers::NONE,
    };
    // The list starts at the top of the screen: PINNED is row 0, deaner row 1.
    assert_eq!(app.handle_mouse(click(0)), None);
    assert_eq!(app.selected_group().unwrap().name, "deaner");
    // Row 2 is deaner's main checkout: a click opens it.
    assert!(
        matches!(app.handle_mouse(click(2)), Some(Command::Open(w)) if w.path == std::path::Path::new("/dev/deaner"))
    );
}

#[test]
fn selection_follows_a_repo_into_another_section() {
    let mut app = App::new();
    app.set_groups(fixture(AgentState::Idle));
    app.handle_key(key(KeyCode::End));
    assert_eq!(app.selected_group().unwrap().name, "dotfiles");

    // dotfiles starts running: it moves from INACTIVE to ACTIVE, and the cursor goes with it.
    let mut groups = fixture(AgentState::Idle);
    let dot = groups.iter_mut().find(|g| g.name == "dotfiles").unwrap();
    dot.workspaces[0].status = WsStatus::Live;
    dot.section = deaner_core::sidebar::Section::Active;
    app.set_groups(groups);
    assert_eq!(app.selected_group().unwrap().name, "dotfiles");
    let screen = render(&mut app, 34, 12);
    assert!(screen.contains("ACTIVE"), "{screen}");
}

#[test]
fn nerd_icons_show_git_state() {
    let mut app = App::new();
    app.icons = &deaner_ui::icons::NERD;
    app.set_groups(fixture(AgentState::Idle));
    let screen = render(&mut app, 34, 12);
    let nerd = &deaner_ui::icons::NERD;
    for glyph in [
        nerd.branch,
        nerd.pull_request,
        nerd.pinned,
        nerd.changed,
        nerd.checks_passing,
    ] {
        assert!(screen.contains(glyph), "missing {glyph:?} in {screen}");
    }
    assert!(screen.contains("+1 -0"), "{screen}");
    assert!(screen.contains("#42"), "{screen}");
}

#[test]
fn detached_checkouts_show_a_short_commit_id() {
    let mut groups = fixture(AgentState::Idle);
    let w = &mut groups[0].workspaces[0];
    w.branch = None;
    w.git = Some(GitStatus {
        oid: Some("5b32e4f0123456789".into()),
        ..GitStatus::default()
    });
    let mut app = App::new();
    app.set_groups(groups);
    let screen = render(&mut app, 34, 12);
    assert!(
        screen.contains("5b32e4f") && !screen.contains("5b32e4f0"),
        "{screen}"
    );
}

#[test]
fn question_mark_opens_the_popup_inside_the_frame() {
    let mut app = App::new();
    app.set_groups(fixture(AgentState::Idle));
    assert_eq!(app.handle_key(ch('?')), Some(Command::Cheatsheet));
    assert!(matches!(app.mode(), Mode::Normal));
}

#[test]
fn sidebar_only_uses_the_terminals_colors() {
    use ratatui::style::Color;
    let check = |app: &mut App, what: &str| {
        let mut term = Terminal::new(TestBackend::new(34, 16)).unwrap();
        term.draw(|f| draw(f, app)).unwrap();
        for cell in &term.backend().buffer().content {
            for color in [cell.fg, cell.bg] {
                assert!(
                    !matches!(color, Color::Rgb(..) | Color::Indexed(_)),
                    "{what} paints fixed color {color:?} at {:?}",
                    cell.symbol()
                );
            }
        }
    };
    let mut app = App::new();
    app.standalone = true;
    app.tab_choices = Box::new(|_| vec![TabSpec::new("shell", None)]);
    app.set_groups(fixture(AgentState::Busy));
    check(&mut app, "the list");

    app.set_groups(fixture(AgentState::Awaiting)); // raises a toast
    app.error("something failed");
    app.progress("job", "working");
    check(&mut app, "toasts");

    for (keys, what) in [
        ("/", "the filter"),
        ("w", "a prompt"),
        ("t", "the tab picker"),
        ("x", "a confirmation"),
        ("?", "the help overlay"),
    ] {
        app.handle_key(ch(keys.chars().next().unwrap()));
        check(&mut app, what);
        app.handle_key(key(KeyCode::Esc));
    }
}

#[test]
fn a_browses_inside_the_frame_and_prompts_outside_it() {
    let mut app = App::new();
    app.set_groups(fixture(AgentState::Idle));
    assert_eq!(app.handle_key(ch('a')), Some(Command::BrowseRepos));

    app.standalone = true;
    assert_eq!(app.handle_key(ch('a')), None);
    assert!(matches!(app.mode(), Mode::Prompt { .. }));
}

fn cell_at(app: &mut App, needle: &str) -> ratatui::buffer::Cell {
    // Tall enough that toasts stacked at the bottom don't cover the rows being checked.
    let mut term = Terminal::new(TestBackend::new(34, 40)).unwrap();
    term.draw(|f| draw(f, app)).unwrap();
    let buf = term.backend().buffer();
    let width = usize::from(buf.area.width);
    for (y, row) in buf.content.chunks(width).enumerate() {
        let text: String = row.iter().map(ratatui::buffer::Cell::symbol).collect();
        if let Some(byte) = text.find(needle) {
            let x = text[..byte].chars().count();
            return buf.content[y * width + x].clone();
        }
    }
    panic!("{needle} not on screen");
}

#[test]
fn the_open_project_is_highlighted_wherever_the_cursor_is() {
    use ratatui::style::Color;
    let mut app = App::new();
    app.set_groups(fixture(AgentState::Idle));
    // shop-api's main is the open one (and the only "main" with changed files).
    let mut term = Terminal::new(TestBackend::new(34, 14)).unwrap();
    term.draw(|f| draw(f, &mut app)).unwrap();
    let buf = term.backend().buffer().clone();
    let row = |y: u16| -> String { (0..34).map(|x| buf[(x, y)].symbol().to_string()).collect() };
    let y = (0..14).find(|y| row(*y).contains("3 +1")).unwrap();
    assert_eq!(
        buf[(0, y)].bg,
        Color::Magenta,
        "the whole row, edge to edge"
    );
    assert_eq!(buf[(33, y)].bg, Color::Magenta);
    assert_eq!(buf[(4, y)].fg, Color::Black, "and readable");

    // The cursor is elsewhere, on another background.
    assert_eq!(cell_at(&mut app, "deaner").bg, Color::DarkGray);
    assert!(
        row(y).starts_with("  "),
        "no cursor bar here yet: {}",
        row(y)
    );

    // Moving onto it keeps the highlight, and the cursor shows as a bar.
    for _ in 0..3 {
        app.handle_key(ch('j'));
    }
    let mut term = Terminal::new(TestBackend::new(34, 14)).unwrap();
    term.draw(|f| draw(f, &mut app)).unwrap();
    let buf = term.backend().buffer();
    assert_eq!(buf[(0, y)].symbol(), "▌");
    assert_eq!(buf[(0, y)].bg, Color::Magenta);
}

#[test]
fn needing_attention_turns_the_name_blue_until_you_look() {
    use ratatui::style::Color;
    let mut app = App::new();
    app.set_groups(fixture(AgentState::Busy));
    assert_ne!(cell_at(&mut app, "shop-api").fg, Color::Blue);

    // Waiting on you: blue while it waits.
    app.set_groups(fixture(AgentState::Awaiting));
    assert_eq!(cell_at(&mut app, "shop-api").fg, Color::Blue);
    assert_eq!(cell_at(&mut app, "sec-bump").fg, Color::Blue);
    assert_ne!(
        cell_at(&mut app, "deaner").fg,
        Color::Blue,
        "only that repo"
    );

    // Finished while you were elsewhere: stays blue after it stops waiting.
    app.set_groups(fixture(AgentState::Busy));
    assert_ne!(cell_at(&mut app, "shop-api").fg, Color::Blue);
    app.set_groups(fixture(AgentState::Idle));
    assert_eq!(cell_at(&mut app, "shop-api").fg, Color::Blue);
    app.set_groups(fixture(AgentState::Idle));
    assert_eq!(
        cell_at(&mut app, "sec-bump").fg,
        Color::Blue,
        "until you look"
    );

    // Opening it clears it.
    let mut groups = fixture(AgentState::Idle);
    let shop = groups.iter_mut().find(|g| g.name == "shop-api").unwrap();
    for w in &mut shop.workspaces {
        w.current = w.branch.as_deref() == Some("sec-bump");
    }
    app.set_groups(groups);
    assert_ne!(cell_at(&mut app, "shop-api").fg, Color::Blue);
}

#[test]
fn the_arrow_only_appears_on_a_collapsed_repo() {
    let mut app = App::new();
    app.set_groups(fixture(AgentState::Idle));
    let screen = render(&mut app, 34, 12);
    assert!(!screen.contains('▾') && !screen.contains('▸'), "{screen}");

    let mut groups = fixture(AgentState::Idle);
    groups[0].collapsed = true;
    app.set_groups(groups);
    let screen = render(&mut app, 34, 12);
    assert!(screen.contains("▸ deaner"), "{screen}");
}

#[test]
fn passing_ci_shows_a_check_next_to_the_pr_number() {
    let mut app = App::new();
    app.set_groups(fixture(AgentState::Idle));
    assert!(render(&mut app, 34, 12).contains("#42 ✓"));

    for (checks, glyph) in [(Checks::Failing, "#42 ✗"), (Checks::Pending, "#42 …")] {
        let mut groups = fixture(AgentState::Idle);
        let shop = groups.iter_mut().find(|g| g.name == "shop-api").unwrap();
        shop.workspaces[1].pr.as_mut().unwrap().checks = checks;
        app.set_groups(groups);
        assert!(render(&mut app, 34, 12).contains(glyph));
    }
    let mut groups = fixture(AgentState::Idle);
    let shop = groups.iter_mut().find(|g| g.name == "shop-api").unwrap();
    let pr = shop.workspaces[1].pr.as_mut().unwrap();
    pr.checks = Checks::None;
    pr.state = PrState::Merged;
    app.set_groups(groups);
    let screen = render(&mut app, 34, 12);
    assert!(screen.contains("#42") && !screen.contains('✓'), "{screen}");
}
