# Workflows

## Start of the day

```sh
deaner
```

If nothing is running (after a reboot, or after `deaner shutdown`), the sidebar asks:

> Restore 4 workspace(s) saved 14 h ago?

Press `y`. Every workspace comes back with its tabs, splits and layouts, in the same directories.
Each claude resumes **its own** conversation (`claude --resume <id>`), even when several claudes
share a repo. nvim reopens in the same folder, and dev-server tabs re-run their command.

If something is already running, deaner just reattaches. Nothing restarts.

## Two claudes on one branch (code + docs)

Both work in the same checkout, so you don't need a worktree:

1. Select the repo and press `t`, then pick `claude`. That's a second claude tab in the same session.
2. Tell one to work on code and the other on docs. They edit different files.

The sidebar shows the busier of the two: `!` if either is waiting on you, `⠋` if either is working.

## A security bump in the middle of a feature (two branches at once)

Switching branches under a running agent pulls its files out from under it. Use a worktree instead:

1. Select the repo and press `w`.
2. Type the branch name, e.g. `deps/security-bump`, and press Enter.

deaner runs `git worktree add` (at `../<repo>.worktrees/<branch>` by default), starts a session
with your default tabs, and shows it. The worktree is nested under the repo in the sidebar.

When the bump is merged, select the worktree and press `x`. deaner kills its session and removes the
worktree. It refuses if there are uncommitted changes (use `deaner archive <name> --force` to override).

From the CLI:

```sh
deaner new shop-api deps/security-bump
deaner archive shop-api/deps/security-bump
```

## Agents talking to each other across repos

Every tab has an address, `<session>:<tab>`:

```sh
$ deaner ls
★ shop-api
  main                     live    shop-api:claude (busy)  shop-api:nvim  shop-api:shell
★ shop-web
  main                     live    shop-web:claude (idle)  shop-web:shell
```

From any pane (or from an agent's shell tool):

```sh
deaner send shop-web:claude "The /v2/orders endpoint now returns ISO dates; update the client."
deaner whoami        # -> shop-api:claude, so the other agent knows where to reply
deaner ls --json     # machine-readable: workspaces, tabs, agent states, addresses
```

`send` types the text into the target tab and presses Enter. See [Agents and hooks](agents.md)
for a CLAUDE.md snippet that teaches claude these commands.

## Pinning

Press `p` on a repo to pin it. Pinned repos sit at the top, marked `★`, in **your** order:
`K` moves one up, `J` moves it down. Activity never reorders them.

Unpinned repos come next, sorted by name. With `float_attention = true` (the default), an
unpinned repo with an agent waiting on you floats to the top of the unpinned section.

## Park a workspace (pause it)

Select a running workspace and press `P` (or `deaner park <name>`). deaner saves it and stops its
session, which frees the memory. It stays in the sidebar, dimmed and marked `parked`. Press `Enter`
on it to bring it back exactly as it was.

## End of the day

```sh
deaner shutdown
```

If an agent is still working, deaner lists them and asks:

```text
Agents still active:
  shop-api:claude  busy
[w]ait for busy agents, [i]nterrupt them, [s]ave anyway, [c]ancel?
```

Then it saves a snapshot and stops everything. Power off. Run `deaner` tomorrow.

Claude writes its transcript continuously, so "save anyway" loses at most the turn in progress.

## Crashes, dead batteries, forgetting to shut down

The sidebar autosaves a snapshot every 5 minutes (`autosave_minutes`), but only while something is
running, so an empty post-crash state never overwrites a good snapshot. After an unplanned power-off,
`deaner` offers to restore from the last autosave.

Timestamped snapshots are kept in `~/.local/state/deaner/snapshots/` (the newest 20 by default).
Restore an older one with:

```sh
deaner restore --from ~/.local/state/deaner/snapshots/snapshot-1793000000.toml
```

## Hide the sidebar

`Alt-b` hides the sidebar (the terminal takes the full window) and shows it again. Drag the
border to resize it. The width is remembered across restarts.
