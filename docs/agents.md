# Agents and hooks

Any command-line agent runs in the-deaner: a tab is just a tmux window. Hooks add two things for
agents that support them:

1. **Live status:** busy, waiting on you, compacting, idle.
2. **Conversation ids:** so a restore resumes each agent's own conversation.

## Claude Code

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
