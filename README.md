# deaner

A tmux-backed command center for running coding agents side by side. Linux first, macOS too.

```text
┌ sidebar ──────────────────┬ terminal ────────────────────────────────┐
│ ★ PINNED                  │                                          │
│ ▾ deaner                  │  your own tmux: claude, nvim, shells,    │
│   ⎇ main                ✻ │  one session per workspace,              │
│ ▾ shop-api                │  one window per tab                      │
│   ⎇ main      ✎ 3 +1 -0 ⠋ │                                          │
│   ⇄ sec-bump        #42 ! │                                          │
│ ○ INACTIVE                │                                          │
│ ▾ dotfiles                │                                          │
│   ⎇ main                ‖ │                                          │
│ ? help            ! 1 ⠋ 1 │                                          │
└───────────────────────────┴──────────────────────────────────────────┘
```

- **Always-visible sidebar** of repos, worktrees and agents. Resize it by dragging, hide it with `Alt-e`.
- **Repo first, worktree on demand.** `w` creates a worktree and session for a parallel branch.
- **Any agent.** Tabs are tmux windows: claude, codex, aider, nvim, dev servers.
- **Agent status.** Working, waiting on you, or idle, via Claude Code hooks, with toasts.
- **Git and PR status.** Branch, changed files, `+ahead -behind`, PR number colored by CI (via `gh`).
- **Pinned, active, inactive sections.** Pinned repos keep your order; activity never moves them.
- **Save, park, shut down, restore.** `deaner shutdown` at night; `deaner` in the morning brings
  everything back and each claude resumes its own conversation.
- **Agents can message each other.** `deaner send shop-web:claude "…"`.

## Quick start

```sh
# needs tmux >= 3.3 and git; gh optional
cargo install --locked --git https://github.com/sean-d/deaner deaner

deaner add ~/dev/shop-api ~/dev/deaner
deaner install-hooks        # agent status for Claude Code
deaner                      # open the frame
```

In the sidebar: `j`/`k` to move, `Enter` to open, `t` new tab, `w` new worktree, `p` pin.
`Alt-s` jumps between the sidebar and the terminal. `Alt-/` opens a searchable cheat sheet of every shortcut.

## Documentation

The full docs live in [`docs/`](docs/src/SUMMARY.md) (published with GitHub Pages):
[getting started](docs/src/getting-started.md), [workflows](docs/src/workflows.md),
[keybindings](docs/src/keybindings.md), [configuration](docs/src/configuration.md),
[agents and hooks](docs/src/agents.md), [CLI](docs/src/cli.md),
[architecture](docs/src/architecture.md), [development](docs/src/development.md).

## Development

```sh
cargo nextest run --workspace --all-features   # unit, tmux/git integration, UI snapshots
bacon                                          # clippy on save
```

See [development](docs/src/development.md) for the test layout, generated docs, and build-speed setup.

## License

MIT
