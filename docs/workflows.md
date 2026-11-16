# Workflows

## Opening a project

Press `a` in the sidebar (or run `the-deaner browse`). A popup lists the folders where your most recent
project lives:

| Key | Action |
|---|---|
| `↑` `↓` | Move |
| type | Narrow the list, or type a path (below) |
| `Enter` | On a git repo: add it, start it and show it. On a plain folder: go into it. |
| `→`, `Tab` | Go into the highlighted folder |
| `←` | Up one folder (with nothing typed; otherwise it moves through what you typed) |
| `Ctrl-f` | Add the highlighted folder to your favorites, or remove it |
| `Esc` | Close |

Git repos have a branch icon, plain folders a folder icon. To use a plain folder as a project, go into
it and pick **Use this folder** at the top. Above that, **↑ Up to …** goes up a folder (it names where
to), the same as `←`. Folders starting with a dot are hidden until you type a `.`.

**Favorites** are folders you jump to, listed at the top wherever you are. `Enter` on one takes you there.
The browser starts in your first favorite, so make that where you usually begin. To add the folder you're
in, go to **Use this folder** and press `Ctrl-f`. They're kept in order in `state.toml`, which is how to
reorder them for now.

To go anywhere on the system, press `←` until you reach the folder you want, or type a path the way
you would in a shell: `/tmp/`, `~/dev/`, `../`, `go/` (from where you were). The path stays in the box,
the list shows the folder up to its last `/`, and whatever comes after narrows that list. So
`~/dev/ru` lists `~/dev` narrowed to `rust`; `Tab` completes it to `~/dev/rust/`, and `Enter` opens the
highlighted repo. Backspacing over a `/` goes back out of that folder. A path to a folder that doesn't
exist lists nothing until you fix it.

To take a project out of the sidebar, select it and press `x`. It asks first, saying which tmux
sessions will stop and warning you if an agent in it is still working. `y` stops its sessions (its
checkout and any worktrees), drops a sleeping copy, and removes it. Nothing on disk is touched, and
`the-deaner add` or `a` brings it back. `the-deaner remove <name>` does the same from a shell.

From the command line: `the-deaner open <path or name>` adds a folder if needed and opens it, and
`the-deaner add <path>` adds without opening.

## A throwaway clone

To have claude read a repo you don't plan to keep, clone it anywhere and open it:

```sh
git clone https://github.com/someone/thing /tmp/thing
the-deaner open /tmp/thing
```

When you're done, close its tabs and delete the folder. It disappears from the sidebar by itself. (A
session still running in a deleted folder stays listed until you close it.)

## Start of the day

```sh
the-deaner
```

If nothing is running (after a reboot, or after `the-deaner shutdown`), the sidebar asks:

> Restore 4 workspace(s) saved 14 h ago?

Press `y`. Every workspace comes back with its tabs, splits and layouts, in the same directories.
Each claude resumes **its own** conversation (`claude --resume <id>`), even when several claudes
share a repo. A claude you never sent a message to has no conversation to resume, so it starts fresh. nvim reopens in the same folder, and dev-server tabs re-run their command.
The terminal shows the workspace you were last on.

If something is already running, the-deaner just reattaches. Nothing restarts.

## Two claudes on one branch (code + docs)

Both work in the same checkout, so you don't need a worktree:

1. Select the repo and press `t`, then pick `claude`. That's a second claude tab in the same session.
2. Tell one to work on code and the other on docs. They edit different files.

The sidebar shows the more urgent of the two: waiting on you if either is, otherwise working if either is.

## What worktrees are for

A worktree is a git branch **plus its own folder**. A normal repo has one folder, so only one branch can be
checked out at a time. A worktree adds a second folder on another branch. Both folders share one git history,
so a commit in either is visible to both, but their files on disk are separate.

Worktrees are a git feature, and they're useful with or without agents.

**Example:** you're halfway through a new endpoint in `shop-api`, with 12 files edited and nothing committed.
Then a security fix has to ship today.

| Step | With `git stash` | With a worktree |
|---|---|---|
| Stop what you're doing | Yes | No, it keeps running |
| Stash your work | Yes | No, it stays where it is |
| Create a branch | Yes | Yes, `w` does it |
| Do the work, commit, push | Yes | Yes, in the new folder |
| Pop and go back | Yes | No, switch rows in the sidebar |
| Clean up | No | Yes, `x` removes the folder |

