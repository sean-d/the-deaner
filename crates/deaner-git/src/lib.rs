//! Runs `git` and `gh` and parses their output with `deaner-core`.
//!
//! deaner shells out instead of linking libgit2: no C dependencies, faster
//! builds, and behaviour identical to the user's own git (config, hooks,
//! credential helpers).

use deaner_core::pr::{GH_QUERY, PrStatus};
use deaner_core::process::output_with_timeout;
use deaner_core::{GitStatus, Worktree};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

/// Deadline for quick git queries (status, rev-parse, worktree list).
pub const GIT_TIMEOUT: Duration = Duration::from_secs(10);
/// Deadline for git commands that write a checkout (worktree add/remove).
pub const GIT_SLOW_TIMEOUT: Duration = Duration::from_secs(120);
/// Deadline for a background `git fetch`. Generous: one that has real data to download
/// must get to finish, or it would be killed and restarted from scratch every time.
pub const GIT_FETCH_TIMEOUT: Duration = Duration::from_secs(120);
/// Deadline for `gh` (network).
pub const GH_TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("could not run {program}: {source}")]
    Spawn {
        program: &'static str,
        source: std::io::Error,
    },
    #[error("{program} {args}: {stderr}")]
    Failed {
        program: &'static str,
        args: String,
        stderr: String,
    },
    #[error("{program} {args}: no answer within {}s", timeout.as_secs())]
    Timeout {
        program: &'static str,
        args: String,
        timeout: Duration,
    },
    #[error("unexpected output from gh: {0}")]
    Parse(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

fn join_args<S: AsRef<OsStr>>(args: &[S]) -> String {
    args.iter()
        .map(|a| a.as_ref().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join(" ")
}

fn run<S: AsRef<OsStr>>(
    program: &'static str,
    dir: &Path,
    args: &[S],
    timeout: Duration,
) -> Result<String> {
    let mut cmd = Command::new(program);
    cmd.current_dir(dir)
        .args(args)
        .env("GIT_OPTIONAL_LOCKS", "0") // status must not fight the user's git for index.lock
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GH_PROMPT_DISABLED", "1");
    let out = output_with_timeout(&mut cmd, timeout).map_err(|source| {
        if source.kind() == std::io::ErrorKind::TimedOut {
            Error::Timeout {
                program,
                args: join_args(args),
                timeout,
            }
        } else {
            Error::Spawn { program, source }
        }
    })?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(Error::Failed {
            program,
            args: join_args(args),
            stderr: String::from_utf8_lossy(&out.stderr).trim().to_string(),
        })
    }
}

fn git<S: AsRef<OsStr>>(dir: &Path, args: &[S]) -> Result<String> {
    run("git", dir, args, GIT_TIMEOUT)
}

fn git_slow<S: AsRef<OsStr>>(dir: &Path, args: &[S]) -> Result<String> {
    run("git", dir, args, GIT_SLOW_TIMEOUT)
}

/// Branch, ahead/behind and change counts for a checkout.
///
/// # Errors
/// When `path` is not a git checkout.
pub fn status(path: &Path) -> Result<GitStatus> {
    git(path, &["status", "--porcelain=v2", "--branch"])
        .map(|out| GitStatus::parse_porcelain_v2(&out))
}

/// Whether `path` is inside a git work tree.
#[must_use]
pub fn is_repo(path: &Path) -> bool {
    git(path, &["rev-parse", "--is-inside-work-tree"]).is_ok_and(|o| o.trim() == "true")
}

/// The main checkout root for `path`, even when `path` is inside a linked worktree.
///
/// # Errors
/// When `path` is not in a git repository.
pub fn main_root(path: &Path) -> Result<PathBuf> {
    let common = git(
        path,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )?;
    let common = PathBuf::from(common.trim());
    // `<root>/.git` for normal repos; bare repos have no work tree to return.
    if common.file_name().is_some_and(|n| n == ".git")
        && let Some(root) = common.parent()
    {
        return Ok(root.to_path_buf());
    }
    let top = git(path, &["rev-parse", "--show-toplevel"])?;
    Ok(PathBuf::from(top.trim()))
}

/// All worktrees of the repo at `repo`; the first is the main checkout.
///
/// # Errors
/// When `repo` is not a git repository.
pub fn worktrees(repo: &Path) -> Result<Vec<Worktree>> {
    git(repo, &["worktree", "list", "--porcelain"])
        .map(|out| deaner_core::git::parse_worktree_list(&out))
}

/// Whether a local branch exists.
#[must_use]
pub fn branch_exists(repo: &Path, branch: &str) -> bool {
    git(
        repo,
        &[
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("refs/heads/{branch}"),
        ],
    )
    .is_ok()
}

/// Create a worktree at `path` on `branch`, creating the branch from HEAD if
/// it does not exist locally (git picks up a matching `origin/<branch>`).
///
/// # Errors
/// When git refuses (path exists, branch checked out elsewhere, …).
pub fn add_worktree(repo: &Path, path: &Path, branch: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|source| Error::Spawn {
            program: "mkdir",
            source,
        })?;
    }
    let remote_exists = git(
        repo,
        &[
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("refs/remotes/origin/{branch}"),
        ],
    )
    .is_ok();
    let args: Vec<&OsStr> = if branch_exists(repo, branch) || remote_exists {
        vec![
            OsStr::new("worktree"),
            OsStr::new("add"),
            path.as_os_str(),
            OsStr::new(branch),
        ]
    } else {
        vec![
            OsStr::new("worktree"),
            OsStr::new("add"),
            OsStr::new("-b"),
            OsStr::new(branch),
            path.as_os_str(),
        ]
    };
    git_slow(repo, &args).map(drop)
}

