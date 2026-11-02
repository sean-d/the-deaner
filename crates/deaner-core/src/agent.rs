//! Agent presence: the states a pane's agent can be in, and how agent hook
//! events map onto them.
//!
//! Agents report through `deaner hook <agent> <event>`, which stores the state
//! as a tmux pane option. The mapping here is the single source of truth for
//! both the hook handler and `deaner install-hooks`.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

/// What an agent in a pane is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AgentState {
    /// No agent known in this pane.
    #[default]
    None,
    /// Agent is running and waiting for a new prompt.
    Idle,
    /// Agent is working on a turn.
    Busy,
    /// Agent is compacting its context.
    Compacting,
    /// Agent is blocked on the user: a permission prompt, a question, or plan approval.
    Awaiting,
}

impl AgentState {
    /// The value stored in the `@deaner_state` pane option.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "",
            Self::Idle => "idle",
            Self::Busy => "busy",
            Self::Compacting => "compacting",
            Self::Awaiting => "awaiting",
        }
    }

    /// Parse a stored option value. Unknown values read as [`AgentState::None`].
    #[must_use]
    pub fn parse(raw: &str) -> Self {
        match raw.trim() {
            "idle" => Self::Idle,
            "busy" => Self::Busy,
            "compacting" => Self::Compacting,
            "awaiting" => Self::Awaiting,
            _ => Self::None,
        }
    }

    /// How urgently this state wants the user's attention; used to roll many
    /// panes up into one badge.
    #[must_use]
    pub const fn urgency(self) -> u8 {
        match self {
            Self::None => 0,
            Self::Idle => 1,
            Self::Compacting => 2,
            Self::Busy => 3,
            Self::Awaiting => 4,
        }
    }

    /// The most urgent of `states`.
    pub fn rollup(states: impl IntoIterator<Item = Self>) -> Self {
        states
            .into_iter()
            .max_by_key(|s| s.urgency())
            .unwrap_or_default()
    }
}

impl fmt::Display for AgentState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(if *self == Self::None {
            "none"
        } else {
            self.as_str()
        })
    }
}

/// An event an agent hook reports, as passed on the `deaner hook` command line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookEvent {
    SessionStart,
    /// A prompt was submitted or a tool finished; the turn is in progress.
    Busy,
    /// A tool is about to run. Questions and plan approval mean the agent waits on the user.
    PreTool,
    Notification,
    Compacting,
    Stop,
    SessionEnd,
}

impl HookEvent {
    pub const ALL: [Self; 7] = [
        Self::SessionStart,
        Self::Busy,
        Self::PreTool,
        Self::Notification,
        Self::Compacting,
        Self::Stop,
        Self::SessionEnd,
    ];

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SessionStart => "session-start",
            Self::Busy => "busy",
            Self::PreTool => "pre-tool",
            Self::Notification => "notification",
            Self::Compacting => "compacting",
            Self::Stop => "stop",
            Self::SessionEnd => "session-end",
        }
    }
}

impl FromStr for HookEvent {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|e| e.as_str() == s)
            .ok_or_else(|| {
                let valid: Vec<_> = Self::ALL.iter().map(|e| e.as_str()).collect();
                format!(
                    "unknown hook event `{s}` (expected one of: {})",
                    valid.join(", ")
                )
            })
    }
}

/// The parts of an agent's hook stdin payload deaner cares about. Everything is
/// optional: a malformed or empty payload must never break the agent.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct HookPayload {
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub tool_name: Option<String>,
    #[serde(default)]
    pub notification_type: Option<String>,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    pub source: Option<String>,
}

impl HookPayload {
    /// Parse leniently; anything unparseable becomes an empty payload.
    #[must_use]
    pub fn parse(raw: &str) -> Self {
        serde_json::from_str(raw).unwrap_or_default()
    }
}

/// Tools that mean the agent is waiting on the user rather than working.
pub const AWAITING_TOOLS: &[&str] = &["AskUserQuestion", "ExitPlanMode"];

/// What a hook event means for the pane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HookEffect {
    /// Set the state (and record the session id when one is present).
    Set(AgentState),
    /// The agent exited: clear state, agent and session id.
    Clear,
    /// Nothing to change.
    Ignore,
}

