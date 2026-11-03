# Workflows

## Opening a project

Press `a` in the sidebar (or run `deaner browse`). A popup lists the folders where your most recent
project lives:

| Key | Action |
|---|---|
| `↑` `↓` | Move |
| type | Narrow the list |
| `Enter` | On a git repo: add it, start it and show it. On a plain folder: go into it. |
| `→` | Go into the highlighted folder |
| `←` | Up one folder |
| `Esc` | Close |

Git repos have a branch icon, plain folders a folder icon. To use a plain folder as a project, go into
it and pick **Use this folder** at the top. Folders starting with a dot are hidden until you type a `.`.

To go anywhere on the system, press `←` until you reach the folder you want, or type a path the way
you would in a shell: `/tmp/`, `~/`, `../`, `go/`. Anything ending in `/` that exists is entered.

From the command line: `deaner open <path or name>` adds a folder if needed and opens it, and
`deaner add <path>` adds without opening.

## A throwaway clone

To have claude read a repo you don't plan to keep, clone it anywhere and open it:

```sh
git clone https://github.com/someone/thing /tmp/thing
deaner open /tmp/thing
```

When you're done, close its tabs and delete the folder. It disappears from the sidebar by itself. (A
session still running in a deleted folder stays listed until you close it.)

## Start of the day

```sh
deaner
```

If nothing is running (after a reboot, or after `deaner shutdown`), the sidebar asks:

> Restore 4 workspace(s) saved 14 h ago?

Press `y`. Every workspace comes back with its tabs, splits and layouts, in the same directories.
Each claude resumes **its own** conversation (`claude --resume <id>`), even when several claudes
share a repo. A claude you never sent a message to has no conversation to resume, so it starts fresh. nvim reopens in the same folder, and dev-server tabs re-run their command.

If something is already running, deaner just reattaches. Nothing restarts.

## Two claudes on one branch (code + docs)

Both work in the same checkout, so you don't need a worktree:

1. Select the repo and press `t`, then pick `claude`. That's a second claude tab in the same session.
2. Tell one to work on code and the other on docs. They edit different files.

The sidebar shows the more urgent of the two: waiting on you if either is, otherwise working if either is.

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

## Sidebar sections and pinning

The sidebar has up to three sections. A section's heading only appears when it has repos.

| Section | What's in it | Order |
|---|---|---|
| `PINNED` | Repos you pinned, running or not | **Your** order; activity never moves them |
| `ACTIVE` | Unpinned repos with a running session (the main checkout or any worktree) | By name |
| `INACTIVE` | Repos you've added that aren't running, including parked ones | By name |

Press `p` on a repo to pin it, `K` to move it up, `J` to move it down.

Starting an unpinned repo moves it from `INACTIVE` to `ACTIVE`, and the cursor stays on it. Parking
or stopping its last session moves it back. With `float_attention = true` (the default), an active
repo with an agent waiting on you floats to the top of `ACTIVE`.

## Forgot a key?

Press `Alt-/` anywhere, or `?` in the sidebar. A popup lists every shortcut, and you can type what
you're trying to do: "hide the sidebar", "new tab", "how do I navigate the tabs". The terminal section
is read from your tmux config, so it shows your real keys, including anything you've rebound.

| Common question | Key (`prefix` is your tmux prefix) |
|---|---|
| Hide the sidebar | `Alt-e` |
| Move between the sidebar and the terminal | `Alt-s` |
| New tab in this project | `t` in the sidebar, or `prefix c` in the terminal |
| Next / previous tab | `prefix n` / `prefix p` |
| Go to a tab by number | `prefix 1`-`9` |

## Worktrees in the sidebar

The sidebar has one row per checkout folder, not per branch, so a repo with 200 local branches and one
folder has one row. Worktrees you make with `w` get a row until you remove them with `x`.

Folders under `.claude/worktrees/` aren't listed. Claude Code creates those for sub-agents and leaves them
behind once they hold commits. They aren't projects you opened, and a sub-agent's progress shows in the tab
that started it. A session running in one does get listed. Clean up leftovers with
`git worktree list` and `git worktree remove <path>`.

## Reading a sidebar row

```text
  <git icon> branch      <changed> +ahead -behind #PR <agent>
```

| Part | Meaning |
|---|---|
| Git icon | Branch, or a pull request icon once the branch has had a PR: green open, dim draft, purple merged. Dim when not running. A detached checkout shows a commit icon and short commit id; a plain folder shows a folder icon. |
| Pencil and number | Changed files |
| `+1 -0` | Commits ahead of and behind the upstream. Hidden when both are 0. deaner fetches running repos in the background every minute (`fetch_seconds`), so this tracks GitHub to within about a minute. Repos without a running session aren't fetched. |
| `#259` | The branch's open PR, or its last merged one. |
| Check | CI: green check passing, yellow clock running, red X failing. It comes from an open PR, otherwise from the branch's latest commit on GitHub, so `main` shows one with no PR at all. Repos without CI show nothing. Running projects update every minute (`pr_poll_seconds`), others every 5. |
| Agent | Spinner: working. Bell: waiting on you. `✻`: idle. `◐`: compacting. Pause: parked. |

The project shown on the right has a solid highlight. **A blue name means it needs you:** an agent is
waiting, or finished while you were looking at something else. It clears when you open the project. The
section heading gets a blue dot too, so you can spot it with a repo scrolled out of view.

`Space` collapses a repo down to one line, and an arrow marks the ones that are collapsed.

Press `Alt-/` and scroll down, or search for "icons", for the same legend. Icons use a Nerd Font; set `icons = "unicode"` in
`config.toml` for plain symbols. The tab bar and terminal colors come from your tmux config and terminal.

## Choosing which tabs open

A new workspace starts the tabs in `[[tabs]]`: by default `claude` and a `shell`, in the project
root (or the worktree root for a worktree). When you want more, press `t` in the sidebar and pick
one: another claude, nvim, a shell. It opens in the same directory.

To always start with something else, set your own list:

```toml
# ~/.config/deaner/config.toml
[[tabs]]
name = "claude"
command = "claude"

[[tabs]]
name = "nvim"
command = "nvim"

[[tabs]]
name = "shell"
```

A repo's own `.deaner.toml` overrides this. See [Configuration](configuration.md).

## Park a workspace (pause it)

Select a running workspace and press `P` (or `deaner park <name>`). deaner saves it and stops its
session, which frees the memory. It stays in the sidebar, dimmed, with a pause icon. Press `Enter`
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

`Alt-e` (think "explorer") hides the sidebar (the terminal takes the full window) and shows it again. Drag the
border to resize it. The width is remembered across restarts.
