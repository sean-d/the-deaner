//! What each claude is working on: the name you gave the conversation with `/rename`,
//! or else the title Claude gave it (the one `/resume` lists).
//!
//! Both are lines in the transcript at
//! `<projects>/<encoded project path>/<session id>.jsonl`:
//!
//! - `{"type":"agent-name","agentName":"..."}`, written once by `/rename`;
//! - `{"type":"ai-title","aiTitle":"..."}`, repeated as the conversation goes on.
//!
//! Only the tail of the file is read, and only when it has grown. A rename can scroll
//! out of that tail, so a new entry also checks Claude's session files
//! (`<sessions>/<pid>.json`), where a user-given name has `"nameSource": "user"`.

use serde_json::Value;
use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// How much of the end of a transcript to look through.
const TAIL: u64 = 512 * 1024;

/// Titles by session id, remembered between refreshes.
pub struct Titles {
    projects: PathBuf,
    sessions: PathBuf,
    known: HashMap<String, Entry>,
}

#[derive(Default)]
struct Entry {
    path: Option<PathBuf>,
    len: u64,
    /// From `/rename`: wins over the automatic title, and stays once seen.
    named: Option<String>,
    automatic: Option<String>,
    prompt: Option<String>,
}

/// What one read of a transcript's tail found.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Found {
    pub named: Option<String>,
    pub automatic: Option<String>,
    /// The last thing the conversation was asked, for when it has no title.
    pub prompt: Option<String>,
}

impl Titles {
    /// Look in Claude's `projects` and `sessions` folders (beside `settings.json`, so
    /// `CLAUDE_CONFIG_DIR` is honoured).
    #[must_use]
    pub fn for_claude() -> Self {
        let settings = crate::hooks::claude_settings_path();
        Self::new(
            settings.with_file_name("projects"),
            settings.with_file_name("sessions"),
        )
    }

    #[must_use]
    pub fn new(projects: PathBuf, sessions: PathBuf) -> Self {
        Self {
            projects,
            sessions,
            known: HashMap::new(),
        }
    }

    /// The conversation's name or title, if it has one yet.
    pub fn get(&mut self, session: &str) -> Option<String> {
        if session.is_empty() || session.contains(['/', '\\']) || session.starts_with('.') {
            return None;
        }
        let entry = match self.known.entry(session.to_string()) {
            std::collections::hash_map::Entry::Occupied(e) => e.into_mut(),
            std::collections::hash_map::Entry::Vacant(e) => e.insert(Entry {
                named: user_name(&self.sessions, session),
                ..Entry::default()
            }),
        };
        if entry.path.is_none() {
            // A new conversation's file shows up after its first message.
            entry.path = find(&self.projects, session);
        }
        if let Some(path) = &entry.path
            && let Ok(meta) = std::fs::metadata(path)
            && meta.len() != entry.len
        {
            entry.len = meta.len();
            let found = read_tail(path, entry.len);
            entry.named = found.named.or(entry.named.take());
            entry.automatic = found.automatic.or(entry.automatic.take());
            entry.prompt = found.prompt.or(entry.prompt.take());
        }
        entry
            .named
            .clone()
            .or_else(|| entry.automatic.clone())
            .or_else(|| entry.prompt.clone())
    }
}

fn find(projects: &Path, session: &str) -> Option<PathBuf> {
    let file = format!("{session}.jsonl");
    std::fs::read_dir(projects)
        .ok()?
        .filter_map(Result::ok)
        .map(|e| e.path().join(&file))
        .find(|p| p.is_file())
}

/// A name the user gave this conversation, from Claude's session files.
fn user_name(sessions: &Path, session: &str) -> Option<String> {
    std::fs::read_dir(sessions)
        .ok()?
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
        .find_map(|e| {
            let v: Value = serde_json::from_slice(&std::fs::read(e.path()).ok()?).ok()?;
            let ours = v.get("sessionId")?.as_str()? == session
                && v.get("nameSource")?.as_str()? == "user";
            ours.then(|| text(&v, "name")).flatten()
        })
}

fn read_tail(path: &Path, len: u64) -> Found {
    let read = || -> Option<String> {
        let mut file = File::open(path).ok()?;
        file.seek(SeekFrom::Start(len.saturating_sub(TAIL))).ok()?;
        let mut buf = Vec::new();
        file.take(TAIL).read_to_end(&mut buf).ok()?;
        Some(String::from_utf8_lossy(&buf).into_owned())
    };
    read().map(|t| latest(&t)).unwrap_or_default()
}

