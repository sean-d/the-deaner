# the-deaner

> *Just give'r.*
> Named for Dean "Deaner" Murdoch of [FUBAR](https://en.wikipedia.org/wiki/FUBAR_(film)) (2002).
> It keeps your agents going, so you can keep going too.

A tmux-backed command center for running coding agents side by side. Linux first, macOS too.

```text
┌ sidebar ──────────────────┬ terminal ────────────────────────────────┐
│ ★ PINNED                  │                                          │
│ the-deaner                    │  your own tmux: claude, nvim, shells,    │
│   ⎇ main                ✻ │  one session per workspace,              │
│ shop-api                  │  one window per tab                      │
│   ⎇ main      ✎ 3 +1 -0 ⠋ │                                          │
│   ⇄ sec-bump      #42 ✓ ! │                                          │
│ ○ INACTIVE                │                                          │
│ dotfiles                  │                                          │
│   ⇉ staging        #380 ✓ │                                          │
│ ? help            ! 1 ⠋ 1 │                                          │
└───────────────────────────┴──────────────────────────────────────────┘
```

- **Always-visible sidebar** of repos, worktrees and agents. Resize it by dragging, hide it with `Alt-e`.
- **Repo first, worktree on demand.** `w` creates a worktree and session for a parallel branch.
- **Any agent.** Tabs are tmux windows: claude, codex, aider, nvim, dev servers.
- **Agent status.** Working, waiting on you, or idle, via Claude Code hooks, with toasts.
- **Git and PR status.** Branch, changed files, `+ahead -behind`, PR number and a CI check (via `gh`).
- **Pinned, active, inactive sections.** Pinned repos keep your order; activity never moves them.
- **Save, park, shut down, restore.** `the-deaner shutdown` at night; `the-deaner` in the morning brings
  everything back and each claude resumes its own conversation.
- **Agents can message each other.** `the-deaner send shop-web:claude "…"`.

## Quick start

```sh
# needs tmux >= 3.3 and git; gh optional
cargo install --locked --git https://github.com/sean-d/the-deaner the-deaner

the-deaner add ~/dev/shop-api ~/dev/the-deaner
the-deaner install-hooks        # agent status for Claude Code
the-deaner                      # open the frame
```

In the sidebar: `j`/`k` to move, `Enter` to open, `t` new tab, `w` new worktree, `p` pin.
`Alt-s` jumps between the sidebar and the terminal. `Alt-/` opens a searchable cheat sheet of every shortcut.

## Documentation

The full docs live in [`docs/`](docs/README.md):
[getting started](docs/getting-started.md), [workflows](docs/workflows.md),
[keybindings](docs/keybindings.md), [configuration](docs/configuration.md),
[agents and hooks](docs/agents.md), [CLI](docs/cli.md),
[architecture](docs/architecture.md), [development](docs/development.md).

## Development

```sh
cargo nextest run --workspace --all-features   # unit, tmux/git integration, UI snapshots
bacon                                          # clippy on save
```

See [development](docs/development.md) for the test layout, generated docs, and build-speed setup.

## License

MIT
