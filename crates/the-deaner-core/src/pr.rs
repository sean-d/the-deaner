//! A branch's pull request and CI status, from GitHub's GraphQL API.
//!
//! GraphQL rather than `gh pr view <branch>`, which finds nothing for long-lived
//! branches like `staging` when a repo's PRs from them have all been merged. Those are
//! exactly the rows where the last PR's check result is worth showing.

use serde::{Deserialize, Serialize};

/// Open and merged PRs for a head branch, most recently updated first, and the CI status
/// of the branch's latest commit on GitHub. `$ref` is `refs/heads/<branch>`.
pub const GH_QUERY: &str = r"query($owner: String!, $name: String!, $branch: String!, $ref: String!) {
  repository(owner: $owner, name: $name) {
    ref(qualifiedName: $ref) {
      target { ... on Commit { statusCheckRollup { state } } }
    }
    pullRequests(first: 5, states: [OPEN, MERGED], headRefName: $branch,
                 orderBy: {field: UPDATED_AT, direction: DESC}) {
      nodes {
        number state isDraft title url reviewDecision
        baseRefName isCrossRepository
        commits(last: 1) { nodes { commit { statusCheckRollup { state } } } }
      }
    }
  }
}";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PrState {
    Open,
    Draft,
    Merged,
    Closed,
}

/// Rolled-up CI status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Checks {
    #[default]
    None,
    Pending,
    Passing,
    Failing,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrStatus {
    pub number: u64,
    pub state: PrState,
    pub title: String,
    pub url: String,
    /// `APPROVED`, `CHANGES_REQUESTED`, `REVIEW_REQUIRED`, or `None`.
    pub review: Option<String>,
}

/// What the sidebar shows for a branch.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BranchStatus {
    pub pr: Option<PrStatus>,
    pub checks: Checks,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Node {
    number: u64,
    state: String,
    #[serde(default)]
    is_draft: bool,
    #[serde(default)]
    title: String,
    #[serde(default)]
    url: String,
    #[serde(default)]
    review_decision: Option<String>,
    #[serde(default)]
    base_ref_name: Option<String>,
    #[serde(default)]
    is_cross_repository: bool,
    commits: Commits,
}

#[derive(Deserialize)]
struct Commits {
    nodes: Vec<CommitNode>,
}

#[derive(Deserialize)]
struct CommitNode {
    commit: Commit,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Commit {
    #[serde(default)]
    status_check_rollup: Option<Rollup>,
}

#[derive(Deserialize)]
struct Rollup {
    state: String,
}

#[derive(Deserialize)]
struct Response {
    data: Data,
}

#[derive(Deserialize)]
struct Data {
    repository: Repository,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Repository {
    /// The branch on GitHub; `None` when it has never been pushed.
    #[serde(rename = "ref", default)]
    tip: Option<Tip>,
    pull_requests: PullRequests,
}

#[derive(Deserialize)]
struct Tip {
    #[serde(default)]
    target: Option<Commit>,
}

fn checks_from(rollup: Option<Rollup>) -> Checks {
    rollup.map_or(Checks::None, |r| match r.state.as_str() {
        "SUCCESS" => Checks::Passing,
        "FAILURE" | "ERROR" => Checks::Failing,
        _ => Checks::Pending,
    })
}

#[derive(Deserialize)]
struct PullRequests {
    nodes: Vec<Node>,
}

impl Node {
    /// A fork's `main` being merged into our `main` is about their branch, not ours.
    fn belongs_to(&self, branch: &str) -> bool {
        !self.is_cross_repository || self.base_ref_name.as_deref() != Some(branch)
    }

    fn checks(&mut self) -> Checks {
        let rollup = self
            .commits
            .nodes
            .first_mut()
            .and_then(|n| n.commit.status_check_rollup.take());
        checks_from(rollup)
    }