```text
shop-api
   feat/new-endpoint  ✎ 12  ⠋     ← still working
   deps/security-fix  #260 ✓       ← the fix, done separately
```

This matters most with agents, because a stash can't pause a claude that's in the middle of editing those
files, and a worktree leaves it running.

**When you don't need one:** working on one thing at a time, or two claudes on the same branch editing
different files. Plain branches are fine for those.

**Worth knowing:**
- Files git ignores don't come along: `node_modules`, build output, virtualenvs and `.env` files are per folder.
  A new worktree may need its own install or copy of `.env`.
- Two worktrees can collide if they run at once and use a fixed dev-server port, database name or Docker
  container name.
- A branch can only be checked out in one folder at a time.

**What removing one loses.** A worktree's folder is a complete second checkout: every tracked file of the
repo (code, docs, `Cargo.toml`, …) is in it, which is why it looks like "a lot of stuff". Removing it with
`x` deletes that folder, but nearly everything in it lives on in git:

| In the worktree's folder | Lost when it's removed? |
|---|---|
| Tracked files you haven't changed | No: the same files are in git (and in your main checkout) |
| Commits on its branch | No: the branch keeps them; it's only deleted if you say so, and only if merged |
| Uncommitted edits and untracked files | **Yes**: `x` names them and asks before they go |
| Ignored files (`node_modules`, build output, `.env`) | Yes, without asking: they were only ever in that folder |

So before removing, commit or push what you want to keep, and `x` will tell you about anything you forgot.

## A security bump in the middle of a feature (two branches at once)

Switching branches under a running agent pulls its files out from under it. Use a worktree instead:

1. Select the repo and press `w`.
2. Type the branch name, e.g. `deps/security-bump`, and press Enter.

the-deaner runs `git worktree add` (at `../<repo>.worktrees/<branch>` by default), starts a session
with your default tabs, and shows it. The worktree is nested under the repo in the sidebar.

When the bump is merged, select the worktree and press `x`, then `y`. the-deaner stops its tmux session
and deletes the worktree's folder (`git worktree remove`). Then it asks:

```text
Removed worktree shop-api/deps/security-bump. Delete branch deps/security-bump too?
y yes · n no  (press one)
```

`y` deletes the branch the safe way (`git branch -d`): if it has commits that aren't merged anywhere, git
keeps it and the sidebar says so, so you can't lose work this way. `n` keeps it. Enter does nothing here;
press `y` or `n`.

If the worktree has uncommitted changes, `x` asks first, naming them (`… has 1 uncommitted change:
scratch.txt. Remove it anyway and lose it?`). `y` removes it and the changes are gone; `n` leaves it as it
was.

From the CLI:

```sh
the-deaner new shop-api deps/security-bump
the-deaner remove shop-api/deps/security-bump                   # keeps the branch
the-deaner remove shop-api/deps/security-bump --delete-branch   # and deletes it if merged
```

`--force` removes it despite uncommitted changes.

## Reports: what every claude last said

With several claudes going, press `o` (or run `the-deaner reports`) to read all their latest answers
in one place, without switching projects:

```text
 Reports                                              live
 ! dmi:claude        2m ago │ ## Runner docs updated
 ✻ tally:claude      9m ago │
 ⠋ kexp-api:claude   working│ I rewrote docs/runners.md and…
 ✻ the-deaner:claude 1h ago │ Tests pass. One question for you: …
```

- On the left, every claude, newest answer first, with the sidebar's icons: `!` waiting on you, `✻`
  done, `⠋` working.
- On the right, the selected claude's latest answer: the last thing it said to finish a turn, drawn
  like the guide (headings, code, lists). One that's working again shows its previous answer, marked
  as such.
- It's live: while it's open, a claude that finishes moves to the top with its new answer.
- `j`/`k` choose, `Ctrl-d`/`Ctrl-u` scroll, `Enter` goes to that claude's tab, `Esc` closes.
- Reading an answer here counts as seeing it: its orange "needs you" mark in the sidebar clears.

It only reads the transcripts Claude Code already writes; nothing is sent to the claudes.

## Agents talking to each other across repos

Every tab has an address, `<session>:<tab>`:

```sh
$ the-deaner ls
★ shop-api
  main                     live    shop-api:claude (busy)  shop-api:nvim  shop-api:shell
★ shop-web
  main                     live    shop-web:claude (idle)  shop-web:shell
```

