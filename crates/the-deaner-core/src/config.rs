//! User configuration (`~/.config/the-deaner/config.toml`) and per-repo
//! configuration (`<repo>/.the-deaner.toml`).
//!
//! Every field has a default, so an empty or missing file is a valid config.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// A tab to open in a new workspace, or offered by "new tab".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TabSpec {
    pub name: String,
    /// Command typed into the tab's shell. `None` leaves a plain shell.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
}

impl TabSpec {
    #[must_use]
    pub fn new(name: &str, command: Option<&str>) -> Self {
        Self {
            name: name.to_string(),
            command: command.map(str::to_string),
        }
    }
}

/// How to launch and resume a coding agent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentDef {
    /// Command that starts the agent, e.g. `claude`.
    pub command: String,
    /// Command that resumes a specific conversation; `{session_id}` is substituted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resume: Option<String>,
}

/// Keys bound in the outer frame (tmux root table, so they work from any pane).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct FrameKeys {
    /// Move focus between the sidebar and the terminal.
    pub focus: String,
    /// Hide or show the sidebar.
    pub toggle_sidebar: String,
    /// Open the searchable shortcut cheat sheet.
    pub cheatsheet: String,
}

impl Default for FrameKeys {
    fn default() -> Self {
        Self {
            focus: "M-s".into(),
            toggle_sidebar: "M-e".into(),
            cheatsheet: "M-/".into(),
        }
    }
}

/// Which icons the sidebar draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IconStyle {
    /// Nerd Font glyphs (needs a patched font, or a terminal with them built in).
    #[default]
    Nerd,
    /// Plain Unicode symbols that work in any font.
    Unicode,
}

/// How the sidebar marks the project shown in the terminal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OpenStyle {
    /// A thin gold box around the project, its name in the top border.
    #[default]
    Box,
    /// A dark gold block with a bar down its left edge.
    Block,
}

/// What kind of thing runs in a tab, which decides how it is restored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TabKind {
    Agent(String),
    Editor,
    Shell,
    Command,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Sidebar width in columns when no width has been saved yet.
    pub sidebar_width: u16,
    /// Sidebar icon set.
    pub icons: IconStyle,
    /// How the open project is marked.
    pub open_project: OpenStyle,
    /// Snapshot everything this often (minutes). 0 disables autosave.
    pub autosave_minutes: u64,
    /// How many timestamped snapshots to keep.
    pub snapshots_keep: usize,
    /// Float active repos with an agent awaiting input to the top of the active section.
    pub float_attention: bool,
    pub git_poll_seconds: u64,
    /// How often to ask `gh` about pull requests and CI for running projects. Others are
    /// asked 5x less often. 0 disables both.
    pub pr_poll_seconds: u64,
    /// How often to ask instead while a project's CI is running or right after a push from
    /// it. 0 turns that off.
    pub pr_fast_seconds: u64,
    /// How often to `git fetch` repos with a running session, so ahead/behind stays
    /// current. 0 disables background fetching.
    pub fetch_seconds: u64,
    /// Ask GitHub once a day whether a newer the-deaner is out, and say so in the sidebar.
    /// It never installs anything; that's `the-deaner upgrade`.
    pub update_check: bool,
    /// Where new worktrees go. `{repo_parent}`, `{repo}` and `{branch}` are substituted.
    pub worktree_dir: String,
    /// Process names that count as "just a shell" (nothing to restore).
    pub shells: Vec<String>,
    /// Editors that are relaunched on restore even if the-deaner did not start them.
    pub editors: Vec<String>,
    pub keys: FrameKeys,
    /// Tabs a new workspace starts with (overridden by a repo's `.the-deaner.toml`).
    pub tabs: Vec<TabSpec>,
    /// Known agents, merged over the built-in `claude` and `codex` definitions.
    pub agents: BTreeMap<String, AgentDef>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            sidebar_width: 32,
            icons: IconStyle::Nerd,
            open_project: OpenStyle::Box,
            autosave_minutes: 5,
            snapshots_keep: 20,
            float_attention: true,
            git_poll_seconds: 3,
            pr_poll_seconds: 60,
            pr_fast_seconds: 5,
            fetch_seconds: 60,
            update_check: true,
            worktree_dir: "{repo_parent}/{repo}.worktrees/{branch}".into(),
            shells: ["zsh", "bash", "fish", "sh", "dash", "nu", "elvish", "xonsh"]
                .map(String::from)
                .to_vec(),
            editors: ["nvim", "vim", "hx", "helix", "emacs"]
                .map(String::from)
                .to_vec(),
            keys: FrameKeys::default(),
            tabs: vec![
                TabSpec::new("nvim", Some("nvim")),
                TabSpec::new("claude", Some("claude")),
                TabSpec::new("shell", None),
            ],
            agents: builtin_agents(),
        }
    }
}

