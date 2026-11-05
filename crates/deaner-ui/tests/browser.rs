//! The project browser popup, with a fake filesystem.

use deaner_ui::browser::{Browser, BrowserAction, DirItem};
use deaner_ui::icons::UNICODE;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::path::{Path, PathBuf};

fn item(dir: &str, name: &str, is_repo: bool, added: bool) -> DirItem {
    DirItem {
        name: name.into(),
        path: PathBuf::from(format!("{dir}/{name}")),
        is_repo,
        added,
    }
}

fn browser() -> Browser {
    Browser::new(
        PathBuf::from("/home/sd/dev"),
        Some(PathBuf::from("/home/sd")),
        &UNICODE,
        Box::new(|dir: &Path| match dir.to_str().unwrap() {
            "/home/sd/dev" => Some(vec![
                item("/home/sd/dev", "website", true, false),
                item("/home/sd/dev", "api", true, true),
                item("/home/sd/dev", "go", false, false),
                item("/home/sd/dev", ".cache", false, false),
            ]),
            "/home/sd/dev/go" => Some(vec![item("/home/sd/dev/go", "tool", true, false)]),
            "/home/sd" => Some(vec![item("/home/sd", "dev", false, false)]),
            "/" => Some(vec![
                item("", "tmp", false, false),
                item("", "home", false, false),
            ]),
            "/tmp" => Some(vec![item("/tmp", "clone", true, false)]),
            _ => None,
        }),
    )
}

fn press(b: &mut Browser, code: KeyCode) -> BrowserAction {
    b.handle_key(KeyEvent::new(code, KeyModifiers::NONE))
}

fn typing(b: &mut Browser, text: &str) {
    for c in text.chars() {
        assert_eq!(press(b, KeyCode::Char(c)), BrowserAction::None);
    }
}

fn picked(path: &str) -> BrowserAction {
    BrowserAction::Pick(PathBuf::from(path))
}

#[test]
fn enter_opens_a_repo_right_away() {
    let mut b = browser();
    // Sorted, dotfolders hidden, and the cursor starts on the first folder.
    assert_eq!(b.visible_names(), ["api", "go", "website"]);
    assert_eq!(press(&mut b, KeyCode::Enter), picked("/home/sd/dev/api"));
}

#[test]
fn enter_walks_into_plain_folders_and_left_comes_back() {
    let mut b = browser();
    press(&mut b, KeyCode::Down);
    assert_eq!(press(&mut b, KeyCode::Enter), BrowserAction::None);
    assert_eq!(b.cwd(), Path::new("/home/sd/dev/go"));
    assert_eq!(b.visible_names(), ["tool"]);
    assert_eq!(
        press(&mut b, KeyCode::Enter),
        picked("/home/sd/dev/go/tool")
    );

    press(&mut b, KeyCode::Left);
    assert_eq!(b.cwd(), Path::new("/home/sd/dev"));
    // The cursor is back on the folder we came from.
    assert_eq!(press(&mut b, KeyCode::Right), BrowserAction::None);
    assert_eq!(b.cwd(), Path::new("/home/sd/dev/go"));
}

#[test]
fn a_plain_folder_can_be_picked_with_use_this_folder() {
    let mut b = browser();
    press(&mut b, KeyCode::Down);
    press(&mut b, KeyCode::Enter); // into go/
    press(&mut b, KeyCode::Up); // onto "Use this folder"
    assert_eq!(press(&mut b, KeyCode::Enter), picked("/home/sd/dev/go"));
}

#[test]
fn right_goes_into_a_repo_instead_of_opening_it() {
    let mut b = browser();
    assert_eq!(press(&mut b, KeyCode::Right), BrowserAction::None);
    assert_eq!(b.cwd(), Path::new("/home/sd/dev/api"));
}

#[test]
fn typing_narrows_and_backspace_goes_up_only_when_empty() {
    let mut b = browser();
    typing(&mut b, "WEB");
    assert_eq!(b.visible_names(), ["website"]);
    assert_eq!(
        press(&mut b, KeyCode::Enter),
        picked("/home/sd/dev/website")
    );
    for _ in 0..3 {
        press(&mut b, KeyCode::Backspace);
        assert_eq!(b.cwd(), Path::new("/home/sd/dev"), "deleting filter text");
    }
    press(&mut b, KeyCode::Backspace);
    assert_eq!(b.cwd(), Path::new("/home/sd"), "empty filter: up a folder");
}

#[test]
fn dotfolders_appear_when_asked_for_and_no_match_is_safe() {
    let mut b = browser();
    typing(&mut b, ".");
    assert_eq!(b.visible_names(), [".cache"]);
    let mut b = browser();
    typing(&mut b, "zzz");
    assert!(b.visible_names().is_empty());
    assert_eq!(press(&mut b, KeyCode::Enter), BrowserAction::None);
    assert_eq!(press(&mut b, KeyCode::Right), BrowserAction::None);
}

#[test]
fn typed_paths_jump_anywhere_on_the_system() {
    // A temp clone outside ~/dev, typed the way you'd type it in a shell.
    let mut b = browser();
    typing(&mut b, "/tmp/");
    assert_eq!(b.cwd(), Path::new("/tmp"));
    assert_eq!(press(&mut b, KeyCode::Enter), picked("/tmp/clone"));

    let mut b = browser();
    typing(&mut b, "~/");
    assert_eq!(b.cwd(), Path::new("/home/sd"));

    let mut b = browser();
    typing(&mut b, "~/dev/go/");
    assert_eq!(b.cwd(), Path::new("/home/sd/dev/go"));

    let mut b = browser();
    typing(&mut b, "go/");
    assert_eq!(b.cwd(), Path::new("/home/sd/dev/go"), "relative to here");
    typing(&mut b, "../");
    assert_eq!(b.cwd(), Path::new("/home/sd/dev"));

    // Somewhere that doesn't exist stays put, and the typing isn't lost.
    let mut b = browser();
    typing(&mut b, "nope/");
    assert_eq!(b.cwd(), Path::new("/home/sd/dev"));
    assert!(b.visible_names().is_empty());
}

