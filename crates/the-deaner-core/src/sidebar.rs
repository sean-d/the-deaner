//! Sidebar model: repos grouped with their workspaces, in three sections.
//!
//! - **Pinned**: the user's order, never moved by activity.
//! - **Active**: unpinned repos with an agent running in one of their sessions.
//! - **Inactive**: unpinned repos with no agent running (sessions with only an
//!   editor or a shell open still count as inactive).

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
    /// `<session>:<tab>` address for `the-deaner send`.
    pub address: String,
    /// The agent's conversation id, while one is running.
    pub session: Option<String>,
    /// What the agent says it's working on (Claude's conversation title).
    pub title: Option<String>,
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

    /// Shown as a single row, name and branch together: an inactive repo with only its
    /// one checkout. Not the open project, which needs its own row for the box.
    #[must_use]
    pub fn one_line(&self) -> bool {
        self.section == Section::Inactive
            && self.workspaces.len() == 1
            && !self.workspaces[0].current
    }
}

/// A visible sidebar line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Row {
    /// Blank line above a section heading, after the section before it; can't be selected.
    Gap {
        section: Section,
    },
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
    /// What an agent in one of a workspace's tabs is working on, under the workspace;
    /// can't be selected.
    Agent {
        group: usize,
        ws: usize,
        tab: usize,
    },
    /// Bottom edge of the box around the open project; can't be selected.
    BoxEnd {
        group: usize,
    },
}

