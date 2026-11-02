//! Agent hooks: the `deaner hook <agent> <event>` handler (called by the agent,
//! must be fast and must never fail the agent) and `deaner install-hooks`.

use anyhow::{Context, Result};
use deaner_core::agent::{CLAUDE_HOOKS, HookEffect, HookEvent, HookPayload, effect};
use deaner_core::pane::{OPT_AGENT, OPT_AGENT_SESSION, OPT_STATE};
use deaner_tmux::{Scope, Tmux};
use serde_json::{Map, Value, json};
use std::io::{IsTerminal, Read};
use std::path::{Path, PathBuf};

/// Marker that identifies hook commands deaner owns.
const MARKER: &str = " hook claude ";

/// Apply a hook event to the pane in `$TMUX_PANE`. Outside tmux this does nothing.
pub fn handle(
    tmux: &Tmux,
    pane: Option<&str>,
    agent: &str,
    event: HookEvent,
    payload: &HookPayload,
) {
    let Some(pane) = pane.filter(|p| !p.is_empty()) else {
        return;
    };
    let scope = Scope::Pane(pane);
    match effect(event, payload) {
        HookEffect::Set(state) => {
            let _ = tmux.set_option(scope, OPT_STATE, state.as_str());
            let _ = tmux.set_option(scope, OPT_AGENT, agent);
            if let Some(id) = payload.session_id.as_deref().filter(|s| !s.is_empty()) {
                let _ = tmux.set_option(scope, OPT_AGENT_SESSION, id);
            }
        }
        HookEffect::Clear => {
            for key in [OPT_STATE, OPT_AGENT, OPT_AGENT_SESSION] {
                let _ = tmux.unset_option(scope, key);
            }
        }
        HookEffect::Ignore => {}
    }
}

/// Entry point for `deaner hook`: reads the payload from stdin (if piped) and
/// updates the current pane. Always succeeds so the agent is never blocked.
pub fn run(agent: &str, event: HookEvent) {
    let mut raw = String::new();
    let stdin = std::io::stdin();
    if !stdin.is_terminal() {
        let _ = stdin.lock().take(1 << 20).read_to_string(&mut raw);
    }
    let payload = HookPayload::parse(&raw);
    let pane = std::env::var("TMUX_PANE").ok();
    handle(&Tmux::inherit(), pane.as_deref(), agent, event, &payload);
}