/// Update remote-tracking branches, unattended. Repos without a remote are left alone.
///
/// Never prompts: ssh would otherwise open the terminal directly to ask for a
/// passphrase, which here would write into the sidebar. It also leaves `FETCH_HEAD`
/// alone and doesn't start git maintenance, since the user didn't ask for a fetch.
///
/// # Errors
/// When the fetch fails or times out (offline, no access).
pub fn fetch(repo: &Path) -> Result<()> {
    if git(repo, &["remote"])?.trim().is_empty() {
        return Ok(());
    }
    let ssh = git(repo, &["config", "--get", "core.sshCommand"]).unwrap_or_default();
    let ssh = match ssh.trim() {
        "" => "ssh",
        custom => custom,
    };
    let ssh = format!("core.sshCommand={ssh} -o BatchMode=yes");
    run(
        "git",
        repo,
        &[
            "-c",
            &ssh,
            "fetch",
            "--quiet",
            "--no-auto-maintenance",
            "--no-write-fetch-head",
        ],
        GIT_FETCH_TIMEOUT,
    )
    .map(drop)
}

/// Remove a linked worktree. Without `force`, git refuses when it has changes.
///
/// # Errors
/// When git refuses.
pub fn remove_worktree(repo: &Path, path: &Path, force: bool) -> Result<()> {
    let mut args = vec![OsStr::new("worktree"), OsStr::new("remove")];
    if force {
        args.push(OsStr::new("--force"));
    }
    args.push(path.as_os_str());
    git_slow(repo, &args).map(drop)
}

/// Whether the `gh` CLI is installed.
#[must_use]
pub fn gh_available() -> bool {
    output_with_timeout(Command::new("gh").arg("--version"), GH_TIMEOUT)
        .is_ok_and(|o| o.status.success())
}

