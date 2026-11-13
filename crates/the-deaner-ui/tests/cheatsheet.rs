//! The shortcut popup: search behaviour and rendering.

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use the_deaner_core::config::FrameKeys;
use the_deaner_core::tmux_keys::TmuxShortcut;
use the_deaner_ui::cheatsheet::{Cheatsheet, Entry, entries};
use the_deaner_ui::icons::UNICODE;

fn sheet() -> Cheatsheet {
    let tmux = vec![
        TmuxShortcut {
            keys: "Ctrl-Space c".into(),
            what: "New tab",
            terms: "window add open create right",
        },
        TmuxShortcut {
            keys: "Alt-Right or Ctrl-Space n".into(),
            what: "Next tab",
            terms: "window switch navigate move go right",
        },
        TmuxShortcut {
            keys: "Ctrl-h/j/k/l".into(),
            what: "Move between panes",
            terms: "split navigate switch focus left right up down",
        },
    ];
    Cheatsheet::new(entries(&FrameKeys::default(), &tmux, &UNICODE))
}

fn press(s: &mut Cheatsheet, code: KeyCode) -> bool {
    s.handle_key(KeyEvent::new(code, KeyModifiers::NONE))
}

fn search(s: &mut Cheatsheet, text: &str) {
    for c in text.chars() {
        assert!(!press(s, KeyCode::Char(c)));
    }
}

fn found(s: &Cheatsheet) -> Vec<Entry> {
    s.visible().into_iter().cloned().collect()
}

fn answers(question: &str, keys: &str) {
    let mut s = sheet();
    search(&mut s, question);
    let hits = found(&s);
    assert!(
        hits.iter().any(|e| e.keys == keys),
        "{question:?} should find {keys}: {hits:#?}"
    );
}

#[test]
fn answers_the_questions_people_actually_ask() {
    answers("how do i hide the lefthand side panel", "Alt-e");
    answers("hide", "Alt-e");
    answers("projects", "Alt-e");
    answers("explorer", "Alt-e");
    answers("how do i navigate between left and right", "Alt-s");
    answers("open a new tab on the right side", "t");
    answers("new tab", "Ctrl-Space c");
    answers("split", "Ctrl-h/j/k/l");
    answers("pause", "P");
    answers("how do i open a project", "a");
    answers("select a project from a file browser", "a");
    answers("how do i navigate the tabs", "Alt-Right or Ctrl-Space n");
    answers("next tab", "Alt-Right or Ctrl-Space n");
    answers("how do i open neovim", "t");
}

#[test]
fn searches_are_narrow_and_can_come_up_empty() {
    let mut s = sheet();
    let all = found(&s).len();
    search(&mut s, "hide");
    assert!(found(&s).len() < 3 && all > 20);
    let mut s = sheet();
    search(&mut s, "zzzz");
    assert!(found(&s).is_empty());
}

#[test]
fn closing_keys() {
    assert!(press(&mut sheet(), KeyCode::Esc));
    assert!(press(&mut sheet(), KeyCode::Enter));
    assert!(sheet().handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::ALT)));
    assert!(sheet().handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)));
    assert!(
        !press(&mut sheet(), KeyCode::Char('c')),
        "plain c is typing"
    );
}

#[test]
fn says_so_when_tmux_keys_cant_be_read() {
    let all = entries(&FrameKeys::default(), &[], &UNICODE);
    assert!(all.iter().any(|e| e.what.contains("Couldn't read")));
}

fn render(s: &mut Cheatsheet, h: u16) -> String {
    let mut term = Terminal::new(TestBackend::new(70, h)).unwrap();
    term.draw(|f| s.draw(f)).unwrap();
    term.backend().to_string()
}

#[test]
fn renders_everything_and_a_search() {
    let mut s = sheet();
    insta::assert_snapshot!("all", render(&mut s, 24));
    search(&mut s, "how do i hide");
    insta::assert_snapshot!("search", render(&mut s, 8));
}

#[test]
fn scrolling_stops_at_the_end() {
    let mut s = sheet();
    for _ in 0..500 {
        press(&mut s, KeyCode::Down);
    }
    let screen = render(&mut s, 12);
    assert!(
        screen.contains("Parked"),
        "last entry is on screen: {screen}"
    );
}

/// Fails on any color outside the-deaner's palette.
fn assert_palette(buffer: &ratatui::buffer::Buffer, what: &str) {
    for cell in &buffer.content {
        for color in [cell.fg, cell.bg] {
            assert!(
                the_deaner_ui::theme::colors::ALL.contains(&color),
                "{what} paints {color:?}, outside the palette, at {:?}",
                cell.symbol()
            );
        }
    }
}

#[test]
fn popup_only_uses_the_palette() {
    let mut s = sheet();
    let mut term = Terminal::new(TestBackend::new(70, 30)).unwrap();
    term.draw(|f| s.draw(f)).unwrap();
    assert_palette(term.backend().buffer(), "empty popup");
    search(&mut s, "tab");
    term.draw(|f| s.draw(f)).unwrap();
    assert_palette(term.backend().buffer(), "popup with a search");
    search(&mut s, "zzz");
    term.draw(|f| s.draw(f)).unwrap();
    assert_palette(term.backend().buffer(), "popup with no results");
}

#[test]
fn shows_the_running_version_in_the_corner() {
    assert!(!render(&mut sheet(), 20).contains("v9.8.7"));
    let mut s = sheet().with_version("9.8.7");
    let screen = render(&mut s, 20);
    let last = screen.lines().last().unwrap();
    assert!(
        last.contains("Esc closes") && last.contains("v9.8.7"),
        "{screen}"
    );
}