/// `~/.claude/settings.json`, honouring `CLAUDE_CONFIG_DIR`.
#[must_use]
pub fn claude_settings_path() -> PathBuf {
    std::env::var_os("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join(".claude"))
        .join("settings.json")
}

fn is_ours(hook: &Value) -> bool {
    hook.get("command")
        .and_then(Value::as_str)
        .is_some_and(|c| c.contains(MARKER) && c.contains("deaner"))
}

/// Remove deaner's entries from a settings document. Returns how many were removed.
fn strip(root: &mut Value) -> usize {
    let mut removed = 0;
    let Some(hooks) = root.get_mut("hooks").and_then(Value::as_object_mut) else {
        return 0;
    };
    for groups in hooks.values_mut() {
        let Some(groups) = groups.as_array_mut() else {
            continue;
        };
        for group in groups.iter_mut() {
            if let Some(list) = group.get_mut("hooks").and_then(Value::as_array_mut) {
                let before = list.len();
                list.retain(|h| !is_ours(h));
                removed += before - list.len();
            }
        }
        groups.retain(|g| {
            g.get("hooks")
                .and_then(Value::as_array)
                .is_none_or(|l| !l.is_empty())
        });
    }
    hooks.retain(|_, groups| groups.as_array().is_none_or(|g| !g.is_empty()));
    removed
}

/// Merge deaner's Claude hooks into a settings document (idempotent).
#[must_use]
pub fn merge(mut root: Value, command: &str) -> Value {
    if !root.is_object() {
        root = Value::Object(Map::new());
    }
    strip(&mut root);
    let hooks = root
        .as_object_mut()
        .expect("object")
        .entry("hooks")
        .or_insert_with(|| Value::Object(Map::new()));
    if !hooks.is_object() {
        *hooks = Value::Object(Map::new());
    }
    let hooks = hooks.as_object_mut().expect("object");
    for (event, matcher, ours) in CLAUDE_HOOKS {
        let mut group = Map::new();
        if let Some(m) = matcher {
            group.insert("matcher".into(), json!(m));
        }
        group.insert(
            "hooks".into(),
            json!([{ "type": "command", "command": format!("{command}{MARKER}{}", ours.as_str()), "timeout": 10 }]),
        );
        let list = hooks.entry(*event).or_insert_with(|| json!([]));
        if let Some(arr) = list.as_array_mut() {
            arr.push(Value::Object(group));
        }
    }
    root
}

/// What `install` did.
#[derive(Debug)]
pub struct InstallReport {
    pub path: PathBuf,
    pub backup: Option<PathBuf>,
    pub removed: usize,
}

/// Install (or with `uninstall`, remove) deaner's hooks in a Claude settings file.
///
/// # Errors
/// When the file is not valid JSON or cannot be written.
pub fn install(path: &Path, command: &str, uninstall: bool) -> Result<InstallReport> {
    let existing = match std::fs::read_to_string(path) {
        Ok(t) if !t.trim().is_empty() => Some(t),
        Ok(_) => None,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
    };
    let mut root: Value = match &existing {
        Some(t) => serde_json::from_str(t)
            .with_context(|| format!("{} is not valid JSON", path.display()))?,
        None => json!({}),
    };
    let removed = strip(&mut root);
    if !uninstall {
        root = merge(root, command);
    }
    let backup = match &existing {
        Some(t) => {
            let b = path.with_extension("json.deaner-backup");
            std::fs::write(&b, t)?;
            Some(b)
        }
        None => None,
    };
    let mut text = serde_json::to_string_pretty(&root)?;
    text.push('\n');
    crate::paths::write_atomic(path, &text)?;
    Ok(InstallReport {
        path: path.to_path_buf(),
        backup,
        removed,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_is_idempotent_and_keeps_user_hooks() {
        let user = json!({
            "model": "opus",
            "hooks": {
                "Stop": [{ "hooks": [{ "type": "command", "command": "say done" }] }]
            }
        });
        let once = merge(user, "deaner");
        let twice = merge(once.clone(), "deaner");
        assert_eq!(once, twice);
        assert_eq!(once["model"], "opus");
        let stop = once["hooks"]["Stop"].as_array().unwrap();
        assert_eq!(stop.len(), 2, "user hook kept, ours added");
        assert_eq!(stop[0]["hooks"][0]["command"], "say done");
        assert_eq!(stop[1]["hooks"][0]["command"], "deaner hook claude stop");
        assert_eq!(once["hooks"]["PreToolUse"][0]["matcher"], "");
        assert_eq!(
            once["hooks"]["PreToolUse"][0]["hooks"][0]["command"],
            "deaner hook claude pre-tool"
        );
    }

    #[test]
    fn install_and_uninstall_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(&path, r#"{"theme":"dark"}"#).unwrap();

        let r = install(&path, "/usr/bin/deaner", false).unwrap();
        assert!(r.backup.is_some());
        let installed: Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(installed["theme"], "dark");
        assert_eq!(installed["hooks"].as_object().unwrap().len(), 8);

        let r = install(&path, "/usr/bin/deaner", true).unwrap();
        assert_eq!(r.removed, 8);
        let cleaned: Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(cleaned, json!({"theme": "dark", "hooks": {}}));
    }

    #[test]
    fn creates_missing_settings() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested/settings.json");
        let r = install(&path, "deaner", false).unwrap();
        assert!(r.backup.is_none());
        assert!(path.exists());
    }
}
