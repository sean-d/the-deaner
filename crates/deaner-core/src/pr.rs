//! Pull request summary parsed from
//! `gh pr view --json number,state,isDraft,title,url,statusCheckRollup,reviewDecision`.

use serde::{Deserialize, Serialize};

/// The `--json` fields deaner asks `gh pr view` for.
pub const GH_FIELDS: &str = "number,state,isDraft,title,url,statusCheckRollup,reviewDecision";

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
struct RawPr {
    number: u64,
    state: String,
    #[serde(default)]
    is_draft: bool,
    #[serde(default)]
    title: String,
    #[serde(default)]
    url: String,
    #[serde(default)]
    status_check_rollup: Option<Vec<RawCheck>>,
    #[serde(default)]
    review_decision: Option<String>,
}

/// Either a `CheckRun` (status + conclusion) or a `StatusContext` (state).
#[derive(Deserialize)]
struct RawCheck {
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    conclusion: Option<String>,
    #[serde(default)]
    state: Option<String>,
}

impl RawCheck {
    fn outcome(&self) -> Checks {
        let pending = |s: &str| {
            matches!(
                s,
                "PENDING" | "QUEUED" | "IN_PROGRESS" | "WAITING" | "REQUESTED" | "EXPECTED"
            )
        };
        let failing = |s: &str| {
            matches!(
                s,
                "FAILURE"
                    | "ERROR"
                    | "CANCELLED"
                    | "TIMED_OUT"
                    | "ACTION_REQUIRED"
                    | "STARTUP_FAILURE"
            )
        };
        if let Some(state) = self.state.as_deref() {
            return if failing(state) {
                Checks::Failing
            } else if pending(state) {
                Checks::Pending
            } else {
                Checks::Passing
            };
        }
        if self.status.as_deref().is_some_and(|s| s != "COMPLETED") {
            return Checks::Pending;
        }
        match self.conclusion.as_deref() {
            Some(c) if failing(c) => Checks::Failing,
            Some("") | None => Checks::Pending,
            _ => Checks::Passing,
        }
    }
}

impl PrStatus {
    /// Parse `gh pr view --json …` output.
    ///
    /// # Errors
    /// Returns the JSON error when `json` is not a PR object.
    pub fn parse_gh(json: &str) -> Result<Self, serde_json::Error> {
        let raw: RawPr = serde_json::from_str(json)?;
        let state = match (raw.state.as_str(), raw.is_draft) {
            ("MERGED", _) => PrState::Merged,
            ("CLOSED", _) => PrState::Closed,
            (_, true) => PrState::Draft,
            _ => PrState::Open,
        };
        let checks = raw
            .status_check_rollup
            .unwrap_or_default()
            .iter()
            .map(RawCheck::outcome)
            .fold(Checks::None, |acc, c| match (acc, c) {
                (Checks::Failing, _) | (_, Checks::Failing) => Checks::Failing,
                (Checks::Pending, _) | (_, Checks::Pending) => Checks::Pending,
                _ => Checks::Passing,
            });
        Ok(Self {
            number: raw.number,
            state,
            title: raw.title,
            url: raw.url,
            checks,
            review: raw.review_decision.filter(|r| !r.is_empty()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mixed_checks_fail_wins() {
        let json = r#"{"number":42,"state":"OPEN","isDraft":false,"title":"Bump deps","url":"u",
          "statusCheckRollup":[
            {"__typename":"CheckRun","status":"COMPLETED","conclusion":"SUCCESS"},
            {"__typename":"CheckRun","status":"IN_PROGRESS","conclusion":""},
            {"__typename":"StatusContext","state":"FAILURE"}],
          "reviewDecision":"APPROVED"}"#;
        let pr = PrStatus::parse_gh(json).unwrap();
        assert_eq!(pr.number, 42);
        assert_eq!(pr.state, PrState::Open);
        assert_eq!(pr.checks, Checks::Failing);
        assert_eq!(pr.review.as_deref(), Some("APPROVED"));
    }

    #[test]
    fn pending_and_passing() {
        let pending = r#"{"number":1,"state":"OPEN","isDraft":true,"statusCheckRollup":[
            {"status":"COMPLETED","conclusion":"SUCCESS"},{"status":"QUEUED"}],"reviewDecision":""}"#;
        let pr = PrStatus::parse_gh(pending).unwrap();
        assert_eq!(pr.state, PrState::Draft);
        assert_eq!(pr.checks, Checks::Pending);
        assert_eq!(pr.review, None);

        let passing = r#"{"number":2,"state":"MERGED","statusCheckRollup":[
            {"status":"COMPLETED","conclusion":"SKIPPED"},{"state":"SUCCESS"}]}"#;
        let pr = PrStatus::parse_gh(passing).unwrap();
        assert_eq!(pr.state, PrState::Merged);
        assert_eq!(pr.checks, Checks::Passing);
    }

    #[test]
    fn no_checks() {
        let pr = PrStatus::parse_gh(r#"{"number":3,"state":"CLOSED","statusCheckRollup":null}"#)
            .unwrap();
        assert_eq!(pr.state, PrState::Closed);
        assert_eq!(pr.checks, Checks::None);
    }
}
