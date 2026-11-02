//! Sidebar model: repos grouped with their workspaces, ordered with pinned repos
//! first (in the user's order, never moved by activity), then everything else.

use crate::agent::AgentState;
use crate::git::GitStatus;
use crate::pr::PrStatus;
use crate::state::RepoEntry;
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum WsStatus {
    /// A tmux session is running.
    Live,
    /// Saved and stopped with `park`; Enter brings it back.
    Parked,
    /// Known checkout without a session.
    Stopped,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TabView {
    pub index: u32,
    pub name: String,
    pub state: AgentState,
    pub agent: Option<String>,
    /// `<session>:<tab>` address for `deaner send`.
    pub address: String,
}

/// One workspace (a checkout plus its session, if any).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WorkspaceView {
    pub repo: PathBuf,
    pub path: PathBuf,
    /// tmux session name (current, or the one it would get).
    pub name: String,
    pub branch: Option<String>,
    pub worktree: bool,
    pub status: WsStatus,
    pub agent: AgentState,
    pub git: Option<GitStatus>,
    pub pr: Option<PrStatus>,
    pub tabs: Vec<TabView>,
    /// Shown in the terminal pane right now.
    pub current: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RepoGroup {
    pub path: PathBuf,
    pub name: String,
    pub pinned: bool,
    pub collapsed: bool,
    pub workspaces: Vec<WorkspaceView>,
}

impl RepoGroup {
    #[must_use]
    pub fn agent(&self) -> AgentState {
        AgentState::rollup(self.workspaces.iter().map(|w| w.agent))
    }
}

/// A visible sidebar line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Row {
    Repo { group: usize },
    Workspace { group: usize, ws: usize },
}

/// Inputs for [`build`].
pub struct Layout<'a> {
    pub repos: &'a [RepoEntry],
    pub pins: &'a [PathBuf],
    pub collapsed: &'a [PathBuf],
    pub float_attention: bool,
}

fn display_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    )
}

/// Group workspaces under their repos and order the groups.
///
/// Order: pinned repos exactly as in `pins`; then unpinned repos by name
/// (case-insensitive), with repos that have an agent awaiting input floated to
/// the top of the unpinned section when `float_attention` is set. Within a
/// repo the main checkout comes first, worktrees keep their input order.
#[must_use]
pub fn build(layout: &Layout<'_>, workspaces: Vec<WorkspaceView>) -> Vec<RepoGroup> {
    let mut groups: Vec<RepoGroup> = layout
        .repos
        .iter()
        .map(|r| RepoGroup {
            path: r.path.clone(),
            name: r.name.clone(),
            pinned: false,
            collapsed: false,
            workspaces: Vec::new(),
        })
        .collect();
    for ws in workspaces {
        let idx = match groups.iter().position(|g| g.path == ws.repo) {
            Some(i) => i,
            None => {
                groups.push(RepoGroup {
                    path: ws.repo.clone(),
                    name: display_name(&ws.repo),
                    pinned: false,
                    collapsed: false,
                    workspaces: Vec::new(),
                });
                groups.len() - 1
            }
        };
        groups[idx].workspaces.push(ws);
    }
    for g in &mut groups {
        g.pinned = layout.pins.contains(&g.path);
        g.collapsed = layout.collapsed.contains(&g.path);
        // Stable: main checkout first, worktrees keep their order.
        g.workspaces.sort_by_key(|w| w.worktree || w.path != g.path);
    }

    let pin_rank = |g: &RepoGroup| layout.pins.iter().position(|p| *p == g.path);
    groups.sort_by(|a, b| match (pin_rank(a), pin_rank(b)) {
        (Some(x), Some(y)) => x.cmp(&y),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => {
            let attention =
                |g: &RepoGroup| layout.float_attention && g.agent() == AgentState::Awaiting;
            attention(b)
                .cmp(&attention(a))
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        }
    });
    groups
}

fn matches(haystack: &str, needle: &str) -> bool {
    haystack.to_lowercase().contains(needle)
}

/// Flatten groups into visible rows. A non-empty `filter` matches repo names,
/// branches and session names, and ignores collapsing.
#[must_use]
pub fn rows(groups: &[RepoGroup], filter: &str) -> Vec<Row> {
    let filter = filter.trim().to_lowercase();
    let mut out = Vec::new();
    for (gi, g) in groups.iter().enumerate() {
        if filter.is_empty() {
            out.push(Row::Repo { group: gi });
            if !g.collapsed {
                out.extend((0..g.workspaces.len()).map(|wi| Row::Workspace { group: gi, ws: wi }));
            }
            continue;
        }
        let repo_hit = matches(&g.name, &filter);
        let hits: Vec<usize> = g
            .workspaces
            .iter()
            .enumerate()
            .filter(|(_, w)| {
                repo_hit
                    || matches(&w.name, &filter)
                    || w.branch.as_deref().is_some_and(|b| matches(b, &filter))
            })
            .map(|(i, _)| i)
            .collect();
        if repo_hit || !hits.is_empty() {
            out.push(Row::Repo { group: gi });
            out.extend(
                hits.into_iter()
                    .map(|wi| Row::Workspace { group: gi, ws: wi }),
            );
        }
    }
    out
}

