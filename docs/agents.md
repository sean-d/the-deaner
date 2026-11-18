# Agents and hooks

Any command-line agent runs in the-deaner: a tab is just a tmux window. Hooks add two things for
agents that support them:

1. **Live status:** busy, waiting on you, compacting, idle.
2. **Conversation ids:** so a restore resumes each agent's own conversation.

## Claude Code

The first time the sidebar starts, if Claude Code is installed but the hooks aren't, it asks whether
to add them. `n` means it won't ask again. To add them yourself:

```sh
the-deaner install-hooks
```

This merges the-deaner's hooks into `~/.claude/settings.json` (or `$CLAUDE_CONFIG_DIR/settings.json`).
Your other settings and hooks are kept, and a backup is written next to the file
(`settings.json.the-deaner-backup`). Running it again is safe: the-deaner replaces only its own entries.

| Claude hook | the-deaner event | Badge |
|---|---|---|
| `SessionStart` | `session-start` (records the conversation id) | idle |
| `UserPromptSubmit` | `busy` | working |
| `PreToolUse` | `pre-tool`: `AskUserQuestion` / `ExitPlanMode` mean waiting, other tools mean busy | waiting or working |
| `PostToolUse` | `busy` | working |
| `Notification` | `notification`: permission prompts mean waiting; the "waiting for your input" idle nudge is ignored | waiting |
| `PreCompact` | `compacting` | compacting |
| `Stop` | `stop` | idle |
| `SessionEnd` | `session-end` (clears the pane) | none |

The hook command is `the-deaner hook claude <event>`. It reads Claude's JSON payload from stdin, writes
the state into a tmux pane option (`@the_deaner_state`) on its own pane, and exits. No daemon or socket
is involved, it is a few milliseconds per call, and it always exits successfully so it can never
block Claude. Outside tmux it does nothing.

If `the-deaner` is not on the `PATH` Claude sees, point the hooks at the binary:

```sh
the-deaner install-hooks --command ~/.cargo/bin/the-deaner
```

Remove them with:

```sh
the-deaner install-hooks --uninstall
```

## Without the hooks

The sidebar still sees what each claude is doing without the hooks, from Claude Code's own session
files: every running claude keeps one in `~/.claude/sessions/` (or `$CLAUDE_CONFIG_DIR/sessions/`)
with its status and the tmux pane it runs in. That gives you working, waiting and idle, the "needs
you" mark, titles, Reports, and resuming the conversation after a restore. What it doesn't give you
is compacting, which shows as working.

The hooks still come first. A file is used for a pane the hooks haven't said anything about, or
when it changed after the hooks last reported, which is what happens when you remove the hooks while
claude is running: its tab doesn't stay stuck on the hooks' last word. Files stay behind after a claude exits and tmux reuses pane ids, so a file only counts while its claude
is still running on that pane's terminal. The file format isn't documented by Anthropic; if a Claude
Code update changes it, those panes simply show no status until the-deaner catches up, and the hooks
keep working.

## Other agents

Agents without the-deaner hooks still run, show up in `the-deaner ls`, and come back on restore (started
fresh, or via their `resume` template if they report a session id). See
[Configuration](configuration.md#adding-an-agent).

To wire up any agent that can run a shell command on its events, call:

```sh
the-deaner hook <agent-name> <session-start|busy|pre-tool|notification|compacting|stop|session-end>
```

with an optional JSON payload on stdin (`{"session_id": "..."}`).

## Teaching agents to message each other

Add this to `~/.claude/CLAUDE.md` (or a repo's `CLAUDE.md`):

```markdown
## Talking to other agents (the-deaner)

You run inside the-deaner, next to other coding agents in other repos.

- `the-deaner whoami` prints your own address (for example `shop-api:claude`).
- `the-deaner ls` lists every workspace and tab with its address and agent state.
- `the-deaner send <address> "<message>"` types a message into another agent's tab and submits it.
  Say who you are (your `the-deaner whoami` address) so the other agent can reply the same way.
- Only message another agent when the user asked you to coordinate with it.
```
