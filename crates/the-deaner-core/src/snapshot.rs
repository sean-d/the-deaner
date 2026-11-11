//! Snapshots: everything needed to rebuild the-deaner's tmux sessions after a
//! reboot, and the rules for what each pane runs when it comes back.

use crate::config::Config;
use crate::pane::PaneInfo;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const SNAPSHOT_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    pub version: u32,
    /// Seconds since the Unix epoch.
    pub created: u64,
    #[serde(default)]
    pub workspaces: Vec<WorkspaceSnap>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceSnap {
    pub name: String,
    pub repo: PathBuf,
    pub path: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(default)]
    pub worktree: bool,
    #[serde(default)]
    pub windows: Vec<WindowSnap>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowSnap {
    pub index: u32,
    pub name: String,
    /// tmux layout string, reapplied when the pane count matches.
    pub layout: String,
    #[serde(default)]
    pub active: bool,
    #[serde(default)]
    pub panes: Vec<PaneSnap>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaneSnap {
    pub index: u32,
    pub cwd: PathBuf,
    #[serde(default)]
    pub active: bool,
    #[serde(default)]
    pub current_command: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_session: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub launch: Option<String>,
    /// Something other than the pane's own login shell was in the foreground.
    /// `None` in old snapshots, where the shell list in the config decides.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub running: Option<bool>,
}

impl Snapshot {
    /// Build a snapshot from `list-panes -a` output, keeping only the-deaner-managed sessions.
    ///
    /// `idle_shell` is the program name of the panes' login shell (tmux `default-shell`).
    /// A pane whose foreground process is anything else counts as running something,
    /// even when that something is another shell (a wrapper script, `sh -c …`).
    #[must_use]
    pub fn from_panes(panes: &[PaneInfo], created: u64, idle_shell: Option<&str>) -> Self {
        let mut workspaces: Vec<WorkspaceSnap> = Vec::new();
        for p in panes {
            let Some(path) = p.workspace_path.clone() else {
                continue;
            };
            let ws = match workspaces.iter_mut().position(|w| w.name == p.session) {
                Some(i) => &mut workspaces[i],
                None => {
                    workspaces.push(WorkspaceSnap {
                        name: p.session.clone(),
                        repo: p.repo.clone().unwrap_or_else(|| path.clone()),
                        path,
                        branch: p.branch.clone(),
                        worktree: p.worktree,
                        windows: Vec::new(),
                    });
                    workspaces.last_mut().expect("just pushed")
                }
            };
            let win = match ws
                .windows
                .iter_mut()
                .position(|w| w.index == p.window_index)
            {
                Some(i) => &mut ws.windows[i],
                None => {
                    ws.windows.push(WindowSnap {
                        index: p.window_index,
                        name: p.window_name.clone(),
                        layout: p.window_layout.clone(),
                        active: p.window_active,
                        panes: Vec::new(),
                    });
                    ws.windows.last_mut().expect("just pushed")
                }
            };
            win.panes.push(PaneSnap {
                index: p.pane_index,
                cwd: p.cwd.clone(),
                active: p.pane_active,
                current_command: p.current_command.clone(),
                agent: p.agent.clone(),
                agent_session: p.agent_session.clone(),
                launch: p.launch.clone(),
                running: idle_shell.map(|s| p.current_command.trim_start_matches('-') != s),
            });
        }
        for ws in &mut workspaces {
            ws.windows.sort_by_key(|w| w.index);
            for w in &mut ws.windows {
                w.panes.sort_by_key(|p| p.index);
            }
        }
        Self {
            version: SNAPSHOT_VERSION,
            created,
            workspaces,
        }
    }

    /// # Errors
    /// Returns the TOML error for invalid syntax.
    pub fn from_toml(text: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(text)
    }

    #[must_use]
    pub fn to_toml(&self) -> String {
        toml::to_string_pretty(self).expect("snapshot always serializes")
    }

    #[must_use]
    pub fn workspace(&self, name: &str) -> Option<&WorkspaceSnap> {
        self.workspaces.iter().find(|w| w.name == name)
    }

    /// Replace or add one workspace (used when parking).
    pub fn upsert(&mut self, ws: WorkspaceSnap) {
        self.workspaces.retain(|w| w.name != ws.name);
        self.workspaces.push(ws);
    }
}

/// What to type into a restored pane, or `None` to leave a plain shell.
///
/// 1. An agent with a conversation that can be resumed resumes it. `resumable` is asked
///    because an id alone proves nothing: Claude assigns one at startup, but only
///    writes a conversation once it gets a first message.
/// 2. Any other agent is started fresh.
/// 3. A command the-deaner launched that was still running is launched again.
/// 4. A known editor that was running is reopened.
/// 5. Anything else (a bare shell, an unknown process) gets a shell.
#[must_use]
pub fn restore_command(
    pane: &PaneSnap,
    cfg: &Config,
    resumable: &dyn Fn(&str, &str) -> bool,
) -> Option<String> {
    if let Some(agent) = pane.agent.as_deref() {
        let def = cfg.agents.get(agent);
        if let (Some(id), Some(template)) = (
            pane.agent_session
                .as_deref()
                .filter(|id| resumable(agent, id)),
            def.and_then(|d| d.resume.as_deref()),
        ) {
            return Some(template.replace("{session_id}", &shell_words_quote(id)));
        }
        return def
            .map(|d| d.command.clone())
            .or_else(|| pane.launch.clone());
    }
    let idle = pane
        .running
        .map_or_else(|| cfg.is_shell(&pane.current_command), |running| !running);
    if idle {
        return None;
    }
    if let Some(launch) = pane.launch.clone() {
        return Some(launch);
    }
    cfg.editors
        .contains(&pane.current_command)
        .then(|| pane.current_command.clone())
}

/// Quote a value for a POSIX shell if it needs it.
fn shell_words_quote(s: &str) -> String {
    if !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_.:/=@".contains(c))
    {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', r"'\''"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pane::sample_line;

    fn pane(
        agent: Option<&str>,
        session: Option<&str>,
        launch: Option<&str>,
        cmd: &str,
    ) -> PaneSnap {
        PaneSnap {
            index: 0,
            cwd: "/r".into(),
            active: true,
            current_command: cmd.into(),
            agent: agent.map(Into::into),
            agent_session: session.map(Into::into),
            launch: launch.map(Into::into),
            running: None,
        }
    }

    #[test]
    fn an_id_with_no_conversation_starts_the_agent_fresh() {
        let cfg = Config::default();
        let p = pane(Some("claude"), Some("never-used"), Some("claude"), "claude");
        let asked = std::cell::RefCell::new(Vec::new());
        let cmd = restore_command(&p, &cfg, &|agent, id| {
            asked.borrow_mut().push(format!("{agent}/{id}"));
            false
        });
        assert_eq!(cmd.as_deref(), Some("claude"), "not claude --resume");
        assert_eq!(*asked.borrow(), ["claude/never-used"]);
    }

    #[test]
    fn wrapped_commands_count_as_running() {
        let cfg = Config::default();
        let mut p = pane(None, None, Some("sh -c 'npm run dev'"), "bash");
        assert_eq!(
            restore_command(&p, &cfg, &|_, _| true),
            None,
            "old snapshots fall back to the shell list"
        );
        p.running = Some(true);
        assert_eq!(
            restore_command(&p, &cfg, &|_, _| true).as_deref(),
            Some("sh -c 'npm run dev'")
        );
        p.running = Some(false);
        assert_eq!(restore_command(&p, &cfg, &|_, _| true), None);
    }

    #[test]
    fn restore_rules() {
        let cfg = Config::default();
        let r = |p: PaneSnap| restore_command(&p, &cfg, &|_, _| true);
        assert_eq!(
            r(pane(
                Some("claude"),
                Some("abc-123"),
                Some("claude"),
                "claude"
            ))
            .as_deref(),
            Some("claude --resume abc-123")
        );
        assert_eq!(
            r(pane(Some("claude"), Some("we ird"), None, "claude")).as_deref(),
            Some("claude --resume 'we ird'")
        );
        assert_eq!(
            r(pane(Some("claude"), None, None, "zsh")).as_deref(),
            Some("claude")
        );
        assert_eq!(
            r(pane(None, None, Some("npm run dev"), "node")).as_deref(),
            Some("npm run dev")
        );
        assert_eq!(
            r(pane(None, None, Some("nvim"), "zsh")),
            None,
            "exited launch stays a shell"
        );
        assert_eq!(r(pane(None, None, None, "nvim")).as_deref(), Some("nvim"));
        assert_eq!(r(pane(None, None, None, "htop")), None);
        assert_eq!(
            r(pane(Some("mystery"), Some("x"), Some("mystery --go"), "x")).as_deref(),
            Some("mystery --go")
        );
    }

    #[test]
    fn builds_from_panes_and_round_trips() {
        let lines = [
            sample_line("api", 1, "nvim", ""),
            sample_line("api", 0, "claude", "busy"),
            sample_line("scratch-unmanaged", 0, "zsh", ""),
        ];
        let mut panes: Vec<PaneInfo> = lines
            .iter()
            .filter_map(|l| PaneInfo::parse_line(l))
            .collect();
        panes[2].workspace_path = None;
        let snap = Snapshot::from_panes(&panes, 1_700_000_000, Some("zsh"));
        assert_eq!(snap.workspaces.len(), 1);
        let ws = &snap.workspaces[0];
        assert_eq!(ws.name, "api");
        assert_eq!(
            ws.windows.iter().map(|w| w.index).collect::<Vec<_>>(),
            [0, 1]
        );
        assert_eq!(
            ws.windows[0].panes[0].agent_session.as_deref(),
            Some("abc-123")
        );

        let back = Snapshot::from_toml(&snap.to_toml()).unwrap();
        assert_eq!(back, snap);
    }

    #[test]
    fn upsert_replaces_by_name() {
        let mut snap = Snapshot {
            version: SNAPSHOT_VERSION,
            created: 0,
            workspaces: vec![],
        };
        let ws = WorkspaceSnap {
            name: "api".into(),
            repo: "/r".into(),
            path: "/r".into(),
            branch: None,
            worktree: false,
            windows: vec![],
        };
        snap.upsert(ws.clone());
        snap.upsert(ws);
        assert_eq!(snap.workspaces.len(), 1);
        assert!(snap.workspace("api").is_some());
    }
}