From any pane (or from an agent's shell tool):

```sh
the-deaner send shop-web:claude "The /v2/orders endpoint now returns ISO dates; update the client."
the-deaner whoami        # -> shop-api:claude, so the other agent knows where to reply
the-deaner ls --json     # machine-readable: workspaces, tabs, agent states, addresses
```

`send` types the text into the target tab and presses Enter. See [Agents and hooks](agents.md)
for a CLAUDE.md snippet that teaches claude these commands.

## Sidebar sections and pinning

The sidebar has up to three sections. A section's heading only appears when it has repos.

| Section | What's in it | Order |
|---|---|---|
| `PINNED` | Repos you pinned, running or not | **Your** order; activity never moves them |
| `ACTIVE` | Unpinned repos with an agent running (in the main checkout or any worktree) | By name |
| `ASLEEP` | Repos put to sleep with `z` (saved, no tmux session) and nothing else of theirs running. Only shown when there's one | By name |
| `INACTIVE` | Repos you've added with no agent running: ones with only an editor or shell open, and ones not started | By name |

Press `p` on a repo to pin it, `K` to move it up, `J` to move it down.

Starting an agent in an unpinned repo moves it from `INACTIVE` to `ACTIVE`, and the cursor stays on
it. Quitting its last agent moves it back, even if an editor or shell is still open there. With `float_attention = true` (the default), an active
repo with an agent waiting on you floats to the top of `ACTIVE`.

## Forgot a key?

Press `Alt-/` anywhere, or `?` in the sidebar. A popup lists every shortcut, and you can type what
you're trying to do: "hide the sidebar", "new tab", "how do I navigate the tabs". The terminal section
is read from your tmux config, so it shows your real keys, including anything you've rebound.

| Common question | Key (`prefix` is your tmux prefix) |
|---|---|
| Hide the sidebar | `Alt-e` |
| Move between the sidebar and the terminal | `Alt-s` |
| Go to the next agent that needs you | `n` in the sidebar |
| Leave, with everything still running | `q` in the sidebar |
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
| `+1 -0` | Commits ahead of and behind the upstream. Hidden when both are 0. the-deaner fetches running repos in the background every minute (`fetch_seconds`), so this tracks GitHub to within about a minute. Repos without a running session aren't fetched. |
| `#259` | The branch's open PR, or its last merged one. |
| Check | CI: green check passing, yellow clock running, red X failing. It comes from an open PR, otherwise from the branch's latest commit on GitHub, so `main` shows one with no PR at all. Repos without CI show nothing. Running projects update every minute (`pr_poll_seconds`), others every 5. While a project's CI is running, and for 3 minutes after you push from it, it updates every 5 seconds (`pr_fast_seconds`); opening a project or pressing `r` updates right away. |
| Agent | Spinner: working. Bell: waiting on you. `✻`: idle. `◐`: compacting. Pause: asleep (`z`). |

The project shown on the right sits in a thin gold box with its name in the top border
(`open_project = "block"` gives a dark gold block with a bar down its edge instead). The cursor is a navy
row and moves between workspaces only; a project's name row is a stop only when the project is collapsed.

Under each workspace, a dim line per agent says what it's working on: the name you gave it with
`/rename`, else Claude's own title for the conversation, else your last prompt (`› …`) or the last message
another claude sent it (`⇄ name: …`). A project running two or more agents counts them on its name row,
or in its box's top border: `!1 ⠋2 ✻1` is one waiting on you, two working, one idle.

**Orange means it needs you.** When an agent starts waiting on you, or finishes while you're looking at
something else, its row gets an orange background and keeps it until you open that workspace, so several
at once read like a to-do list. The bell stays until you actually answer. The repo name and a dot on the
section heading turn orange too, and the footer's `n` turns orange: press it to go to the next one.

`Space` collapses a repo down to one line, and an arrow marks the ones that are collapsed. In INACTIVE, a
project with only its main checkout takes a single row (`› name  branch`). When the list is longer than the
sidebar, `▲ 3 more` / `▼ 5 more` say how many projects are out of view; click one to scroll, or use
`Ctrl-d` / `Ctrl-u`.

`v` opens a popup with the end of an agent's output (the one waiting on you first), without leaving what
you're looking at; scroll up for more, `q` closes it. From a shell: `the-deaner peek shop-api:claude`.

Press `Alt-/` and scroll down, or search for "icons", for the same legend. Icons use a Nerd Font; set `icons = "unicode"` in
`config.toml` for plain symbols. The tab bar and terminal colors come from your tmux config and terminal.

## Choosing which tabs open

A new workspace starts the tabs in `[[tabs]]`, in order: by default `nvim`, `claude` and a `shell`, in
the project root (or the worktree root for a worktree). You land on the first one. When you want more,
press `t` in the sidebar and pick one: another claude, a shell. It opens in the same directory.

To start with something else, set your own list. This one starts lighter:

```toml
# ~/.config/the-deaner/config.toml
[[tabs]]
name = "claude"
command = "claude"

[[tabs]]
name = "shell"
```

This applies to projects as they start. A project restored from a snapshot keeps the tabs it had.

A repo's own `.the-deaner.toml` overrides this. See [Configuration](configuration.md).

## Put a workspace to sleep

Select a running workspace and press `z` or `P` (or `the-deaner sleep <name>`). the-deaner saves it and stops
its tmux session, which frees the memory (any agent in it stops too). It moves to the `‖ ASLEEP` section,
between ACTIVE and INACTIVE since you're likely to come back to it (a pinned project stays in PINNED,
with the `‖`). The section only appears when something is asleep. Press `Enter` on it (or `the-deaner wake <name>`) to wake it exactly as it was, each claude resuming its own
conversation. It's `the-deaner shutdown` for one project: for something you're done with for now, but
not finished with.

