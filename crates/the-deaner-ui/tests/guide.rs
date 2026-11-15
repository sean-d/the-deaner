//! The built-in guide: pages, reading, search.

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use the_deaner_ui::guide::{Guide, GuideAction, GuidePage};

const ONE: &str = "# One\n\nFirst page.\n";
fn two() -> String {
    let mut s = String::from("# Two\n\n");
    for i in 0..60 {
        s.push_str(&format!("Line {i} of the second page.\n\n"));
    }
    s.push_str("The needle is here.\n");
    s
}

fn guide() -> Guide {
    let two: &'static str = Box::leak(two().into_boxed_str());
    Guide::new(vec![
        GuidePage {
            slug: "one",
            title: "First",
            source: ONE,
        },
        GuidePage {
            slug: "two",
            title: "Second",
            source: two,
        },
    ])
}

fn press(g: &mut Guide, code: KeyCode) -> GuideAction {
    g.handle_key(KeyEvent::new(code, KeyModifiers::NONE))
}

fn render(g: &mut Guide) -> String {
    let mut term = Terminal::new(TestBackend::new(70, 20)).unwrap();
    term.draw(|f| g.draw(f)).unwrap();
    let buf = term.backend().buffer();
    (0..20)
        .map(|y| {
            (0..70)
                .map(|x| buf[(x, y)].symbol().to_string())
                .collect::<String>()
                + "\n"
        })
        .collect()
}

#[test]
fn the_list_opens_pages_and_esc_goes_back() {
    let mut g = guide();
    assert!(render(&mut g).contains("Second"));
    press(&mut g, KeyCode::Char('j'));
    press(&mut g, KeyCode::Enter);
    assert_eq!(g.page(), Some("two"));
    assert!(render(&mut g).contains("Line 0 of the second page"));
    press(&mut g, KeyCode::Esc);
    assert_eq!(g.page(), None);
    assert_eq!(press(&mut g, KeyCode::Esc), GuideAction::Close);
}

#[test]
fn tab_goes_to_the_keys_and_pages_open_by_name() {
    let mut g = guide();
    assert_eq!(press(&mut g, KeyCode::Tab), GuideAction::Keys);
    assert!(g.open("sec"), "start of a title");
    assert_eq!(g.page(), Some("two"));
    assert!(!g.open("nope"));
    assert_eq!(g.slugs(), ["one", "two"]);
}

#[test]
fn scrolling_stops_at_the_end_and_brackets_change_page() {
    let mut g = guide();
    g.open("two");
    render(&mut g);
    press(&mut g, KeyCode::Char('G'));
    let end = g.scroll();
    assert!(end > 0);
    press(&mut g, KeyCode::Char('j'));
    assert_eq!(g.scroll(), end, "no scrolling past the last line");
    assert!(render(&mut g).contains("The needle is here."));
    press(&mut g, KeyCode::Char('['));
    assert_eq!(g.page(), Some("one"));
    press(&mut g, KeyCode::Char(']'));
    assert_eq!(g.page(), Some("two"));
}

#[test]
fn slash_searches_the_page() {
    let mut g = guide();
    g.open("two");
    render(&mut g);
    press(&mut g, KeyCode::Char('/'));
    for c in "needle".chars() {
        press(&mut g, KeyCode::Char(c));
    }
    press(&mut g, KeyCode::Enter);
    assert!(g.scroll() > 50, "jumped down to the match");
    assert!(render(&mut g).contains("The needle is here."));
    press(&mut g, KeyCode::Char('/'));
    for c in "zzz".chars() {
        press(&mut g, KeyCode::Char(c));
    }
    press(&mut g, KeyCode::Enter);
    assert!(render(&mut g).contains("no match for \"zzz\""));
}
