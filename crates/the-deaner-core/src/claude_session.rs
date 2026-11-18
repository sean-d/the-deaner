//! Agent status without hooks: Claude Code's own session files.
//!
//! Each running claude writes `~/.claude/sessions/<pid>.json` with its status and the
//! tmux pane it runs in (`"tmux": "<session>:@<window>.%<pane>"`). Hooks stay the main
//! source; a file fills in a pane the hooks have said nothing about, or have gone quiet on
//! (the file changed after the last hook: the hooks were removed while claude ran), so the
//! sidebar still shows what claude is doing where the hooks aren't installed.
//!
//! The format is undocumented. Files outlive their claude and pane ids get reused, so a
//! file counts only when its process is alive and runs on that pane's terminal. Anything
//! that doesn't parse or match is ignored, which leaves the pane as it would be anyway.

use crate::agent::AgentState;
use crate::pane::PaneInfo;
use serde_json::Value;
use std::collections::HashMap;

/// What one session file says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaudeSession {
    pub pid: u32,
    /// The conversation id, used for titles, Reports and resuming after a restore.
    pub session_id: Option<String>,
    pub state: AgentState,
    /// When the status last changed (unix ms), 0 when the file doesn't say.
    pub updated_at: u64,
    /// The tmux session and pane id it runs in.
    pub tmux_session: String,
    pub pane_id: String,
}

impl ClaudeSession {
    /// Parse a session file. `None` when it isn't one, or doesn't say where it runs.
    #[must_use]
    pub fn parse(json: &str) -> Option<Self> {
        let v: Value = serde_json::from_str(json).ok()?;
        let pid = u32::try_from(v["pid"].as_u64()?).ok()?;
        // "leka:@29.%29"
        let (session, rest) = v["tmux"].as_str()?.split_once(':')?;
        let (_window, pane) = rest.split_once('.')?;
        if session.is_empty() || !pane.starts_with('%') {
            return None;
        }
        Some(Self {
            pid,
            session_id: v["sessionId"]
                .as_str()
                .filter(|s| !s.is_empty())
                .map(str::to_string),
            state: status(v["status"].as_str().unwrap_or_default()),
            updated_at: v["statusUpdatedAt"]
                .as_u64()
                .or_else(|| v["updatedAt"].as_u64())
                .unwrap_or(0),
            tmux_session: session.to_string(),
            pane_id: pane.to_string(),
        })
    }
}

/// Claude's status values. `shell` is claude idle while a `!` command runs. A value this
/// doesn't know still means claude is running there, so it reads as idle.
fn status(s: &str) -> AgentState {
    match s {
        "busy" => AgentState::Busy,
        "waiting" => AgentState::Awaiting,
        _ => AgentState::Idle,
    }
}

/// Pairs of (pane index, session index) worth checking: each session file naming a pane
/// that isn't showing a shell, where the hooks have said nothing, or said it before the
/// file last changed.
#[must_use]
pub fn candidates(
    panes: &[PaneInfo],
    sessions: &[ClaudeSession],
    shells: &[String],
) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    for (pi, p) in panes.iter().enumerate() {
        let hooked = p.state != AgentState::None || p.agent.is_some();
        if shells.contains(&p.current_command) || p.agent.as_deref().is_some_and(|a| a != "claude")
        {
            continue;
        }
        for (si, s) in sessions.iter().enumerate() {
            let newer = !hooked || s.updated_at > p.state_at;
            if newer && s.tmux_session == p.session && s.pane_id == p.pane_id {
                out.push((pi, si));
            }
        }
    }
    out
}

