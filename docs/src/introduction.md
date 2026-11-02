# deaner

deaner is a command center for running coding agents side by side in the terminal.
It is built on tmux and works on Linux (the main target) and macOS.

```text
┌ sidebar ──────────────────┬ terminal ────────────────────────────────┐
│ ★ PINNED                  │                                          │
│ ▾ deaner                  │  claude, nvim, shells: your own tmux,    │
│   ⎇ main                ✻ │  your own config, one session per        │
│ ▾ shop-api                │  workspace, one tmux window per tab      │
│   ⎇ main      ✎ 3 +1 -0 ⠋ │                                          │
│   ⇄ sec-bump        #42 ! │                                          │
│ ○ INACTIVE                │                                          │
│ ▾ dotfiles                │                                          │
│   ⎇ main                ‖ │                                          │
│ ? help            ! 1 ⠋ 1 │                                          │
└───────────────────────────┴──────────────────────────────────────────┘
```

- **One sidebar for everything.** Every repo, every worktree, every agent, always visible.
  Resize it by dragging the border, or hide it with a key.
- **Repo first, worktree on demand.** Each repo gets a tmux session in its normal checkout. When you need
  to work on a second branch in parallel (say, a security bump in the middle of a feature),
  press `w` and get a worktree with its own session, nested under the repo.
- **Any agent, any tool.** Tabs are tmux windows. Start claude, codex, aider, nvim, or a dev server.
- **Agent status at a glance.** With hooks installed, each claude reports whether it is working,
  waiting on you, or idle. You get a toast when an agent needs you.
- **Git and PR status.** Changed files, ahead/behind, and the PR's CI state, live.
- **Pinned, active, inactive.** Pinned repos keep your order no matter how active they are. The rest
  are split into what's running and what isn't.
- **Done for the day.** `deaner shutdown` saves everything and stops it. Tomorrow, `deaner` offers to
  restore it all, and each claude resumes its own conversation.
- **Agents can talk to each other.** `deaner ls` and `deaner send` let an agent in one repo message an
  agent in another.

Start with [Getting started](getting-started.md).
