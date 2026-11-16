# What's new

What changed in each version of the-deaner, newest first. The same notes are built into
the-deaner: they open by themselves after an upgrade, they're the first page of the guide (`Alt-/`,
then `Tab`), and `the-deaner changelog` shows them from a shell. Search them with `/`.

## 0.1.15 · 2026-11-15

- **What's new.** These notes: they open by themselves the first time the-deaner starts on a new
  version, with what changed since the version you had at the top and everything else below. They're
  also the first page of the guide, and `the-deaner changelog` shows them from a shell.
  `show_changelog = false` in config.toml turns the popup off (the "Updated to" toast stays).

## 0.1.14 · 2026-11-15

- **Sleep instead of park.** `z` (or `P`) puts a workspace to sleep: it's saved and its tmux session
  stops. `Enter` or `the-deaner wake <name>` brings it back, each claude resuming its conversation.
  `the-deaner sleep <name>` from a shell. The footer reads `a add | x remove | z sleep`.
- **An ASLEEP section** between ACTIVE and INACTIVE, shown only when something is asleep. Pinned
  projects stay in PINNED. `the-deaner ls` says asleep.
- **New workspaces open on the claude tab** (`focus_tab = "claude"` in config.toml). Woken and
  restored ones come back on the tab they were saved on.
- The old command names `park`, `archive` and `forget` are gone.

## 0.1.13 · 2026-11-14

- **A built-in user guide.** The docs, readable and searchable in the terminal: `Tab` in the cheat
  sheet (`Alt-/`, or `?` in the sidebar), or `the-deaner guide [page]`. Headings, code, tables
  wrapped to fit, and mermaid diagrams drawn as text. Works over SSH and always matches the version
  you're running.
- The cheat sheet says "User Guide: Tab while viewing this help" under its search box.

## 0.1.12 · 2026-11-14

- **Removing a worktree says "remove"**, not "archive". If it has uncommitted changes, `x` names them
  and asks before they're lost; afterwards it asks whether to delete the branch too (only if it's
  merged). Both questions take only `y` or `n`.
- `the-deaner remove <name>` replaces `archive` and `forget`, with `--force` and `--delete-branch`.
- The docs and the cheat sheet explain what removing a worktree actually loses.

## 0.1.11 · 2026-11-14

- **`x` removes a project for real**: its tmux sessions stop too. Before, a forgotten project that was
  still open came straight back to the sidebar.
- The footer gets `a add | x remove`.

## 0.1.10 · 2026-11-14

- **`the-deaner` takes over the screen**: running it detaches any other window showing it (say, your
  desk when you SSH in), so the screen you're using sets the size. `the-deaner --shared` keeps both.
- After a restart, every window that was open comes back, even several at once.

## 0.1.9 · 2026-11-13

- The version this sidebar is running, dim at the bottom (`v0.1.9`), and in the cheat sheet's corner.
- The new-version notice reads `↑ 0.1.9 upgrade available`.

## 0.1.8 · 2026-11-13

- After `the-deaner upgrade --check`, the sidebar shows the new version within seconds, not within
  the hour.

## 0.1.7 · 2026-11-13

- **The sidebar switches to a newly installed version by itself**, within a few seconds, however it
  was installed (`brew upgrade`, `pacman -U`, …). No restart needed.
- A restore puts you back on the workspace you were last on.

## 0.1.6 · 2026-11-13

- **The project browser (`a`) keeps the path you type**: the list follows it, `Tab` completes into a
  folder, and backspacing over a `/` goes back out. An "Up to …" row goes up a folder.
- A footer row for `a add | x remove`.

## 0.1.5 · 2026-11-13

- "Updated to the-deaner …" once, the first time it starts on a new version.
- `the-deaner upgrade` runs `brew update` first on the Mac.

## 0.1.4 · 2026-11-13

- **`the-deaner upgrade`**: installs the newest release (pacman or Homebrew), then restarts into it
  and offers to restore your workspaces. `--check` only reports.
- **`the-deaner restart`**: saves, stops and comes back in the same window.
- The sidebar checks for a new version once a day and says so at the bottom. It never installs
  anything by itself.

## 0.1.3 · 2026-11-13

- A project that needs you gets an orange bar on the left and its name in bold cyan.
- An agent killed hard stops counting as working once its shell is back.
- The sidebar keeps the width you dragged it to, through window resizes and restarts.

## 0.1.2 · 2026-11-12

- Pull request and CI status update every 5 seconds while it matters: while a project's CI is running
  and right after you push from it. Otherwise every minute.

## 0.1.1 · 2026-11-12

- `p pin` in the footer.

## 0.1.0 · 2026-11-12

- The first release: a sidebar of your repos, worktrees and agents next to your own tmux, agent
  status from Claude Code's hooks, git and GitHub status, sections, pins, worktrees with `w`, peek
  with `v`, shutdown and restore with each claude resuming its conversation, and `the-deaner send`
  so agents can talk to each other. Homebrew and Arch packages.