fn builtin_agents() -> BTreeMap<String, AgentDef> {
    BTreeMap::from([
        (
            "claude".to_string(),
            AgentDef {
                command: "claude".into(),
                resume: Some("claude --resume {session_id}".into()),
            },
        ),
        (
            "codex".to_string(),
            AgentDef {
                command: "codex".into(),
                resume: Some("codex resume {session_id}".into()),
            },
        ),
    ])
}

/// The first word of a command line, without any directory part.
#[must_use]
pub fn program_name(command: &str) -> &str {
    let first = command.split_whitespace().next().unwrap_or("");
    first.rsplit('/').next().unwrap_or(first)
}

impl Config {
    /// Parse a config file; user agents are merged over the built-in ones.
    ///
    /// # Errors
    /// Returns the TOML error for invalid syntax or unknown value types.
    pub fn from_toml(text: &str) -> Result<Self, toml::de::Error> {
        let mut cfg: Self = toml::from_str(text)?;
        for (name, def) in builtin_agents() {
            cfg.agents.entry(name).or_insert(def);
        }
        Ok(cfg)
    }

    /// Classify a launch command.
    #[must_use]
    pub fn classify(&self, command: Option<&str>) -> TabKind {
        let Some(command) = command.filter(|c| !c.trim().is_empty()) else {
            return TabKind::Shell;
        };
        let program = program_name(command);
        if let Some((name, _)) = self
            .agents
            .iter()
            .find(|(_, def)| program_name(&def.command) == program)
        {
            return TabKind::Agent(name.clone());
        }
        if self.editors.iter().any(|e| e == program) {
            TabKind::Editor
        } else if self.shells.iter().any(|s| s == program) {
            TabKind::Shell
        } else {
            TabKind::Command
        }
    }

    /// Whether a pane's foreground process is just a shell.
    #[must_use]
    pub fn is_shell(&self, current_command: &str) -> bool {
        let p = current_command.trim_start_matches('-');
        p.is_empty() || self.shells.iter().any(|s| s == p)
    }

    /// Path for a new worktree of `repo` on `branch`.
    #[must_use]
    pub fn worktree_path(&self, repo: &Path, branch: &str) -> PathBuf {
        let parent = repo.parent().unwrap_or(repo);
        let name = repo
            .file_name()
            .map_or_else(|| "repo".to_string(), |n| n.to_string_lossy().into_owned());
        let rendered = self
            .worktree_dir
            .replace("{repo_parent}", &parent.to_string_lossy())
            .replace("{repo}", &name)
            .replace("{branch}", branch);
        PathBuf::from(rendered)
    }

