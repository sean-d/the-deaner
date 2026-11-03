//! Sidebar model: repos grouped with their workspaces, in three sections.
//!
//! - **Pinned**: the user's order, never moved by activity.
//! - **Active**: unpinned repos with a running session.
//! - **Inactive**: unpinned repos that are added but not running.

use crate::agent::AgentState;
use crate::git::GitStatus;
use crate::pr::{Checks, PrStatus};
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
    /// CI for this branch.
    pub checks: Checks,
    pub tabs: Vec<TabView>,
    /// Shown in the terminal pane right now.
    pub current: bool,
}

/// Which part of the sidebar a repo sits in. Declared in display order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Section {
    Pinned,
    Active,
    Inactive,
}

impl Section {
    /// Heading shown above the section.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Pinned => "PINNED",
            Self::Active => "ACTIVE",
            Self::Inactive => "INACTIVE",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RepoGroup {
    pub path: PathBuf,
    pub name: String,
    pub pinned: bool,
    pub section: Section,
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
    /// Section heading; can't be selected.
    Header {
        section: Section,
    },
    Repo {
        group: usize,
    },
    Workspace {
        group: usize,
        ws: usize,
    },
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
/// Order: pinned repos exactly as in `pins` (running or not); then active repos,
/// then inactive ones, each by name (case-insensitive). Active repos with an agent
/// awaiting input float to the top of their section when `float_attention` is set.
/// Within a repo the main checkout comes first, worktrees keep their input order.
#[must_use]
pub fn build(layout: &Layout<'_>, workspaces: Vec<WorkspaceView>) -> Vec<RepoGroup> {
    let mut groups: Vec<RepoGroup> = layout
        .repos
        .iter()
        .map(|r| RepoGroup {
            path: r.path.clone(),
            name: r.name.clone(),
            pinned: false,
            section: Section::Inactive,
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
                    section: Section::Inactive,
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
        g.section = if g.pinned {
            Section::Pinned
        } else if g.workspaces.iter().any(|w| w.status == WsStatus::Live) {
            Section::Active
        } else {
            Section::Inactive
        };
    }

    let pin_rank = |g: &RepoGroup| layout.pins.iter().position(|p| *p == g.path);
    groups.sort_by(|a, b| {
        a.section.cmp(&b.section).then_with(|| {
            if a.section == Section::Pinned {
                return pin_rank(a).cmp(&pin_rank(b));
            }
            let attention =
                |g: &RepoGroup| layout.float_attention && g.agent() == AgentState::Awaiting;
            attention(b)
                .cmp(&attention(a))
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        })
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
    let mut current = None;
    let mut heading = |out: &mut Vec<Row>, section: Section| {
        if current != Some(section) {
            current = Some(section);
            out.push(Row::Header { section });
        }
    };
    for (gi, g) in groups.iter().enumerate() {
        if filter.is_empty() {
            heading(&mut out, g.section);
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
            heading(&mut out, g.section);
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
        checks: Checks::None,
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
            ws("/r/Beta", "/r/Beta", "main", AgentState::Idle),
            // delta is unpinned and awaiting: floats to the top of the active section only
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
                Row::Header {
                    section: Section::Active
                },
                Row::Repo { group: 0 },
                Row::Workspace { group: 0, ws: 0 },
                Row::Workspace { group: 0, ws: 1 },
                Row::Repo { group: 1 },
            ]
        );
        assert_eq!(
            rows(&groups, "SECUR"),
            [
                Row::Header {
                    section: Section::Active
                },
                Row::Repo { group: 0 },
                Row::Workspace { group: 0, ws: 1 }
            ]
        );
        // repo-name match shows all its workspaces, even when collapsed
        assert_eq!(
            rows(&groups, "web"),
            [
                Row::Header {
                    section: Section::Active
                },
                Row::Repo { group: 1 },
                Row::Workspace { group: 1, ws: 0 }
            ]
        );
    }

    #[test]
    fn sections_split_active_from_inactive_and_pins_stay_put() {
        let repos = repos(&["zed", "api", "web", "notes"]);
        let pins = vec![PathBuf::from("/r/zed")];
        let stopped = |repo: &str| WorkspaceView {
            status: WsStatus::Stopped,
            ..ws(repo, repo, "main", AgentState::None)
        };
        let layout = Layout {
            repos: &repos,
            pins: &pins,
            collapsed: &[],
            float_attention: true,
        };
        // zed is pinned and stopped; web is running; api and notes are stopped.
        let groups = build(
            &layout,
            vec![
                stopped("/r/zed"),
                stopped("/r/api"),
                ws("/r/web", "/r/web", "main", AgentState::Idle),
                stopped("/r/notes"),
            ],
        );
        assert_eq!(names(&groups), ["zed", "web", "api", "notes"]);
        let sections: Vec<_> = groups.iter().map(|g| g.section).collect();
        assert_eq!(
            sections,
            [
                Section::Pinned,
                Section::Active,
                Section::Inactive,
                Section::Inactive
            ]
        );

        // Each section gets one heading, in order, and only if it has repos.
        let headings: Vec<_> = rows(&groups, "")
            .into_iter()
            .filter_map(|r| match r {
                Row::Header { section } => Some(section),
                _ => None,
            })
            .collect();
        assert_eq!(
            headings,
            [Section::Pinned, Section::Active, Section::Inactive]
        );

        // A worktree running makes its repo active, and starting a pinned repo doesn't move it.
        let groups = build(
            &layout,
            vec![
                ws("/r/zed", "/r/zed", "main", AgentState::Busy),
                stopped("/r/api"),
                ws("/r/api", "/r/api.wt/x", "x", AgentState::None),
            ],
        );
        assert_eq!(names(&groups), ["zed", "api", "notes", "web"]);
        assert_eq!(groups[0].section, Section::Pinned);
        assert_eq!(groups[1].section, Section::Active);
        assert!(rows(&groups, "").contains(&Row::Header {
            section: Section::Pinned
        }));
    }
}