#[cfg(test)]
pub(crate) fn ws(repo: &str, path: &str, branch: &str, agent: AgentState) -> WorkspaceView {
    WorkspaceView {
        repo: PathBuf::from(repo),
        path: PathBuf::from(path),
        name: branch.to_string(),
        branch: Some(branch.to_string()),
        worktree: repo != path,
        status: WsStatus::Live,
        agent,
        git: None,
        pr: None,
        tabs: Vec::new(),
        current: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repos(names: &[&str]) -> Vec<RepoEntry> {
        names
            .iter()
            .map(|n| RepoEntry {
                path: PathBuf::from(format!("/r/{n}")),
                name: (*n).to_string(),
            })
            .collect()
    }

    fn names(groups: &[RepoGroup]) -> Vec<&str> {
        groups.iter().map(|g| g.name.as_str()).collect()
    }

    #[test]
    fn pinned_first_in_user_order_and_never_float() {
        let repos = repos(&["alpha", "Beta", "gamma", "delta"]);
        let pins = vec![PathBuf::from("/r/gamma"), PathBuf::from("/r/alpha")];
        let workspaces = vec![
            ws("/r/alpha", "/r/alpha", "main", AgentState::Idle),
            ws("/r/gamma", "/r/gamma", "main", AgentState::Idle),
            // delta is unpinned and awaiting: floats to the top of the unpinned section only
            ws("/r/delta", "/r/delta", "main", AgentState::Awaiting),
        ];
        let layout = Layout {
            repos: &repos,
            pins: &pins,
            collapsed: &[],
            float_attention: true,
        };
        let groups = build(&layout, workspaces.clone());
        assert_eq!(names(&groups), ["gamma", "alpha", "delta", "Beta"]);
        assert!(groups[0].pinned && groups[1].pinned && !groups[2].pinned);

        let no_float = Layout {
            float_attention: false,
            ..layout
        };
        assert_eq!(
            names(&build(&no_float, workspaces)),
            ["gamma", "alpha", "Beta", "delta"]
        );
    }

    #[test]
    fn pinned_awaiting_does_not_reorder_pins() {
        let repos = repos(&["a", "b"]);
        let pins = vec![PathBuf::from("/r/a"), PathBuf::from("/r/b")];
        let layout = Layout {
            repos: &repos,
            pins: &pins,
            collapsed: &[],
            float_attention: true,
        };
        let groups = build(
            &layout,
            vec![ws("/r/b", "/r/b", "main", AgentState::Awaiting)],
        );
        assert_eq!(names(&groups), ["a", "b"]);
    }

    #[test]
    fn main_checkout_first_and_unknown_repos_grouped() {
        let repos = repos(&["api"]);
        let layout = Layout {
            repos: &repos,
            pins: &[],
            collapsed: &[],
            float_attention: false,
        };
        let groups = build(
            &layout,
            vec![
                ws("/r/api", "/r/api.wt/feat", "feat", AgentState::None),
                ws("/r/api", "/r/api", "main", AgentState::None),
                ws(
                    "/elsewhere/tool",
                    "/elsewhere/tool",
                    "main",
                    AgentState::None,
                ),
            ],
        );
        assert_eq!(names(&groups), ["api", "tool"]);
        assert_eq!(groups[0].workspaces[0].name, "main");
        assert_eq!(groups[0].workspaces[1].name, "feat");
    }

    #[test]
    fn rows_respect_collapse_and_filter() {
        let repos = repos(&["api", "web"]);
        let collapsed = vec![PathBuf::from("/r/web")];
        let layout = Layout {
            repos: &repos,
            pins: &[],
            collapsed: &collapsed,
            float_attention: false,
        };
        let groups = build(
            &layout,
            vec![
                ws("/r/api", "/r/api", "main", AgentState::None),
                ws("/r/api", "/r/api.wt/sec", "security-bump", AgentState::None),
                ws("/r/web", "/r/web", "main", AgentState::None),
            ],
        );
        assert_eq!(
            rows(&groups, ""),
            [
                Row::Repo { group: 0 },
                Row::Workspace { group: 0, ws: 0 },
                Row::Workspace { group: 0, ws: 1 },
                Row::Repo { group: 1 },
            ]
        );
        assert_eq!(
            rows(&groups, "SECUR"),
            [Row::Repo { group: 0 }, Row::Workspace { group: 0, ws: 1 }]
        );
        // repo-name match shows all its workspaces, even when collapsed
        assert_eq!(
            rows(&groups, "web"),
            [Row::Repo { group: 1 }, Row::Workspace { group: 1, ws: 0 }]
        );
    }
}