/// The newest `/rename` name, automatic title and prompt in these transcript lines.
/// A partial first line (the tail starts mid-line) just doesn't parse.
#[must_use]
pub fn latest(text_lines: &str) -> Found {
    let mut found = Found::default();
    for line in text_lines.lines().rev() {
        if found.named.is_some() && found.automatic.is_some() && found.prompt.is_some() {
            break;
        }
        let named = found.named.is_none() && line.contains("\"agent-name\"");
        let automatic = found.automatic.is_none() && line.contains("\"ai-title\"");
        let prompt = found.prompt.is_none()
            && (line.contains("\"last-prompt\"") || line.contains("\"type\":\"user\""));
        if !named && !automatic && !prompt {
            continue;
        }
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        match v.get("type").and_then(Value::as_str) {
            Some("agent-name") if named => found.named = text(&v, "agentName"),
            Some("ai-title") if automatic => found.automatic = text(&v, "aiTitle"),
            Some("last-prompt") if prompt => found.prompt = text(&v, "lastPrompt").map(typed),
            Some("user") if prompt => found.prompt = asked(&v),
            _ => {}
        }
    }
    found
}

/// `› first line` of something you typed.
fn typed(prompt: String) -> String {
    format!("› {}", first_line(&prompt))
}

/// What a user message asked: your own words (`› …`), or a message from another claude
/// (`⇄ its-name: …`). Tool results, slash commands and other bookkeeping don't count.
fn asked(v: &Value) -> Option<String> {
    if v.get("isSidechain").and_then(Value::as_bool) == Some(true) {
        return None;
    }
    let content = v.get("message")?.get("content")?;
    let text = match content {
        Value::String(s) => s.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter(|p| p.get("type").and_then(Value::as_str) == Some("text"))
            .filter_map(|p| p.get("text")?.as_str())
            .collect::<Vec<_>>()
            .join("\n"),
        _ => return None,
    };
    if let Some(start) = text.find("<cross-session-message") {
        let tag = &text[start..];
        let from = tag
            .split_once("from-name=\"")
            .and_then(|(_, rest)| rest.split_once('"'))
            .map_or("another claude", |(name, _)| name);
        let body = tag.split_once('>')?.1;
        let body = body
            .split("</cross-session-message>")
            .next()
            .unwrap_or(body);
        return Some(format!("⇄ {from}: {}", first_line(body)));
    }
    if v.get("isMeta").and_then(Value::as_bool) == Some(true) {
        return None;
    }
    let mut own = text.as_str().to_string();
    while let Some(start) = own.find("<system-reminder>") {
        let end = own[start..]
            .find("</system-reminder>")
            .map_or(own.len(), |e| start + e + "</system-reminder>".len());
        own.replace_range(start..end, "");
    }
    let own = own.trim();
    (!own.is_empty() && !own.starts_with('<')).then(|| typed(own.to_string()))
}

