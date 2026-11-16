//! The Reports popup: every claude's latest answer.

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use the_deaner_core::AgentState;
use the_deaner_ui::icons::UNICODE;
use the_deaner_ui::reports::{ReportEntry, Reports, ReportsAction, ago};

fn entry(address: &str, state: AgentState, answer: Option<&str>, at: Option<u64>) -> ReportEntry {
    ReportEntry {
        address: address.into(),
        session: address.split(':').next().unwrap().into(),
        window: 1,
        state,
        title: Some(format!("{address} work")),
        answer: answer.map(str::to_string),
        at,
    }
}

fn press(r: &mut Reports, code: KeyCode) -> ReportsAction {
    r.handle_key(KeyEvent::new(code, KeyModifiers::NONE))
}

fn render(r: &mut Reports) -> String {
    let mut term = Terminal::new(TestBackend::new(90, 16)).unwrap();
    term.draw(|f| r.draw(f)).unwrap();
    let buf = term.backend().buffer();
    (0..16)
        .map(|y| {
            (0..90)
                .map(|x| buf[(x, y)].symbol().to_string())
                .collect::<String>()
                + "\n"
        })
        .collect()
}

fn reports() -> Reports {
    let mut r = Reports::new(&UNICODE);
    r.set_entries(
        vec![
            entry(
                "tally:claude",
                AgentState::Idle,
                Some("Old news."),
                Some(1_000),
            ),
            entry(
                "dmi:claude",
                AgentState::Awaiting,
                Some("**Runner docs** updated.\n\nOne question for you."),
                Some(1_400),
            ),
            entry(
                "api:claude",
                AgentState::Busy,
                Some("Previous answer."),
                Some(1_200),
            ),
            entry("new:claude", AgentState::Idle, None, None),
        ],
        1_500,
    );
    r
}

#[test]
fn newest_answer_first_and_shown() {
    let mut r = reports();
    let order: Vec<&str> = r.entries().iter().map(|e| e.address.as_str()).collect();
    assert_eq!(
        order,
        ["dmi:claude", "api:claude", "tally:claude", "new:claude"]
    );
    let screen = render(&mut r);
    assert!(
        screen.contains("dmi:claude") && screen.contains("1m ago"),
        "{screen}"
    );
    assert!(
        screen.contains("Runner docs updated.") && screen.contains("One question"),
        "{screen}"
    );
    assert!(
        screen.contains("working") && screen.contains("no answer yet"),
        "{screen}"
    );
}

#[test]
fn a_working_claude_shows_its_previous_answer() {
    let mut r = reports();
    press(&mut r, KeyCode::Char('j'));
    let screen = render(&mut r);
    assert!(
        screen.contains("Working on something new") && screen.contains("Previous answer."),
        "{screen}"
    );
}

#[test]
fn the_selection_follows_its_claude_when_new_answers_arrive() {
    let mut r = reports();
    press(&mut r, KeyCode::Char('j')); // api
    let mut fresh: Vec<ReportEntry> = r.entries().to_vec();
    fresh.push(entry(
        "web:claude",
        AgentState::Idle,
        Some("Brand new."),
        Some(1_490),
    ));
    r.set_entries(fresh, 1_500);
    assert_eq!(r.entries()[0].address, "web:claude", "newest on top");
    assert_eq!(r.selected().unwrap().address, "api:claude", "still on api");
}

#[test]
fn enter_goes_to_it_and_esc_closes() {
    let mut r = reports();
    assert_eq!(
        press(&mut r, KeyCode::Enter),
        ReportsAction::Go("dmi".into(), 1)
    );
    assert_eq!(press(&mut r, KeyCode::Esc), ReportsAction::Close);
}

#[test]
fn empty_says_so_and_times_read_naturally() {
    let mut r = Reports::new(&UNICODE);
    assert!(render(&mut r).contains("No claudes running"));
    assert_eq!(ago(100, 90), "just now");
    assert_eq!(ago(1_000, 100), "15m ago");
    assert_eq!(ago(10_000, 100), "2h ago");
    assert_eq!(ago(300_000, 100), "3d ago");
}
