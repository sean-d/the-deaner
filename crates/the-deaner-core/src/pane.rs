//! One row of `tmux list-panes -a -F <PANE_FORMAT>`, plus the option names
//! the-deaner stores on sessions and panes.

use crate::agent::AgentState;
use std::path::PathBuf;

/// Session option: workspace directory (main checkout or worktree). Marks a session as the-deaner's.
pub const OPT_PATH: &str = "@the_deaner_path";
/// Session option: main repo root.
pub const OPT_REPO: &str = "@the_deaner_repo";
/// Session option: branch the workspace was created for.
pub const OPT_BRANCH: &str = "@the_deaner_branch";
/// Session option: `1` when the workspace is a linked worktree.
pub const OPT_WORKTREE: &str = "@the_deaner_worktree";
/// Pane option: [`AgentState`] value.
pub const OPT_STATE: &str = "@the_deaner_state";
/// Pane option: agent name (`claude`, …).
pub const OPT_AGENT: &str = "@the_deaner_agent";
/// Pane option: agent conversation id, used to resume after a restore.
pub const OPT_AGENT_SESSION: &str = "@the_deaner_agent_session";
/// Pane option: command the-deaner launched in this pane.
pub const OPT_LAUNCH: &str = "@the_deaner_launch";

/// Field separator in formatted output. Unit separator: never in paths or names.
pub const SEP: char = '\u{1f}';

const FIELDS: &[&str] = &[
    "#{session_name}",
    "#{window_index}",
    "#{window_name}",
    "#{window_active}",
    "#{window_layout}",
    "#{pane_id}",
    "#{pane_index}",
    "#{pane_active}",
    "#{pane_current_path}",
    "#{pane_current_command}",
    "#{pane_tty}",
    "#{window_bell_flag}",
    "#{@the_deaner_path}",
    "#{@the_deaner_repo}",
    "#{@the_deaner_branch}",
    "#{@the_deaner_worktree}",
    "#{@the_deaner_state}",
    "#{@the_deaner_agent}",
    "#{@the_deaner_agent_session}",
    "#{@the_deaner_launch}",
];

/// The `-F` format for [`PaneInfo::parse_line`].
#[must_use]
pub fn pane_format() -> String {
    FIELDS.join(&SEP.to_string())
}

/// A pane as tmux reports it, with the-deaner's options resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneInfo {
    pub session: String,
    pub window_index: u32,
    pub window_name: String,
    pub window_active: bool,
    pub window_layout: String,
    pub pane_id: String,
    pub pane_index: u32,
    pub pane_active: bool,
    pub cwd: PathBuf,
    pub current_command: String,
    pub tty: String,
    pub bell: bool,
    /// Workspace path; `None` for sessions the-deaner does not manage.
    pub workspace_path: Option<PathBuf>,
    pub repo: Option<PathBuf>,
    pub branch: Option<String>,
    pub worktree: bool,
    pub state: AgentState,
    pub agent: Option<String>,
    pub agent_session: Option<String>,
    pub launch: Option<String>,
}

fn opt(s: &str) -> Option<String> {
    (!s.is_empty()).then(|| s.to_string())
}

impl PaneInfo {
    /// Parse one line produced by [`pane_format`]. Returns `None` for short lines.
    #[must_use]
    pub fn parse_line(line: &str) -> Option<Self> {
        let f: Vec<&str> = line.split(SEP).collect();
        if f.len() < FIELDS.len() {
            return None;
        }
        Some(Self {
            session: f[0].to_string(),
            window_index: f[1].parse().ok()?,
            window_name: f[2].to_string(),
            window_active: f[3] == "1",
            window_layout: f[4].to_string(),
            pane_id: f[5].to_string(),
            pane_index: f[6].parse().ok()?,
            pane_active: f[7] == "1",
            cwd: PathBuf::from(f[8]),
            current_command: f[9].to_string(),
            tty: f[10].to_string(),
            bell: f[11] == "1",
            workspace_path: opt(f[12]).map(PathBuf::from),
            repo: opt(f[13]).map(PathBuf::from),
            branch: opt(f[14]),
            worktree: f[15] == "1",
            state: AgentState::parse(f[16]),
            agent: opt(f[17]),
            agent_session: opt(f[18]),
            launch: opt(f[19]),
        })
    }

    /// Parse all lines, skipping malformed ones.
    #[must_use]
    pub fn parse_all(out: &str) -> Vec<Self> {
        out.lines().filter_map(Self::parse_line).collect()
    }

    /// Stable address other agents can send to: `<session>:<window name>`.
    #[must_use]
    pub fn address(&self) -> String {
        format!("{}:{}", self.session, self.window_name)
    }
}

#[cfg(test)]
pub(crate) fn sample_line(session: &str, window: u32, name: &str, state: &str) -> String {
    [
        session,
        &window.to_string(),
        name,
        "1",
        "b25d,80x24,0,0,1",
        "%1",
        "0",
        "1",
        "/home/sd/dev/api",
        "claude",
        "/dev/pts/3",
        "0",
        "/home/sd/dev/api",
        "/home/sd/dev/api",
        "main",
        "",
        state,
        "claude",
        "abc-123",
        "claude",
    ]
    .join(&SEP.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_and_parse_agree() {
        assert_eq!(pane_format().split(SEP).count(), FIELDS.len());
        let p = PaneInfo::parse_line(&sample_line("api", 2, "claude", "busy")).unwrap();
        assert_eq!(p.session, "api");
        assert_eq!(p.window_index, 2);
        assert_eq!(p.state, AgentState::Busy);
        assert_eq!(p.agent_session.as_deref(), Some("abc-123"));
        assert_eq!(p.workspace_path, Some(PathBuf::from("/home/sd/dev/api")));
        assert!(!p.worktree);
        assert_eq!(p.address(), "api:claude");
    }

    #[test]
    fn unmanaged_and_malformed() {
        let mut line = sample_line("scratch", 0, "zsh", "");
        // blank out the the-deaner options
        let mut parts: Vec<&str> = line.split(SEP).collect();
        for p in parts.iter_mut().skip(12) {
            *p = "";
        }
        line = parts.join(&SEP.to_string());
        let p = PaneInfo::parse_line(&line).unwrap();
        assert_eq!(p.workspace_path, None);
        assert_eq!(p.state, AgentState::None);
        assert!(PaneInfo::parse_line("too\u{1f}short").is_none());
        assert_eq!(PaneInfo::parse_all("junk\n").len(), 0);
    }
}
