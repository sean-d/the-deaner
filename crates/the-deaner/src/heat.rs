//! When to ask GitHub more often. Most of the time nothing changes there, so the sidebar asks
//! every minute (`pr_poll_seconds`). A project is *hot*, and asked every few seconds
//! (`pr_fast_seconds`), only while something is about to change:
//!
//! - its CI is running (pending), for at most [`PENDING_LIMIT`], so a stuck run can't keep it
//!   hot forever;
//! - for [`AFTER_PUSH`] after a push from it: the branch goes from commits ahead to in sync
//!   (or gets its upstream), which is when a run or a pull request is about to show up.
//!
//! GitHub allows 5,000 API calls an hour per user, shared with everything else using `gh`, so
//! asking every few seconds for every project would run out; for a hot one or two, briefly, it's
//! nothing.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// How long after a push a project stays hot.
pub const AFTER_PUSH: Duration = Duration::from_secs(3 * 60);
/// The longest a pending CI run keeps a project hot.
pub const PENDING_LIMIT: Duration = Duration::from_secs(20 * 60);

#[derive(Default)]
pub struct Heat {
    known: HashMap<PathBuf, Seen>,
}

struct Seen {
    /// Commits ahead of the upstream last time; `None` when there was no upstream.
    ahead: Option<u32>,
    pending_since: Option<Instant>,
    hot_until: Option<Instant>,
}

impl Heat {
    /// Take in what's known about `path` now (its commits ahead, whether its CI is pending) and
    /// say whether to ask GitHub about it often.
    pub fn observe(
        &mut self,
        path: &Path,
        ahead: Option<u32>,
        pending: bool,
        now: Instant,
    ) -> bool {
        let Some(seen) = self.known.get_mut(path) else {
            // First sight: nothing to compare with yet, so no push to see.
            self.known.insert(
                path.to_path_buf(),
                Seen {
                    ahead,
                    pending_since: pending.then_some(now),
                    hot_until: None,
                },
            );
            return pending;
        };
        let pushed = match (seen.ahead, ahead) {
            (Some(before), Some(0)) => before > 0,
            (None, Some(0)) => true, // first push of a new branch
            _ => false,
        };
        if pushed {
            seen.hot_until = Some(now + AFTER_PUSH);
        }
        seen.ahead = ahead;
        if pending {
            let since = *seen.pending_since.get_or_insert(now);
            if now.duration_since(since) < PENDING_LIMIT {
                return true;
            }
        } else {
            seen.pending_since = None;
        }
        seen.hot_until.is_some_and(|until| now < until)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hot_after_a_push_for_a_few_minutes() {
        let mut heat = Heat::default();
        let p = Path::new("/r/api");
        let t = Instant::now();
        assert!(
            !heat.observe(p, Some(0), false, t),
            "in sync from the start isn't a push"
        );
        assert!(
            !heat.observe(p, Some(2), false, t),
            "committing isn't either"
        );
        assert!(
            heat.observe(p, Some(0), false, t + Duration::from_secs(1)),
            "pushed"
        );
        assert!(heat.observe(p, Some(0), false, t + Duration::from_secs(120)));
        assert!(!heat.observe(p, Some(0), false, t + AFTER_PUSH + Duration::from_secs(2)));
    }

    #[test]
    fn hot_while_ci_runs_but_not_forever() {
        let mut heat = Heat::default();
        let p = Path::new("/r/web");
        let t = Instant::now();
        assert!(heat.observe(p, Some(0), true, t), "pending from the start");
        assert!(heat.observe(p, Some(0), true, t + Duration::from_secs(600)));
        assert!(
            !heat.observe(p, Some(0), true, t + PENDING_LIMIT),
            "stuck: back to normal"
        );
        assert!(
            !heat.observe(p, Some(0), false, t + PENDING_LIMIT),
            "finished"
        );
        // A new run starts the clock again.
        let later = t + PENDING_LIMIT * 2;
        assert!(heat.observe(p, Some(0), true, later));
    }

    #[test]
    fn a_new_branch_getting_its_upstream_counts_as_a_push() {
        let mut heat = Heat::default();
        let p = Path::new("/r/api.worktrees/feat");
        let t = Instant::now();
        assert!(!heat.observe(p, None, false, t));
        assert!(heat.observe(p, Some(0), false, t + Duration::from_secs(3)));
    }
}