impl Row {
    /// Repos and workspaces; not headings or the gaps above them.
    #[must_use]
    pub const fn selectable(self) -> bool {
        matches!(self, Self::Repo { .. } | Self::Workspace { .. })
    }
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
/// Order: pinned repos exactly as in `pins` (running or not); then active repos
/// (an agent is running in one of their workspaces), then inactive ones, each by name (case-insensitive). Active repos with an agent
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
        } else if g.workspaces.iter().any(|w| w.agent != AgentState::None) {
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
/// branches and session names, and ignores collapsing. With `boxed`, the open
/// project (the one with a workspace on screen) gets a row closing its box.
#[must_use]
pub fn rows(groups: &[RepoGroup], filter: &str, boxed: bool) -> Vec<Row> {
    let mut out = visible_rows(groups, filter);
    if boxed
        && let Some(open) = groups
            .iter()
            .position(|g| g.workspaces.iter().any(|w| w.current))
    {
        let last = out.iter().rposition(
            |r| matches!(r, Row::Repo { group } | Row::Workspace { group, .. } | Row::Agent { group, .. } if *group == open),
        );
        if let Some(i) = last {
            out.insert(i + 1, Row::BoxEnd { group: open });
        }
    }
    out
}

fn visible_rows(groups: &[RepoGroup], filter: &str) -> Vec<Row> {
    let filter = filter.trim().to_lowercase();
    let mut out = Vec::new();
    let mut current = None;
    let mut heading = |out: &mut Vec<Row>, section: Section| {
        if current != Some(section) {
            if current.is_some() {
                out.push(Row::Gap { section });
            }
            current = Some(section);
            out.push(Row::Header { section });
        }
    };
    for (gi, g) in groups.iter().enumerate() {
        if filter.is_empty() {
            heading(&mut out, g.section);
            if g.one_line() {
                push_workspace(&mut out, g, gi, 0);
                continue;
            }
            out.push(Row::Repo { group: gi });
            if !g.collapsed {
                for wi in 0..g.workspaces.len() {
                    push_workspace(&mut out, g, gi, wi);
                }
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
            if g.one_line() {
                push_workspace(&mut out, g, gi, 0);
                continue;
            }
            out.push(Row::Repo { group: gi });
            for wi in hits {
                push_workspace(&mut out, g, gi, wi);
            }
        }
    }
    out
}

/// A workspace row, then a line for each agent tab that has a title.
fn push_workspace(out: &mut Vec<Row>, g: &RepoGroup, gi: usize, wi: usize) {
    out.push(Row::Workspace { group: gi, ws: wi });
    let tabs = &g.workspaces[wi].tabs;
    out.extend(
        (0..tabs.len())
            .filter(|&ti| tabs[ti].title.is_some())
            .map(|tab| Row::Agent {
                group: gi,
                ws: wi,
                tab,
            }),
    );
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
                ws("/r/api", "/r/api", "main", AgentState::Idle),
                ws("/r/api", "/r/api.wt/sec", "security-bump", AgentState::Idle),
                ws("/r/web", "/r/web", "main", AgentState::Idle),
            ],
        );
        assert_eq!(
            rows(&groups, "", false),
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
            rows(&groups, "SECUR", false),
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
            rows(&groups, "web", false),
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
    fn the_open_project_gets_a_box_end_after_its_last_visible_row() {
        let repos = repos(&["api", "web"]);
        let layout = Layout {
            repos: &repos,
            pins: &[],
            collapsed: &[],
            float_attention: false,
        };
        let mut open = ws("/r/api", "/r/api", "main", AgentState::Idle);
        open.current = true;
        let groups = build(
            &layout,
            vec![
                open,
                ws("/r/api", "/r/api.wt/sec", "sec", AgentState::Idle),
                ws("/r/web", "/r/web", "main", AgentState::Idle),
            ],
        );
        let boxed = rows(&groups, "", true);
        assert_eq!(boxed[4], Row::BoxEnd { group: 0 }, "{boxed:?}");
        assert!(!boxed[4].selectable());
        assert_eq!(boxed.len(), rows(&groups, "", false).len() + 1);
        // Nothing open, nothing to close.
        let mut closed = groups.clone();
        closed[0].workspaces[0].current = false;
        assert!(!rows(&closed, "", true).contains(&Row::BoxEnd { group: 0 }));
    }

    #[test]
    fn an_inactive_repo_with_one_checkout_is_one_row() {
        let repos = repos(&["api", "notes", "web"]);
        let layout = Layout {
            repos: &repos,
            pins: &[],
            collapsed: &[],
            float_attention: false,
        };
        let stopped = |repo: &str, path: &str| WorkspaceView {
            status: WsStatus::Stopped,
            ..ws(repo, path, "main", AgentState::None)
        };
        let groups = build(
            &layout,
            vec![
                stopped("/r/api", "/r/api"),
                stopped("/r/api", "/r/api.wt/x"),
                stopped("/r/notes", "/r/notes"),
                ws("/r/web", "/r/web", "main", AgentState::Idle),
            ],
        );
        let notes = groups.iter().position(|g| g.name == "notes").unwrap();
        let api = groups.iter().position(|g| g.name == "api").unwrap();
        let shown = rows(&groups, "", false);
        assert!(!shown.contains(&Row::Repo { group: notes }), "{shown:?}");
        assert!(shown.contains(&Row::Workspace {
            group: notes,
            ws: 0
        }));
        assert!(
            shown.contains(&Row::Repo { group: api }),
            "two checkouts keep the repo row"
        );
        assert!(rows(&groups, "note", false).contains(&Row::Workspace {
            group: notes,
            ws: 0
        }));

        // Open, it keeps its own row (the box needs it).
        let mut open = groups.clone();
        open[notes].workspaces[0].current = true;
        assert!(rows(&open, "", true).contains(&Row::Repo { group: notes }));
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
        // zed is pinned and stopped; web has an agent; api and notes are stopped.
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
        let headings: Vec<_> = rows(&groups, "", false)
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

        // An agent in a worktree makes its repo active, and starting a pinned repo doesn't move it.
        let groups = build(
            &layout,
            vec![
                ws("/r/zed", "/r/zed", "main", AgentState::Busy),
                stopped("/r/api"),
                ws("/r/api", "/r/api.wt/x", "x", AgentState::Idle),
            ],
        );
        assert_eq!(names(&groups), ["zed", "api", "notes", "web"]);
        assert_eq!(groups[0].section, Section::Pinned);
        assert_eq!(groups[1].section, Section::Active);
        assert!(rows(&groups, "", false).contains(&Row::Header {
            section: Section::Pinned
        }));
    }

    #[test]
    fn a_session_without_an_agent_is_inactive() {
        let repos = repos(&["api", "web"]);
        let layout = Layout {
            repos: &repos,
            pins: &[],
            collapsed: &[],
            float_attention: true,
        };
        // Both are running; only web has an agent. api just has nvim and a shell.
        let groups = build(
            &layout,
            vec![
                ws("/r/api", "/r/api", "main", AgentState::None),
                ws("/r/web", "/r/web", "main", AgentState::Idle),
            ],
        );
        assert_eq!(names(&groups), ["web", "api"]);
        assert_eq!(groups[0].section, Section::Active);
        assert_eq!(groups[1].section, Section::Inactive);
        assert_eq!(groups[1].workspaces[0].status, WsStatus::Live);
    }
}