    /// Tabs to open for a new workspace: the repo's own list if it has one.
    #[must_use]
    pub fn tabs_for(&self, repo: Option<&RepoConfig>) -> Vec<TabSpec> {
        repo.and_then(|r| r.tabs.clone())
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| self.tabs.clone())
    }

    /// Choices offered by "new tab": the default tabs, every agent, the first
    /// configured editor, and a shell. All of them open in the workspace's directory.
    #[must_use]
    pub fn new_tab_choices(&self, repo: Option<&RepoConfig>) -> Vec<TabSpec> {
        let mut out: Vec<TabSpec> = Vec::new();
        let mut push = |t: TabSpec| {
            if !out.iter().any(|o| o.name == t.name) {
                out.push(t);
            }
        };
        for t in self.tabs_for(repo) {
            push(t);
        }
        for (name, def) in &self.agents {
            push(TabSpec::new(name, Some(&def.command)));
        }
        if let Some(editor) = self.editors.first() {
            push(TabSpec::new(editor, Some(editor)));
        }
        push(TabSpec::new("shell", None));
        out
    }
}

/// `<repo>/.the-deaner.toml`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct RepoConfig {
    pub tabs: Option<Vec<TabSpec>>,
}

impl RepoConfig {
    /// # Errors
    /// Returns the TOML error for invalid syntax.
    pub fn from_toml(text: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_file_is_default() {
        assert_eq!(Config::from_toml("").unwrap(), Config::default());
    }

    #[test]
    fn user_agents_merge_over_builtins() {
        let cfg = Config::from_toml(
            r#"
sidebar_width = 40
[agents.aider]
command = "aider --no-auto-commits"
[agents.claude]
command = "claude --model opus"
resume = "claude --model opus --resume {session_id}"
"#,
        )
        .unwrap();
        assert_eq!(cfg.sidebar_width, 40);
        assert_eq!(cfg.agents.len(), 3);
        assert_eq!(cfg.agents["claude"].command, "claude --model opus");
        assert!(cfg.agents.contains_key("codex"));
    }

    #[test]
    fn classifies_commands() {
        let cfg = Config::default();
        assert_eq!(
            cfg.classify(Some("claude")),
            TabKind::Agent("claude".into())
        );
        assert_eq!(
            cfg.classify(Some("/usr/local/bin/claude --continue")),
            TabKind::Agent("claude".into())
        );
        assert_eq!(cfg.classify(Some("nvim .")), TabKind::Editor);
        assert_eq!(cfg.classify(Some("npm run dev")), TabKind::Command);
        assert_eq!(cfg.classify(None), TabKind::Shell);
        assert_eq!(cfg.classify(Some("  ")), TabKind::Shell);
        assert!(cfg.is_shell("-zsh"));
        assert!(!cfg.is_shell("nvim"));
    }

    #[test]
    fn worktree_path_template() {
        let cfg = Config::default();
        assert_eq!(
            cfg.worktree_path(Path::new("/home/sd/dev/api"), "feat/x"),
            PathBuf::from("/home/sd/dev/api.worktrees/feat/x")
        );
    }

    #[test]
    fn repo_tabs_override_defaults() {
        let cfg = Config::default();
        let repo = RepoConfig::from_toml(
            r#"
[[tabs]]
name = "claude"
command = "claude"
[[tabs]]
name = "dev"
command = "npm run dev"
"#,
        )
        .unwrap();
        let tabs = cfg.tabs_for(Some(&repo));
        assert_eq!(tabs.len(), 2);
        assert_eq!(tabs[1].command.as_deref(), Some("npm run dev"));
        assert_eq!(cfg.tabs_for(None), cfg.tabs);

        let choices: Vec<_> = cfg
            .new_tab_choices(Some(&repo))
            .into_iter()
            .map(|t| t.name)
            .collect();
        assert_eq!(choices, ["claude", "dev", "codex", "nvim", "shell"]);

        // The default tabs, in order, then anything else that can be opened.
        let names = |tabs: Vec<TabSpec>| tabs.into_iter().map(|t| t.name).collect::<Vec<_>>();
        assert_eq!(names(cfg.tabs_for(None)), ["nvim", "claude", "shell"]);
        assert_eq!(
            names(cfg.new_tab_choices(None)),
            ["nvim", "claude", "shell", "codex"]
        );
    }
}