    fn into_status(self) -> PrStatus {
        let state = match (self.state.as_str(), self.is_draft) {
            ("MERGED", _) => PrState::Merged,
            ("CLOSED", _) => PrState::Closed,
            (_, true) => PrState::Draft,
            _ => PrState::Open,
        };
        PrStatus {
            number: self.number,
            state,
            title: self.title,
            url: self.url,
            review: self.review_decision.filter(|r| !r.is_empty()),
        }
    }
}

impl BranchStatus {
    /// Build a branch's status from a [`GH_QUERY`] response.
    ///
    /// The PR shown is the most recently updated open one, otherwise the most recently
    /// updated merged one. CI comes from an open PR, otherwise the branch's latest commit
    /// on GitHub, which is what's current for `main` or `staging`. If that commit has no
    /// checks, the merged PR's result is the best remaining answer.
    ///
    /// # Errors
    /// Returns the JSON error when `json` isn't a query response.
    pub fn parse_graphql(json: &str, branch: &str) -> Result<Self, serde_json::Error> {
        let response: Response = serde_json::from_str(json)?;
        let repository = response.data.repository;
        let tip = checks_from(
            repository
                .tip
                .and_then(|t| t.target)
                .and_then(|c| c.status_check_rollup),
        );
        let mut nodes: Vec<Node> = repository
            .pull_requests
            .nodes
            .into_iter()
            .filter(|n| n.belongs_to(branch))
            .collect();
        let open = nodes.iter().position(|n| n.state == "OPEN");
        let chosen = open.or((!nodes.is_empty()).then_some(0));
        let Some(chosen) = chosen else {
            return Ok(Self {
                pr: None,
                checks: tip,
            });
        };
        let mut node = nodes.swap_remove(chosen);
        let own = node.checks();
        let checks = if open.is_some() || tip == Checks::None {
            own
        } else {
            tip
        };
        Ok(Self {
            pr: Some(node.into_status()),
            checks,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(number: u64, state: &str, rollup: Option<&str>) -> String {
        let rollup = rollup.map_or("null".to_string(), |r| format!(r#"{{"state":"{r}"}}"#));
        format!(
            r#"{{"number":{number},"state":"{state}","isDraft":false,"title":"t","url":"u",
            "reviewDecision":null,"baseRefName":"main","isCrossRepository":false,
            "commits":{{"nodes":[{{"commit":{{"statusCheckRollup":{rollup}}}}}]}}}}"#
        )
    }

    /// `tip`: `None` for a branch GitHub doesn't have, `Some(None)` for a commit without CI.
    fn response(tip: Option<Option<&str>>, nodes: &[String]) -> String {
        let tip = match tip {
            None => "null".to_string(),
            Some(None) => r#"{"target":{"statusCheckRollup":null}}"#.to_string(),
            Some(Some(s)) => format!(r#"{{"target":{{"statusCheckRollup":{{"state":"{s}"}}}}}}"#),
        };
        format!(
            r#"{{"data":{{"repository":{{"ref":{tip},"pullRequests":{{"nodes":[{}]}}}}}}}}"#,
            nodes.join(",")
        )
    }

    fn parse(json: &str, branch: &str) -> BranchStatus {
        BranchStatus::parse_graphql(json, branch).unwrap()
    }

    #[test]
    fn a_branch_with_no_pr_still_reports_ci() {
        // Commits pushed straight to main.
        let s = parse(&response(Some(Some("SUCCESS")), &[]), "main");
        assert_eq!(s.pr, None);
        assert_eq!(s.checks, Checks::Passing);
        let s = parse(&response(Some(Some("PENDING")), &[]), "main");
        assert_eq!(s.checks, Checks::Pending);
    }

    #[test]
    fn current_ci_beats_a_merged_prs_old_result() {
        // staging's last PR passed, but what's on the branch now is failing.
        let json = response(
            Some(Some("FAILURE")),
            &[
                node(480, "MERGED", Some("SUCCESS")),
                node(477, "MERGED", Some("FAILURE")),
            ],
        );
        let s = parse(&json, "staging");
        let pr = s.pr.unwrap();
        assert_eq!(pr.number, 480);
        assert_eq!(pr.state, PrState::Merged);
        assert_eq!(s.checks, Checks::Failing);
    }

    #[test]
    fn a_merged_prs_result_is_the_fallback() {
        let json = response(Some(None), &[node(480, "MERGED", Some("SUCCESS"))]);
        assert_eq!(parse(&json, "staging").checks, Checks::Passing);
        let json = response(None, &[node(480, "MERGED", Some("FAILURE"))]);
        assert_eq!(parse(&json, "staging").checks, Checks::Failing);
    }

    #[test]
    fn an_open_pr_wins_over_a_newer_merged_one_and_uses_its_own_ci() {
        let json = response(
            Some(Some("FAILURE")),
            &[
                node(9, "MERGED", Some("SUCCESS")),
                node(8, "OPEN", Some("PENDING")),
            ],
        );
        let s = parse(&json, "feat");
        let pr = s.pr.unwrap();
        assert_eq!(pr.number, 8);
        assert_eq!(pr.state, PrState::Open);
        assert_eq!(s.checks, Checks::Pending);
    }

    #[test]
    fn check_states() {
        let checks = |rollup| parse(&response(None, &[node(1, "OPEN", rollup)]), "b").checks;
        assert_eq!(checks(Some("SUCCESS")), Checks::Passing);
        assert_eq!(checks(Some("FAILURE")), Checks::Failing);
        assert_eq!(checks(Some("ERROR")), Checks::Failing);
        assert_eq!(checks(Some("PENDING")), Checks::Pending);
        assert_eq!(checks(Some("EXPECTED")), Checks::Pending);
        assert_eq!(checks(None), Checks::None);
    }

    #[test]
    fn a_forks_branch_with_our_name_is_not_ours() {
        let fork = node(5, "OPEN", Some("SUCCESS")).replace(
            r#""isCrossRepository":false"#,
            r#""isCrossRepository":true"#,
        );
        let json = response(Some(None), &[fork]);
        assert_eq!(parse(&json, "main").pr, None);
        assert_eq!(parse(&json, "main").checks, Checks::None);
        assert_eq!(
            parse(&json, "feature").pr.unwrap().number,
            5,
            "a fork's feature branch into our main does match"
        );
    }

    #[test]
    fn nothing_known_and_bad_output() {
        assert_eq!(parse(&response(None, &[]), "b"), BranchStatus::default());
        assert!(BranchStatus::parse_graphql("not json", "b").is_err());
    }
}