/// The first non-empty line, with runs of whitespace squeezed to one space.
fn first_line(text: &str) -> String {
    let line = text
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("");
    line.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn text(v: &Value, key: &str) -> Option<String> {
    let s = v.get(key)?.as_str()?.trim();
    (!s.is_empty()).then(|| s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(kind: &str, key: &str, value: &str) -> String {
        format!("{{\"type\":\"{kind}\",\"{key}\":\"{value}\",\"sessionId\":\"s\"}}\n")
    }
    fn title(t: &str) -> String {
        line("ai-title", "aiTitle", t)
    }
    fn rename(t: &str) -> String {
        line("agent-name", "agentName", t)
    }

    #[test]
    fn newest_of_each_and_other_lines_are_ignored() {
        let text = [
            "ld\":\"a partial line from the middle of the file\"}\n".to_string(),
            title("First idea"),
            r#"{"type":"user","message":{"content":"mentions \"ai-title\" in passing"}}"#.into(),
            "\n".into(),
            title("  Sidebar section spacing  "),
            r#"{"type":"last-prompt","lastPrompt":"commit"}"#.into(),
        ]
        .concat();
        assert_eq!(
            latest(&text),
            Found {
                named: None,
                automatic: Some("Sidebar section spacing".into()),
                prompt: Some("› commit".into()),
            }
        );
        let renamed = [title("Old"), rename("my name"), title("Newer")].concat();
        assert_eq!(latest(&renamed).named.as_deref(), Some("my name"));
        assert_eq!(latest(&renamed).automatic.as_deref(), Some("Newer"));
        assert_eq!(latest(&title(" ")), Found::default());
    }

    #[test]
    fn follows_the_transcript_and_a_rename_wins_and_sticks() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("projects/-home-me-dev-app");
        std::fs::create_dir_all(&project).unwrap();
        let sessions = dir.path().join("sessions");
        std::fs::create_dir_all(&sessions).unwrap();
        let file = project.join("abc-123.jsonl");
        let mut titles = Titles::new(dir.path().join("projects"), sessions);

        assert_eq!(titles.get("abc-123"), None, "no transcript yet");
        std::fs::write(&file, "{\"type\":\"user\"}\n").unwrap();
        assert_eq!(titles.get("abc-123"), None, "no title yet");

        let mut text = title("Fix login");
        std::fs::write(&file, &text).unwrap();
        assert_eq!(titles.get("abc-123").as_deref(), Some("Fix login"));

        text.push_str(&rename("login work"));
        text.push_str(&title("Fix login and tests"));
        std::fs::write(&file, &text).unwrap();
        assert_eq!(titles.get("abc-123").as_deref(), Some("login work"));

        // Grows past the rename: still the name.
        text.push_str(&"{\"type\":\"user\"}\n".repeat(40_000));
        text.push_str(&title("Later"));
        std::fs::write(&file, &text).unwrap();
        assert!(
            latest(&text[text.len() - 512 * 1024..]).named.is_none(),
            "out of the tail"
        );
        assert_eq!(titles.get("abc-123").as_deref(), Some("login work"));

        assert_eq!(titles.get("../etc/passwd"), None);
    }

    #[test]
    fn without_a_title_the_last_prompt_or_message_from_another_claude() {
        let user = |content: &str| {
            format!(
                "{{\"type\":\"user\",\"message\":{{\"role\":\"user\",\"content\":{content}}}}}\n"
            )
        };
        let typed = user(r#""fix the\n\n  login   redirect\nplease""#);
        let tool = user(r#"[{"type":"tool_result","content":"ok"}]"#);
        let command = user(r#""<command-name>/rename</command-name>""#);
        let reminder =
            user(r#"[{"type":"text","text":"<system-reminder>x</system-reminder>\nship it"}]"#);
        let cross = format!(
            "{{\"type\":\"user\",\"isMeta\":true,\"message\":{{\"content\":{}}}}}\n",
            serde_json::to_string(
                "Another Claude session sent a message:\n<cross-session-message from=\"uds:/x\" \
                 from-name=\"infra-ee\" from-mode=\"prompting\">\nThanks. Noted: the PR\nmore\n\
                 </cross-session-message>\n\nThis came from another Claude session"
            )
            .unwrap()
        );

        let found = |lines: &[&String]| {
            latest(&lines.iter().map(|s| s.as_str()).collect::<String>()).prompt
        };
        assert_eq!(
            found(&[&typed, &tool, &command]).as_deref(),
            Some("› fix the")
        );
        assert_eq!(
            found(&[&typed, &reminder, &tool]).as_deref(),
            Some("› ship it")
        );
        assert_eq!(
            found(&[&typed, &cross, &tool]).as_deref(),
            Some("⇄ infra-ee: Thanks. Noted: the PR")
        );
        let with_text =
            r#"{"type":"last-prompt","lastPrompt":"commit and push"}"#.to_string() + "\n";
        assert_eq!(
            found(&[&cross, &with_text]).as_deref(),
            Some("› commit and push")
        );
        let empty = r#"{"type":"last-prompt","leafUuid":"x"}"#.to_string() + "\n";
        assert_eq!(found(&[&typed, &empty]).as_deref(), Some("› fix the"));

        // A title, when there is one, wins.
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("projects/p");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(
            project.join("s.jsonl"),
            [typed.clone(), title("Login fix")].concat(),
        )
        .unwrap();
        let mut titles = Titles::new(dir.path().join("projects"), dir.path().join("sessions"));
        assert_eq!(titles.get("s").as_deref(), Some("Login fix"));
        std::fs::write(project.join("t.jsonl"), &typed).unwrap();
        assert_eq!(titles.get("t").as_deref(), Some("› fix the"));
    }

    #[test]
    fn a_rename_out_of_reach_is_found_in_the_session_files() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("projects/p");
        let sessions = dir.path().join("sessions");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::create_dir_all(&sessions).unwrap();
        std::fs::write(project.join("abc.jsonl"), title("Automatic")).unwrap();
        std::fs::write(
            sessions.join("111.json"),
            r#"{"sessionId":"other","name":"not this","nameSource":"user"}"#,
        )
        .unwrap();
        std::fs::write(
            sessions.join("222.json"),
            r#"{"sessionId":"abc","name":"my name","nameSource":"user"}"#,
        )
        .unwrap();
        std::fs::write(
            sessions.join("333.json"),
            r#"{"sessionId":"def","name":"def-00","nameSource":"derived"}"#,
        )
        .unwrap();
        std::fs::write(project.join("def.jsonl"), title("Automatic")).unwrap();

        let mut titles = Titles::new(dir.path().join("projects"), sessions);
        assert_eq!(titles.get("abc").as_deref(), Some("my name"));
        assert_eq!(
            titles.get("def").as_deref(),
            Some("Automatic"),
            "a derived name isn't a name"
        );
    }
}
