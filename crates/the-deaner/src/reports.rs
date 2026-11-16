//! `the-deaner reports` (`o` in the sidebar): every claude's latest answer, live.
//!
//! An answer is the last thing claude said to end a turn: in its transcript, the newest
//! `assistant` entry with text and `stop_reason: end_turn` that isn't a subagent's
//! (`isSidechain`). Showing one stamps its workspace as read (`@the_deaner_read`), which
//! clears the sidebar's "needs you" mark.

use crate::engine::Engine;
use anyhow::Result;
use ratatui::crossterm::event::{self, Event};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use the_deaner_core::pane::OPT_READ;
use the_deaner_tmux::{Scope, window_target};
use the_deaner_ui::reports::{ReportEntry, Reports, ReportsAction};

/// An answer's text, and when it was written (unix seconds).
pub type Answer = (String, Option<u64>);

/// How much of the end of a transcript to look through first, and at most.
const TAIL: u64 = 1024 * 1024;
const MAX_TAIL: u64 = 16 * 1024 * 1024;
/// How often the popup looks for new answers.
const REFRESH: Duration = Duration::from_secs(2);

/// The latest finished answer in these transcript lines, and when it was written (unix
/// seconds). A partial first line (the tail starts mid-line) just doesn't parse.
#[must_use]
pub fn latest_answer(lines: &str) -> Option<Answer> {
    for line in lines.lines().rev() {
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if v["type"] != "assistant" || v["isSidechain"] == true {
            continue;
        }
        if v["message"]["stop_reason"] != "end_turn" {
            continue;
        }
        let text: Vec<&str> = v["message"]["content"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|c| c["type"] == "text")
            .filter_map(|c| c["text"].as_str())
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .collect();
        if text.is_empty() {
            continue;
        }
        let at = v["timestamp"].as_str().and_then(parse_time);
        return Some((text.join("\n\n"), at));
    }
    None
}

/// `2026-11-16T20:14:31.502Z` as unix seconds.
#[must_use]
pub fn parse_time(ts: &str) -> Option<u64> {
    let (date, time) = ts.split_once('T')?;
    let mut d = date.split('-').map(str::parse::<i64>);
    let (y, m, day) = (d.next()?.ok()?, d.next()?.ok()?, d.next()?.ok()?);
    let time = time.trim_end_matches('Z');
    let mut t = time.split(':');
    let (h, min) = (
        t.next()?.parse::<i64>().ok()?,
        t.next()?.parse::<i64>().ok()?,
    );
    let sec = t.next()?.split('.').next()?.parse::<i64>().ok()?;
    // Days from 1970-01-01 to y-m-d (proleptic Gregorian), Howard Hinnant's algorithm.
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    u64::try_from(days * 86_400 + h * 3_600 + min * 60 + sec).ok()
}

fn read_tail(path: &Path, len: u64, take: u64) -> Option<String> {
    let mut file = std::fs::File::open(path).ok()?;
    file.seek(SeekFrom::Start(len.saturating_sub(take))).ok()?;
    let mut buf = Vec::new();
    file.take(take).read_to_end(&mut buf).ok()?;
    Some(String::from_utf8_lossy(&buf).into_owned())
}

/// Answers by transcript, re-read only when the file has grown.
#[derive(Default)]
struct Cache {
    answers: HashMap<PathBuf, (u64, Option<Answer>)>,
}

impl Cache {
    fn answer(&mut self, path: &Path) -> Option<Answer> {
        let len = std::fs::metadata(path).ok()?.len();
        if let Some((seen, a)) = self.answers.get(path)
            && *seen == len
        {
            return a.clone();
        }
        // The end of the file first; a long stretch of tool output since the last answer
        // means looking further back.
        let mut take = TAIL;
        let found = loop {
            let found = read_tail(path, len, take).and_then(|t| latest_answer(&t));
            if found.is_some() || take >= len || take >= MAX_TAIL {
                break found;
            }
            take *= 4;
        };
        self.answers
            .insert(path.to_path_buf(), (len, found.clone()));
        found
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis())
}