/// The pull request to show for `branch`, or `None` if it has never had one.
///
/// # Errors
/// When gh can't answer (offline, not a GitHub repo, not logged in, timed out). Callers
/// keep showing the last known state rather than clearing the badge.
pub fn pr_status(path: &Path, branch: &str) -> Result<Option<PrStatus>> {
    let query = format!("query={GH_QUERY}");
    let branch_field = format!("branch={branch}");
    let json = run(
        "gh",
        path,
        &[
            "api",
            "graphql",
            "-f",
            &query,
            // -F fills in {owner} and {repo} from the current repo. The branch uses -f,
            // because -F would turn a branch named `123` into a number.
            "-F",
            "owner={owner}",
            "-F",
            "name={repo}",
            "-f",
            &branch_field,
        ],
        GH_TIMEOUT,
    )?;
    Ok(PrStatus::parse_graphql(&json, branch)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sh(dir: &Path, args: &[&str]) {
        let ok = Command::new("git")
            .current_dir(dir)
            .args(args)
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@t")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@t")
            .output()
            .unwrap()
            .status
            .success();
        assert!(ok, "git {args:?}");
    }

    fn repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        sh(dir.path(), &["init", "-q", "-b", "main"]);
        std::fs::write(dir.path().join("a.txt"), "a").unwrap();
        sh(dir.path(), &["add", "."]);
        sh(dir.path(), &["commit", "-qm", "init"]);
        dir
    }

    #[test]
    fn status_counts_changes() {
        let dir = repo();
        let root = dir.path();
        assert!(status(root).unwrap().is_clean());
        std::fs::write(root.join("a.txt"), "changed").unwrap();
        std::fs::write(root.join("new.txt"), "n").unwrap();
        let s = status(root).unwrap();
        assert_eq!(s.branch.as_deref(), Some("main"));
        assert_eq!((s.modified, s.untracked), (1, 1));
        assert!(is_repo(root));
    }

    #[test]
    fn fetch_updates_behind_count_without_touching_the_checkout() {
        let origin = tempfile::tempdir().unwrap();
        sh(origin.path(), &["init", "-q", "--bare", "-b", "main"]);
        let url = origin.path().display().to_string();
        let mine = repo();
        sh(mine.path(), &["remote", "add", "origin", &url]);
        sh(mine.path(), &["push", "-q", "-u", "origin", "main"]);

        // Someone else pushes.
        let theirs = tempfile::tempdir().unwrap();
        sh(theirs.path(), &["clone", "-q", &url, "."]);
        std::fs::write(theirs.path().join("b.txt"), "b").unwrap();
        sh(theirs.path(), &["add", "."]);
        sh(theirs.path(), &["commit", "-qm", "theirs"]);
        sh(theirs.path(), &["push", "-q"]);

        assert_eq!(status(mine.path()).unwrap().behind, 0, "not fetched yet");
        fetch(mine.path()).unwrap();
        assert_eq!(status(mine.path()).unwrap().behind, 1);
        assert!(!mine.path().join("b.txt").exists(), "fetch doesn't merge");
        assert!(!mine.path().join(".git/FETCH_HEAD").exists());
    }

    #[test]
    fn fetch_without_a_remote_is_fine_and_a_bad_remote_is_an_error() {
        let dir = repo();
        fetch(dir.path()).unwrap();
        sh(
            dir.path(),
            &["remote", "add", "origin", "/nonexistent/deaner-repo"],
        );
        assert!(fetch(dir.path()).is_err());
    }

    #[test]
    fn worktree_lifecycle() {
        let dir = repo();
        let root = dir.path().canonicalize().unwrap();
        let wt_parent = tempfile::tempdir().unwrap();
        let wt = wt_parent.path().canonicalize().unwrap().join("feat/x");

        assert!(!branch_exists(&root, "feat/x"));
        add_worktree(&root, &wt, "feat/x").unwrap();
        assert!(branch_exists(&root, "feat/x"));

        let list = worktrees(&root).unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].path, root);
        assert_eq!(list[1].branch.as_deref(), Some("feat/x"));

        // main_root resolves back to the main checkout from inside the worktree
        assert_eq!(main_root(&wt).unwrap().canonicalize().unwrap(), root);

        std::fs::write(wt.join("dirty.txt"), "x").unwrap();
        assert!(
            remove_worktree(&root, &wt, false).is_err(),
            "dirty worktree is protected"
        );
        remove_worktree(&root, &wt, true).unwrap();
        assert_eq!(worktrees(&root).unwrap().len(), 1);
    }
}
