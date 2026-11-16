# Projects on every machine

Your sidebar lives on one machine. A **projects file** lists your projects so another machine can have
the same ones: `the-deaner export` writes it from the sidebar you have, and `the-deaner sync` adds what
it lists to the sidebar you're on, cloning what's missing.

## The projects file

`~/.config/the-deaner/projects.toml`:

```toml
[[project]]
path = "~/dev/rust/the-deaner"
remote = "git@github.com:sean-d/the-deaner.git"
pinned = true

[[project]]
path = "~/dev/go/kexp-api"
remote = "git@github.com:KEXP/kexp-api.git"

[[project]]
path = "~/temp"
```

| Field | What it means |
|---|---|
| `path` | Where the project lives. Written with `~`, so one file fits `/Users/you` and `/home/you`. |
| `remote` | Its git remote (`origin`): where `sync` clones it from. Leave it out for a plain folder. |
| `pinned` | Pin it on every machine. Pinned projects are pinned in the file's order. |

Keep it in your dotfiles, so every machine has the same one. It stays out of the-deaner's own repo,
so your project names stay private. Each machine's own state (sidebar width, what's asleep, the last
project you were on, favorites) stays in `state.toml` and isn't part of it.

## Export: write it from your sidebar

```sh
the-deaner export --show     # print it, to look at first
the-deaner export            # save it as ~/.config/the-deaner/projects.toml
```

Every project in the sidebar, pinned ones first in your order, with its path and its `origin` remote
(plain folders have none). If you already have a projects file and it's different, `export` asks
before replacing it (identical, it says it's up to date). `--file <path>` saves it somewhere else.

## Sync: bring them to this machine

```sh
the-deaner sync
```

It reads the projects file, works out what to do with each project, and **shows you the plan before
doing anything**:

```text
From ~/.config/the-deaner/projects.toml:
  skip    the-deaner     already in the sidebar
  clone   kexp-api       git@github.com:KEXP/kexp-api.git → ~/dev/go/kexp-api
  add     tally          ~/dev/python/tally
  warn    website-wp     ~/dev/website-wp has a different remote (git@github.com:someone/fork.git); left alone
  note    temp           ~/temp doesn't exist here; nothing to clone
  pin     the-deaner     pinned, in the file's order
Do it? [y/N]
```

| The project, on this machine | What sync does |
|---|---|
| In the sidebar already, same path and remote | Skips it |
| In the sidebar at that path, with a different remote | Warns and leaves it alone |
| The folder exists, not in the sidebar, same remote | Adds it |
| The folder exists, with a different remote, or not a git repo when the file has a remote | Warns and leaves it alone |
| The folder is missing, and the file has a remote | Clones it there, then adds it |
| A plain folder (no remote) that exists | Adds it |
| A plain folder that doesn't exist here | Notes it: there's nothing to clone |
| `pinned = true` | Pins it; pinned projects follow the file's order, and pins you already have that it doesn't mention stay, after them |

Remotes match when they point at the same repo, however they're written: `git@github.com:you/x.git`,
`ssh://git@github.com/you/x` and `https://github.com/you/x` are the same remote.

Clones run one at a time with git's own progress showing; one that fails doesn't stop the rest. Sync
**never removes anything**: a project you delete from the file stays in the sidebar until you remove it
there (`x`). It never runs by itself, either; you run it when you want it.

- `the-deaner sync --yes` skips the question (for scripts, like setting up a new machine).
- `the-deaner sync --file <path>` reads another file.
- Running it again when nothing has changed says "Nothing to do."

## Setting up a new machine

1. Install the-deaner (see [Getting started](getting-started.md)).
2. Put your dotfiles in place, including `~/.config/the-deaner/projects.toml`.
3. `the-deaner sync` and answer `y`: your projects are cloned and in the sidebar, pinned as before.
4. `the-deaner`.

When you add a project on one machine, `the-deaner export` there, commit the file in your
dotfiles, and `the-deaner sync` on the other.
