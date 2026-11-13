# Configuration

the-deaner works without any config. To change things, create `~/.config/the-deaner/config.toml`
(or `$XDG_CONFIG_HOME/the-deaner/config.toml`; same path on macOS). `the-deaner config` prints
the paths in use, and `the-deaner config --default` prints the defaults below.

Only set what you want to change. Missing keys keep their defaults.

## `config.toml`

```toml
# Sidebar width (columns) until you drag it; after that your width is remembered.
sidebar_width = 32

# Sidebar icons: "nerd" (needs a Nerd Font, or a terminal with the glyphs built in, like Ghostty)
# or "unicode" (plain symbols that work in any font).
icons = "nerd"

# How the project open in the terminal is marked: "box" (a thin gold box around it, its
# name in the top border) or "block" (dark gold rows with a bar down the left edge).
open_project = "box"

# Snapshot everything this often while something is running. 0 turns autosave off.
autosave_minutes = 5
# Timestamped snapshots to keep in ~/.local/state/the-deaner/snapshots/.
snapshots_keep = 20

# Float active repos with an agent waiting on you to the top of the ACTIVE section.
float_attention = true

# How often to refresh git status for running workspaces (stopped ones: every 30 s).
git_poll_seconds = 3
# How often to ask `gh` about pull requests and CI for running projects; others are asked 5x less
# often. 0 turns both off.
pr_poll_seconds = 60
# How often to ask instead while that matters: while a project's CI is running (up to 20 minutes),
# and for 3 minutes after you push from it. 0 turns this off. Opening a project, or `r`, asks right
# away. GitHub allows 5,000 API calls an hour, shared with everything else using `gh`, so this only
# ever applies to the project(s) where something is about to change.
pr_fast_seconds = 5
# How often to `git fetch` repos with a running session, so the behind count stays current.
# Never prompts, doesn't touch your checkout, and only updates remote-tracking branches.
# 0 turns it off; the minimum otherwise is 30.
fetch_seconds = 60

# Where `w` / `the-deaner new` put worktrees. {repo_parent}, {repo} and {branch} are substituted.
worktree_dir = "{repo_parent}/{repo}.worktrees/{branch}"

# Programs that count as "just a shell". Older snapshots use this list; new ones
# compare against tmux's default-shell instead.
shells = ["zsh", "bash", "fish", "sh", "dash", "nu", "elvish", "xonsh"]

# Editors reopened on restore even if the-deaner did not start them.
editors = ["nvim", "vim", "hx", "helix", "emacs"]

# Frame keys (tmux key names: M- = Alt, C- = Ctrl). They work in the sidebar and the terminal.
[keys]
focus = "M-s"           # move focus between the sidebar and the terminal
toggle_sidebar = "M-e"  # hide / show the sidebar (think "explorer")
cheatsheet = "M-/"      # open the searchable shortcut cheat sheet

# Tabs a new workspace starts with, in order (a repo's .the-deaner.toml overrides these).
[[tabs]]
name = "nvim"
command = "nvim"

[[tabs]]
name = "claude"
command = "claude"

[[tabs]]
name = "shell"          # no command: a plain shell

# Agents. These are merged over the built-in claude and codex definitions.
# `resume` is how a restored tab gets its conversation back; {session_id} comes from the agent's hooks.
[agents.claude]
command = "claude"
resume = "claude --resume {session_id}"

[agents.codex]
command = "codex"
resume = "codex resume {session_id}"
```

### Adding an agent

```toml
[agents.aider]
command = "aider --no-auto-commits"
```

It then appears in the `t` (new tab) picker. A tab whose command starts with `aider` is recognised as
an agent and relaunched on restore. Agents without the-deaner hooks show no busy/waiting badge.

### Different flags for claude

```toml
[agents.claude]
command = "claude --model opus"
resume = "claude --model opus --resume {session_id}"
```

## Per-repo tabs: `.the-deaner.toml`

Put a `.the-deaner.toml` in a repo's root to give it its own starting tabs:

```toml
[[tabs]]
name = "claude"
command = "claude"

[[tabs]]
name = "docs"
command = "claude"

[[tabs]]
name = "dev"
command = "npm run dev"

[[tabs]]
name = "nvim"
command = "nvim"
```

Commands are typed into a shell in the tab, so when one exits you're left at a prompt in the
same place.

## Files the-deaner writes

| Path | What |
|---|---|
| `~/.local/state/the-deaner/state.toml` | Known repos, pin order, collapsed groups, sidebar width |
| `~/.local/state/the-deaner/snapshots/` | `latest.toml` plus timestamped snapshots |
| `~/.local/state/the-deaner/parked/` | One file per parked workspace |
| `~/.local/state/the-deaner/frame.tmux.conf` | Generated config for the frame server |

`THE_DEANER_HOME=/some/dir` moves all of it (config and state) under one directory.

## tmux servers

the-deaner uses two tmux servers, so it never touches your other tmux sessions:

| Socket | What | Attach by hand |
|---|---|---|
| `the-deaner` | Your workspaces; uses your `~/.tmux.conf` | `tmux -L the-deaner attach` |
| `the-deaner-frame` | The sidebar + terminal layout | `the-deaner` |

Override them with `THE_DEANER_SOCKET` and `THE_DEANER_FRAME_SOCKET`.

If you use tmux-resurrect and tmux-continuum, the-deaner turns off continuum's autosave on its own
server (`@continuum-save-interval 0`). the-deaner snapshots that server itself, and continuum would
otherwise write the-deaner's sessions into the resurrect file your normal tmux uses. Don't enable
`@continuum-restore` for the-deaner's server either: the-deaner restores its own sessions.