/// Fill in panes from session files. `ttys` gets the pids worth checking and returns the
/// terminal each one that is still running is on (`pts/4`, `ttys004`); it's only called
/// when there's something to check. A file counts only when its process is on that pane's
/// terminal.
pub fn fill(
    panes: &mut [PaneInfo],
    sessions: &[ClaudeSession],
    shells: &[String],
    ttys: impl FnOnce(&[u32]) -> HashMap<u32, String>,
) {
    let pairs = candidates(panes, sessions, shells);
    if pairs.is_empty() {
        return;
    }
    let mut pids: Vec<u32> = pairs.iter().map(|&(_, si)| sessions[si].pid).collect();
    pids.dedup();
    let running = ttys(&pids);
    for (pi, si) in pairs {
        let s = &sessions[si];
        let p = &mut panes[pi];
        let on_pane = running.get(&s.pid).is_some_and(|tty| {
            let tty = tty.trim();
            !tty.is_empty() && p.tty.strip_prefix("/dev/").unwrap_or(&p.tty) == tty
        });
        if on_pane {
            p.state = s.state;
            p.agent = Some("claude".into());
            if p.agent_session.is_none() {
                p.agent_session.clone_from(&s.session_id);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pane::sample_line;

    fn pane(session: &str, id: &str, tty: &str, command: &str) -> PaneInfo {
        let mut p = PaneInfo::parse_line(&sample_line(session, 1, "claude", "")).unwrap();
        p.pane_id = id.into();
        p.tty = tty.into();
        p.current_command = command.into();
        p.agent = None;
        p.agent_session = None;
        p.state_at = 0;
        p
    }

    fn file(pid: u32, tmux: &str, status: &str) -> ClaudeSession {
        file_at(pid, tmux, status, 0)
    }

    fn file_at(pid: u32, tmux: &str, status: &str, at: u64) -> ClaudeSession {
        ClaudeSession::parse(&format!(
            r#"{{"pid":{pid},"sessionId":"conv-{pid}","tmux":"{tmux}","status":"{status}","waitingFor":null,"statusUpdatedAt":{at}}}"#
        ))
        .unwrap()
    }

    fn shells() -> Vec<String> {
        vec!["bash".into(), "zsh".into()]
    }

    #[test]
    fn parses_what_claude_writes() {
        let s = ClaudeSession::parse(
            r#"{"pid":34477,"sessionId":"2e0f","cwd":"/x","status":"waiting","waitingFor":"permission prompt","tmux":"leka:@29.%29","name":"leka-90"}"#,
        )
        .unwrap();
        assert_eq!(s.pid, 34477);
        assert_eq!(s.session_id.as_deref(), Some("2e0f"));
        assert_eq!(s.state, AgentState::Awaiting);
        assert_eq!(
            (s.tmux_session.as_str(), s.pane_id.as_str()),
            ("leka", "%29")
        );

        assert_eq!(file(1, "a:@1.%1", "busy").state, AgentState::Busy);
        assert_eq!(file(1, "a:@1.%1", "idle").state, AgentState::Idle);
        assert_eq!(file(1, "a:@1.%1", "shell").state, AgentState::Idle);
        assert_eq!(file(1, "a:@1.%1", "something-new").state, AgentState::Idle);
    }

    #[test]
    fn files_that_dont_say_where_they_run_are_ignored() {
        for json in [
            r#"{"pid":1,"status":"idle","tmux":null}"#,
            r#"{"pid":1,"status":"idle"}"#,
            r#"{"status":"idle","tmux":"a:@1.%1"}"#,
            r#"{"pid":1,"tmux":"a-pane-somewhere"}"#,
            "not json",
        ] {
            assert_eq!(ClaudeSession::parse(json), None, "{json}");
        }
    }

    #[test]
    fn a_live_claude_on_the_pane_fills_it_in() {
        let mut panes = vec![pane("leka", "%29", "/dev/pts/35", "claude")];
        let files = vec![file(34477, "leka:@29.%29", "busy")];
        fill(&mut panes, &files, &shells(), |pids| {
            assert_eq!(pids, [34477]);
            HashMap::from([(34477, "pts/35".to_string())])
        });
        assert_eq!(panes[0].state, AgentState::Busy);
        assert_eq!(panes[0].agent.as_deref(), Some("claude"));
        assert_eq!(panes[0].agent_session.as_deref(), Some("conv-34477"));
    }

    #[test]
    fn stale_and_mismatched_files_change_nothing() {
        let files = vec![
            file(10, "leka:@29.%29", "busy"), // dead: not in the running list
            file(11, "dmi:@4.%4", "busy"),    // alive, on another terminal (pane id reused)
            file(12, "the-deaner:@1.%1", "busy"), // a pane the hooks already cover
            file(13, "tally:@16.%16", "busy"), // a pane showing a shell
            file(14, "elsewhere:@9.%9", "busy"), // not a managed pane
        ];
        let mut panes = vec![
            pane("leka", "%29", "/dev/pts/35", "claude"),
            pane("dmi", "%4", "/dev/pts/7", "claude"),
            pane("the-deaner", "%1", "/dev/pts/4", "claude"),
            pane("tally", "%16", "/dev/pts/19", "bash"),
        ];
        panes[2].state = AgentState::Idle;
        panes[2].agent = Some("claude".into());
        let before = panes.clone();
        fill(&mut panes, &files, &shells(), |pids| {
            assert_eq!(pids, [10, 11]);
            HashMap::from([(11, "pts/99".to_string())])
        });
        assert_eq!(panes, before);
    }

    #[test]
    fn nothing_to_check_runs_nothing() {
        let mut panes = vec![pane("dmi", "%3", "/dev/pts/6", "nvim")];
        fill(
            &mut panes,
            &[file(1, "leka:@29.%29", "busy")],
            &shells(),
            |_| panic!("no pane to check"),
        );
    }

    #[test]
    fn of_two_files_for_one_pane_the_running_one_counts() {
        let mut panes = vec![pane("leka", "%29", "/dev/ttys004", "claude")];
        let files = vec![
            file(900, "leka:@29.%29", "waiting"),
            file(500, "leka:@29.%29", "busy"),
        ];
        fill(&mut panes, &files, &shells(), |pids| {
            assert_eq!(pids, [900, 500]);
            HashMap::from([(900, "ttys004".to_string())])
        });
        assert_eq!(panes[0].state, AgentState::Awaiting);
    }

    #[test]
    fn a_file_newer_than_the_last_hook_wins() {
        // The hooks said busy at 1000 and were then removed; claude has been idle since 2000.
        let hooked = |at| {
            let mut p = pane("temp", "%31", "/dev/pts/31", "claude");
            p.state = AgentState::Busy;
            p.agent = Some("claude".into());
            p.agent_session = Some("from-the-hook".into());
            p.state_at = at;
            p
        };
        let files = vec![file_at(85743, "temp:@31.%31", "shell", 2000)];
        let running = |_: &[u32]| HashMap::from([(85743, "pts/31".to_string())]);

        let mut panes = vec![hooked(1000)];
        fill(&mut panes, &files, &shells(), running);
        assert_eq!(panes[0].state, AgentState::Idle);
        assert_eq!(panes[0].agent_session.as_deref(), Some("from-the-hook"));

        // Hooks still reporting: the latest one is newer, so it stands.
        let mut panes = vec![hooked(3000)];
        fill(&mut panes, &files, &shells(), |_| {
            panic!("nothing to check")
        });
        assert_eq!(panes[0].state, AgentState::Busy);
    }

    #[test]
    fn another_agents_hooks_are_never_overridden() {
        let mut p = pane("temp", "%31", "/dev/pts/31", "aider");
        p.state = AgentState::Busy;
        p.agent = Some("aider".into());
        let mut panes = vec![p];
        fill(
            &mut panes,
            &[file_at(1, "temp:@31.%31", "idle", 9999)],
            &shells(),
            |_| panic!("nothing to check"),
        );
        assert_eq!(panes[0].state, AgentState::Busy);
    }
}
