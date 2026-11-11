//! Parsers for `git status --porcelain=v2 --branch` and `git worktree list --porcelain`.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Working-tree and branch state of one checkout.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitStatus {
    /// Branch name; `None` when HEAD is detached.
    pub branch: Option<String>,
    /// Current commit id; `None` before the first commit.
    pub oid: Option<String>,
    pub upstream: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    /// Entries with index (staged) changes.
    pub staged: u32,
    /// Entries with unstaged working-tree changes.
    pub modified: u32,
    pub untracked: u32,
    pub conflicted: u32,
}

impl GitStatus {
    /// Parse the output of `git status --porcelain=v2 --branch`.
    #[must_use]
    pub fn parse_porcelain_v2(out: &str) -> Self {
        let mut s = Self::default();
        for line in out.lines() {
            if let Some(header) = line.strip_prefix("# ") {
                let (key, value) = header.split_once(' ').unwrap_or((header, ""));
                match key {
                    "branch.oid" if value != "(initial)" => s.oid = Some(value.to_string()),
                    "branch.head" if value != "(detached)" => s.branch = Some(value.to_string()),
                    "branch.upstream" => s.upstream = Some(value.to_string()),
                    "branch.ab" => {
                        for part in value.split_whitespace() {
                            if let Some(n) = part.strip_prefix('+') {
                                s.ahead = n.parse().unwrap_or(0);
                            } else if let Some(n) = part.strip_prefix('-') {
                                s.behind = n.parse().unwrap_or(0);
                            }
                        }
                    }
                    _ => {}
                }
                continue;
            }
            let mut fields = line.splitn(3, ' ');
            match fields.next() {
                Some("1" | "2") => {
                    let xy = fields.next().unwrap_or("..").as_bytes();
                    if xy.first().is_some_and(|&x| x != b'.') {
                        s.staged += 1;
                    }
                    if xy.get(1).is_some_and(|&y| y != b'.') {
                        s.modified += 1;
                    }
                }
                Some("u") => s.conflicted += 1,
                Some("?") => s.untracked += 1,
                _ => {}
            }
        }
        s
    }

    /// Number of changed entries of any kind.
    #[must_use]
    pub const fn changed(&self) -> u32 {
        self.staged + self.modified + self.untracked + self.conflicted
    }

    #[must_use]
    pub const fn is_clean(&self) -> bool {
        self.changed() == 0
    }
}

/// One entry from `git worktree list --porcelain`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Worktree {
    pub path: PathBuf,
    pub head: Option<String>,
    /// Short branch name (without `refs/heads/`); `None` when detached or bare.
    pub branch: Option<String>,
    pub bare: bool,
    pub detached: bool,
    pub prunable: bool,
}

/// Parse `git worktree list --porcelain`. The first entry is the main checkout.
#[must_use]
pub fn parse_worktree_list(out: &str) -> Vec<Worktree> {
    let mut list = Vec::new();
    let mut current: Option<Worktree> = None;
    for line in out.lines() {
        if let Some(path) = line.strip_prefix("worktree ") {
            list.extend(current.take());
            current = Some(Worktree {
                path: PathBuf::from(path),
                head: None,
                branch: None,
                bare: false,
                detached: false,
                prunable: false,
            });
            continue;
        }
        let Some(wt) = current.as_mut() else { continue };
        if let Some(head) = line.strip_prefix("HEAD ") {
            wt.head = Some(head.to_string());
        } else if let Some(branch) = line.strip_prefix("branch ") {
            wt.branch = Some(
                branch
                    .strip_prefix("refs/heads/")
                    .unwrap_or(branch)
                    .to_string(),
            );
        } else if line == "bare" {
            wt.bare = true;
        } else if line == "detached" {
            wt.detached = true;
        } else if line.starts_with("prunable") {
            wt.prunable = true;
        }
    }
    list.extend(current);
    list
}

#[cfg(test)]
mod tests {
    use super::*;

    const STATUS: &str = "\
# branch.oid 1234567890abcdef
# branch.head feat/x
# branch.upstream origin/feat/x
# branch.ab +2 -1
1 M. N... 100644 100644 100644 abc abc src/staged.rs
1 .M N... 100644 100644 100644 abc abc src/modified.rs
1 MM N... 100644 100644 100644 abc abc src/both.rs
2 R. N... 100644 100644 100644 abc abc R100 new.rs\told.rs
u UU N... 100644 100644 100644 100644 abc abc abc conflict.rs
? untracked.txt
? other.txt
! ignored.log
";

    #[test]
    fn parses_branch_and_counts() {
        let s = GitStatus::parse_porcelain_v2(STATUS);
        assert_eq!(s.branch.as_deref(), Some("feat/x"));
        assert_eq!(s.upstream.as_deref(), Some("origin/feat/x"));
        assert_eq!((s.ahead, s.behind), (2, 1));
        assert_eq!(s.staged, 3, "M., MM, R.");
        assert_eq!(s.modified, 2, ".M, MM");
        assert_eq!(s.conflicted, 1);
        assert_eq!(s.untracked, 2);
        assert_eq!(s.changed(), 8);
        assert!(!s.is_clean());
    }

    #[test]
    fn detached_and_clean() {
        let s = GitStatus::parse_porcelain_v2("# branch.oid abc\n# branch.head (detached)\n");
        assert_eq!(s.branch, None);
        assert_eq!(s.oid.as_deref(), Some("abc"));
        assert!(s.is_clean());
        assert_eq!((s.ahead, s.behind), (0, 0));
    }

    #[test]
    fn parses_worktree_list() {
        let out = "\
worktree /home/sd/dev/api
HEAD 1111
branch refs/heads/main

worktree /home/sd/dev/api.worktrees/feat/x
HEAD 2222
branch refs/heads/feat/x

worktree /tmp/old
HEAD 3333
detached
prunable gitdir file points to non-existent location
";
        let list = parse_worktree_list(out);
        assert_eq!(list.len(), 3);
        assert_eq!(list[0].branch.as_deref(), Some("main"));
        assert_eq!(
            list[1].path,
            PathBuf::from("/home/sd/dev/api.worktrees/feat/x")
        );
        assert_eq!(list[1].branch.as_deref(), Some("feat/x"));
        assert!(list[2].detached && list[2].prunable && list[2].branch.is_none());
    }
}