/// Decide what `event` (with its `payload`) does to the pane's state.
#[must_use]
pub fn effect(event: HookEvent, payload: &HookPayload) -> HookEffect {
    match event {
        HookEvent::SessionStart | HookEvent::Stop => HookEffect::Set(AgentState::Idle),
        HookEvent::Busy => HookEffect::Set(AgentState::Busy),
        HookEvent::PreTool => {
            let waits = payload
                .tool_name
                .as_deref()
                .is_some_and(|t| AWAITING_TOOLS.contains(&t));
            HookEffect::Set(if waits {
                AgentState::Awaiting
            } else {
                AgentState::Busy
            })
        }
        HookEvent::Notification => {
            // "Waiting for your input" fires after a minute of idling; that is
            // not a blocked agent, so leave the state alone.
            let idle_nudge = payload.notification_type.as_deref() == Some("idle_prompt")
                || payload
                    .message
                    .as_deref()
                    .is_some_and(|m| m.contains("waiting for your input"));
            if idle_nudge {
                HookEffect::Ignore
            } else {
                HookEffect::Set(AgentState::Awaiting)
            }
        }
        HookEvent::Compacting => HookEffect::Set(AgentState::Compacting),
        HookEvent::SessionEnd => HookEffect::Clear,
    }
}

/// One entry of the Claude Code hook map: (hook event name, matcher, deaner event).
pub type ClaudeHook = (&'static str, Option<&'static str>, HookEvent);

/// The hooks `deaner install-hooks claude` writes into `~/.claude/settings.json`.
///
/// PreToolUse is a single catch-all entry; the handler inspects `tool_name`,
/// because Claude runs matching hooks in parallel and two entries would race.
pub const CLAUDE_HOOKS: &[ClaudeHook] = &[
    ("SessionStart", None, HookEvent::SessionStart),
    ("UserPromptSubmit", None, HookEvent::Busy),
    ("PreToolUse", Some(""), HookEvent::PreTool),
    ("PostToolUse", Some(""), HookEvent::Busy),
    ("Notification", Some(""), HookEvent::Notification),
    ("PreCompact", None, HookEvent::Compacting),
    ("Stop", None, HookEvent::Stop),
    ("SessionEnd", Some(""), HookEvent::SessionEnd),
];

#[cfg(test)]
mod tests {
    use super::*;

    fn payload(json: &str) -> HookPayload {
        HookPayload::parse(json)
    }

    #[test]
    fn state_round_trips_through_option_value() {
        for s in [
            AgentState::Idle,
            AgentState::Busy,
            AgentState::Compacting,
            AgentState::Awaiting,
        ] {
            assert_eq!(AgentState::parse(s.as_str()), s);
        }
        assert_eq!(AgentState::parse("garbage"), AgentState::None);
    }

    #[test]
    fn rollup_prefers_awaiting() {
        let s = AgentState::rollup([AgentState::Idle, AgentState::Awaiting, AgentState::Busy]);
        assert_eq!(s, AgentState::Awaiting);
        assert_eq!(AgentState::rollup([]), AgentState::None);
    }

    #[test]
    fn pre_tool_distinguishes_questions() {
        let ask = payload(r#"{"tool_name":"AskUserQuestion"}"#);
        let bash = payload(r#"{"tool_name":"Bash"}"#);
        assert_eq!(
            effect(HookEvent::PreTool, &ask),
            HookEffect::Set(AgentState::Awaiting)
        );
        assert_eq!(
            effect(HookEvent::PreTool, &bash),
            HookEffect::Set(AgentState::Busy)
        );
    }

    #[test]
    fn idle_notification_is_ignored_but_permission_awaits() {
        let idle = payload(r#"{"notification_type":"idle_prompt"}"#);
        let idle_msg = payload(r#"{"message":"Claude is waiting for your input"}"#);
        let perm = payload(r#"{"message":"Claude needs your permission to use Bash"}"#);
        assert_eq!(effect(HookEvent::Notification, &idle), HookEffect::Ignore);
        assert_eq!(
            effect(HookEvent::Notification, &idle_msg),
            HookEffect::Ignore
        );
        assert_eq!(
            effect(HookEvent::Notification, &perm),
            HookEffect::Set(AgentState::Awaiting)
        );
    }

    #[test]
    fn malformed_payload_is_empty() {
        let p = payload("not json");
        assert!(p.session_id.is_none());
        assert_eq!(
            effect(HookEvent::Stop, &p),
            HookEffect::Set(AgentState::Idle)
        );
        assert_eq!(effect(HookEvent::SessionEnd, &p), HookEffect::Clear);
    }

    #[test]
    fn hook_events_parse_from_cli_names() {
        for e in HookEvent::ALL {
            assert_eq!(e.as_str().parse::<HookEvent>().unwrap(), e);
        }
        assert!("nope".parse::<HookEvent>().is_err());
    }
}
