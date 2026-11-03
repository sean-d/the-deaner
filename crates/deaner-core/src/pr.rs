//! The pull request for a branch, from GitHub's GraphQL API.
//!
//! GraphQL rather than `gh pr view <branch>`, which finds nothing for long-lived
//! branches like `staging` when a repo's PRs from them have all been merged. Those are
//! exactly the rows where the last PR's check result is worth showing.

use serde::{Deserialize, Serialize};

/// Open and merged PRs for a head branch, most recently updated first.
pub const GH_QUERY: &str = r"query($owner: String!, $name: String!, $branch: String!) {
  repository(owner: $owner, name: $name) {
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
    pub checks: Checks,
    /// `APPROVED`, `CHANGES_REQUESTED`, `REVIEW_REQUIRED`, or `None`.
    pub review: Option<String>,
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
    pull_requests: PullRequests,
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

    fn into_status(self) -> PrStatus {
        let state = match (self.state.as_str(), self.is_draft) {
            ("MERGED", _) => PrState::Merged,
            ("CLOSED", _) => PrState::Closed,
            (_, true) => PrState::Draft,
            _ => PrState::Open,
        };
        let checks = self
            .commits
            .nodes
            .into_iter()
            .next()
            .and_then(|n| n.commit.status_check_rollup)
            .map_or(Checks::None, |r| match r.state.as_str() {
                "SUCCESS" => Checks::Passing,
                "FAILURE" | "ERROR" => Checks::Failing,
                _ => Checks::Pending,
            });
        PrStatus {
            number: self.number,
            state,
            title: self.title,
            url: self.url,
            checks,
            review: self.review_decision.filter(|r| !r.is_empty()),
        }
    }
}

impl PrStatus {
    /// Pick the PR to show for `branch` from a [`GH_QUERY`] response: the most recently
    /// updated open one, otherwise the most recently updated merged one.
    ///
    /// # Errors
    /// Returns the JSON error when `json` isn't a query response.
    pub fn parse_graphql(json: &str, branch: &str) -> Result<Option<Self>, serde_json::Error> {
        let response: Response = serde_json::from_str(json)?;
        let mut nodes: Vec<Node> = response
            .data
            .repository
            .pull_requests
            .nodes
            .into_iter()
            .filter(|n| n.belongs_to(branch))
            .collect();
        let chosen = nodes
            .iter()
            .position(|n| n.state == "OPEN")
            .or((!nodes.is_empty()).then_some(0));
        Ok(chosen.map(|i| nodes.swap_remove(i).into_status()))
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

    fn response(nodes: &[String]) -> String {
        format!(
            r#"{{"data":{{"repository":{{"pullRequests":{{"nodes":[{}]}}}}}}}}"#,
            nodes.join(",")
        )
    }

    #[test]
    fn long_lived_branch_shows_its_last_merged_pr() {
        let json = response(&[
            node(480, "MERGED", Some("SUCCESS")),
            node(477, "MERGED", Some("FAILURE")),
        ]);
        let pr = PrStatus::parse_graphql(&json, "staging").unwrap().unwrap();
        assert_eq!(pr.number, 480);
        assert_eq!(pr.state, PrState::Merged);
        assert_eq!(pr.checks, Checks::Passing);
    }

    #[test]
    fn an_open_pr_wins_over_a_newer_merged_one() {
        let json = response(&[
            node(9, "MERGED", Some("SUCCESS")),
            node(8, "OPEN", Some("PENDING")),
        ]);
        let pr = PrStatus::parse_graphql(&json, "feat").unwrap().unwrap();
        assert_eq!(pr.number, 8);
        assert_eq!(pr.state, PrState::Open);
        assert_eq!(pr.checks, Checks::Pending);
    }

    #[test]
    fn check_states() {
        let checks = |rollup| {
            let json = response(&[node(1, "OPEN", rollup)]);
            PrStatus::parse_graphql(&json, "b").unwrap().unwrap().checks
        };
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
        let json = response(&[fork]);
        assert_eq!(PrStatus::parse_graphql(&json, "main").unwrap(), None);
        assert_eq!(
            PrStatus::parse_graphql(&json, "feature")
                .unwrap()
                .unwrap()
                .number,
            5,
            "a fork's feature branch into our main does match"
        );
    }

    #[test]
    fn no_prs_and_bad_output() {
        assert_eq!(PrStatus::parse_graphql(&response(&[]), "b").unwrap(), None);
        assert!(PrStatus::parse_graphql("not json", "b").is_err());
    }
}