/// Every running claude: its address, state, title and latest answer.
fn collect(
    engine: &Engine,
    cache: &mut Cache,
    titles: &mut crate::titles::Titles,
    projects: &Path,
) -> Vec<ReportEntry> {
    let mut out: Vec<ReportEntry> = Vec::new();
    for p in engine.managed_panes() {
        if p.agent.as_deref() != Some("claude") {
            continue;
        }
        let address = p.address();
        if out.iter().any(|e| e.address == address) {
            continue;
        }
        let (answer, at) = p
            .agent_session
            .as_deref()
            .and_then(|s| crate::titles::find(projects, s))
            .and_then(|path| cache.answer(&path))
            .map_or((None, None), |(a, at)| (Some(a), at));
        out.push(ReportEntry {
            title: p.agent_session.as_deref().and_then(|s| titles.get(s)),
            address,
            session: p.session.clone(),
            window: p.window_index,
            state: p.state,
            answer,
            at,
        });
    }
    out
}

/// Run until closed.
///
/// # Errors
/// When the terminal can't be set up.
pub fn run(engine: &Engine) -> Result<()> {
    let icons = the_deaner_ui::icons::for_style(engine.config.icons);
    let mut reports = Reports::new(icons).with_version(crate::upgrade::CURRENT);
    let mut cache = Cache::default();
    let mut titles = crate::titles::Titles::for_claude();
    let projects = crate::hooks::claude_settings_path().with_file_name("projects");
    // What's been stamped read already: (session, answer time).
    let mut stamped: HashSet<(String, Option<u64>)> = HashSet::new();
    let mut terminal = ratatui::init();
    let result = (|| -> Result<Option<(String, u32)>> {
        let mut refreshed = Instant::now() - REFRESH;
        loop {
            if refreshed.elapsed() >= REFRESH {
                reports.set_entries(
                    collect(engine, &mut cache, &mut titles, &projects),
                    now_secs(),
                );
                refreshed = Instant::now();
            }
            // Showing an answer counts as reading it: stamp its workspace once per answer.
            if let Some(e) = reports.selected()
                && e.answer.is_some()
                && stamped.insert((e.session.clone(), e.at))
            {
                let _ = engine.tmux.set_option(
                    Scope::Session(&e.session),
                    OPT_READ,
                    &now_ms().to_string(),
                );
            }
            terminal.draw(|f| reports.draw(f))?;
            if !event::poll(Duration::from_millis(250))? {
                continue;
            }
            if let Event::Key(key) = event::read()? {
                match reports.handle_key(key) {
                    ReportsAction::Close => return Ok(None),
                    ReportsAction::Go(session, window) => return Ok(Some((session, window))),
                    ReportsAction::None => {}
                }
            }
        }
    })();
    ratatui::restore();
    if let Some((session, window)) = result? {
        let switcher = crate::frame::Switcher::detect(engine);
        if let Some(hint) = switcher.show(engine, &session)? {
            println!("{hint}");
        }
        let _ = engine.tmux.select_window(&window_target(&session, window));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(kind: &str, stop: &str, content: &str, extra: &str) -> String {
        format!(
            r#"{{"type":"{kind}","timestamp":"2026-11-16T20:14:31.502Z"{extra},"message":{{"stop_reason":"{stop}","content":{content}}}}}"#
        )
    }

    #[test]
    fn the_answer_is_the_last_end_of_turn_text_from_claude_itself() {
        let lines = [
            entry(
                "assistant",
                "end_turn",
                r#"[{"type":"text","text":"Old answer."}]"#,
                "",
            ),
            entry("user", "", r#"[{"type":"text","text":"do more"}]"#, ""),
            entry(
                "assistant",
                "tool_use",
                r#"[{"type":"text","text":"Let me check."},{"type":"tool_use"}]"#,
                "",
            ),
            entry(
                "assistant",
                "end_turn",
                r#"[{"type":"thinking"},{"type":"text","text":"**Done**\n\nAll good."}]"#,
                "",
            ),
            entry(
                "assistant",
                "end_turn",
                r#"[{"type":"text","text":"A subagent's answer."}]"#,
                r#","isSidechain":true"#,
            ),
            entry("assistant", "end_turn", r#"[{"type":"thinking"}]"#, ""),
            "{partial line".to_string(),
        ]
        .join("\n");
        let (text, at) = latest_answer(&lines).unwrap();
        assert_eq!(text, "**Done**\n\nAll good.");
        assert_eq!(at, Some(1_794_860_071));
        assert!(latest_answer("nothing useful\n").is_none());
    }

    #[test]
    fn times_parse_to_unix_seconds() {
        assert_eq!(parse_time("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(parse_time("2000-03-01T00:00:00.000Z"), Some(951_868_800));
        assert_eq!(parse_time("2026-11-16T20:14:31.502Z"), Some(1_794_860_071));
        assert_eq!(parse_time("garbage"), None);
    }
}