## End of the day

```sh
the-deaner shutdown
```

If an agent is still working, the-deaner lists them and asks:

```text
Agents still active:
  shop-api:claude  busy
[w]ait for busy agents, [i]nterrupt them, [s]ave anyway, [c]ancel?
```

Then it saves a snapshot and stops everything. Power off. Run `the-deaner` tomorrow.

Claude writes its transcript continuously, so "save anyway" loses at most the turn in progress.

## A new version is out

When the sidebar shows `↑ 0.2.0 upgrade available` at the bottom, run `the-deaner upgrade` in any
tab. It installs the release (with pacman or Homebrew), saves everything, restarts in the same
window, and offers to restore it all, with each claude picking up its own conversation, the same as
the start of the day. If an agent is still working it asks first, as `shutdown` does. Details in
[getting started](getting-started.md#updating).

## Snapshots: how everything comes back

A **snapshot** is what a restore rebuilds from: every running workspace with its tabs, splits and
layouts, each pane's folder and what was running in it, and each claude's conversation, so it can
resume. You don't need to take them yourself; the-deaner saves one:

| When | Which |
|---|---|
| Every 5 minutes while anything is running (`autosave_minutes`) | Everything |
| `the-deaner shutdown`, `restart` and `upgrade`, just before stopping | Everything |
| Putting a workspace to sleep (`z`) | That workspace (it's what waking it rebuilds from) |
| `the-deaner save`, whenever you like (say, before something risky) | Everything |

Autosave only runs while something is running, so an empty post-crash state never overwrites a good
snapshot.

## Crashes, dead batteries, forgetting to shut down

After an unplanned power-off, `the-deaner` offers to restore from the last autosave, at most five
minutes old.

Timestamped snapshots are kept in `~/.local/state/the-deaner/snapshots/` (the newest 20 by default,
`snapshots_keep`), next to `latest.toml`. Restore an older one with:

```sh
the-deaner restore --from ~/.local/state/the-deaner/snapshots/snapshot-1793000000.toml
```

## Hide the sidebar

`Alt-e` (think "explorer") hides the sidebar (the terminal takes the full window) and shows it again. Drag the
border to resize it. The width is remembered across restarts.

## Working from another machine

the-deaner runs on its own tmux servers, and a terminal window is only a view onto them. So you can
leave your desk, SSH in from a laptop and pick up where you were:

```sh
ssh you@your-box
the-deaner
```

`the-deaner` takes over: any other window showing it (say, the one still open at your desk) is
detached, so the screen you're using sets the size and the two don't fight over it. That window
just goes back to its shell, and running `the-deaner` there takes it back. Nothing stops either way.
To keep both on screen (pairing, or two monitors), open with `the-deaner --shared`: the other window
stays attached, and the frame takes the size of whichever one you used last.

Detach with `q` in the sidebar (or close the window) and everything keeps running. After
`the-deaner restart` or an upgrade, every window that was showing it comes back.