#[test]
fn walking_up_reaches_the_root_and_stops() {
    let mut b = browser();
    for _ in 0..10 {
        press(&mut b, KeyCode::Left);
    }
    assert_eq!(b.cwd(), Path::new("/"));
    assert_eq!(b.visible_names(), ["home", "tmp"]);
}

fn ctrl_f(b: &mut Browser) -> BrowserAction {
    b.handle_key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL))
}

fn with_favorites() -> Browser {
    let mut b = browser();
    b.set_favorites(vec![
        PathBuf::from("/home/sd/dev/go"),
        PathBuf::from("/tmp"),
    ]);
    b
}

#[test]
fn enter_on_a_favorite_travels_there() {
    let mut b = with_favorites();
    // The cursor still starts on the first folder, so Enter never travels by accident.
    assert_eq!(press(&mut b, KeyCode::Enter), picked("/home/sd/dev/api"));

    // Up past "Use this folder" and over the blank line to the last favorite.
    press(&mut b, KeyCode::Up);
    press(&mut b, KeyCode::Up);
    assert_eq!(press(&mut b, KeyCode::Enter), BrowserAction::None);
    assert_eq!(b.cwd(), Path::new("/tmp"));

    // Favorites stay listed wherever you are, and the arrow key travels too.
    press(&mut b, KeyCode::Up);
    press(&mut b, KeyCode::Up);
    press(&mut b, KeyCode::Up);
    assert_eq!(press(&mut b, KeyCode::Right), BrowserAction::None);
    assert_eq!(b.cwd(), Path::new("/home/sd/dev/go"));
}

#[test]
fn the_cursor_cant_land_on_a_heading() {
    let mut b = with_favorites();
    for _ in 0..20 {
        press(&mut b, KeyCode::Up);
    }
    // Top of the list is the first favorite, not the "Favorites" title.
    press(&mut b, KeyCode::Enter);
    assert_eq!(b.cwd(), Path::new("/home/sd/dev/go"));
}

#[test]
fn ctrl_f_toggles_whatever_is_highlighted() {
    let mut b = browser();
    // A folder in the list.
    let toggle = |p: &str| BrowserAction::ToggleFavorite(PathBuf::from(p));
    assert_eq!(ctrl_f(&mut b), toggle("/home/sd/dev/api"));
    // The folder you're in.
    press(&mut b, KeyCode::Up);
    assert_eq!(ctrl_f(&mut b), toggle("/home/sd/dev"));

    // A favorite, which removes it.
    let mut b = with_favorites();
    for _ in 0..20 {
        press(&mut b, KeyCode::Up);
    }
    assert_eq!(ctrl_f(&mut b), toggle("/home/sd/dev/go"));

    // Typing f is still typing.
    let mut b = browser();
    assert_eq!(press(&mut b, KeyCode::Char('f')), BrowserAction::None);
}

#[test]
fn the_cursor_stays_put_when_favorites_change() {
    let mut b = browser();
    press(&mut b, KeyCode::Down); // on go
    b.set_favorites(vec![PathBuf::from("/tmp"), PathBuf::from("/home/sd")]);
    assert_eq!(press(&mut b, KeyCode::Right), BrowserAction::None);
    assert_eq!(b.cwd(), Path::new("/home/sd/dev/go"));
}

#[test]
fn favorites_filter_too_and_missing_ones_are_left_out() {
    let mut b = browser();
    b.set_favorites(vec![
        PathBuf::from("/home/sd/dev/go"),
        PathBuf::from("/tmp"),
        PathBuf::from("/unplugged-drive"),
    ]);
    assert_eq!(b.visible_favorites().len(), 2);
    typing(&mut b, "tmp");
    assert_eq!(b.visible_favorites(), [PathBuf::from("/tmp")]);
    assert_eq!(press(&mut b, KeyCode::Enter), BrowserAction::None);
    assert_eq!(b.cwd(), Path::new("/tmp"), "the only match is reachable");
}

#[test]
fn renders_favorites_at_the_top() {
    use ratatui::style::Color;
    let mut b = with_favorites();
    let mut term = Terminal::new(TestBackend::new(60, 15)).unwrap();
    term.draw(|f| b.draw(f)).unwrap();
    insta::assert_snapshot!(term.backend());
    for cell in &term.backend().buffer().content {
        for color in [cell.fg, cell.bg] {
            assert!(
                !matches!(color, Color::Rgb(..) | Color::Indexed(_)),
                "{color:?}"
            );
        }
    }
}

#[test]
fn escape_and_ctrl_c_close() {
    assert_eq!(press(&mut browser(), KeyCode::Esc), BrowserAction::Close);
    assert_eq!(
        browser().handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)),
        BrowserAction::Close
    );
}

#[test]
fn renders_with_only_terminal_colors() {
    use ratatui::style::Color;
    let mut b = browser();
    let mut term = Terminal::new(TestBackend::new(60, 10)).unwrap();
    term.draw(|f| b.draw(f)).unwrap();
    insta::assert_snapshot!(term.backend());
    for cell in &term.backend().buffer().content {
        for color in [cell.fg, cell.bg] {
            assert!(
                !matches!(color, Color::Rgb(..) | Color::Indexed(_)),
                "{color:?}"
            );
        }
    }
}
