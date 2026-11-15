# Development

## Toolchain

Stable Rust. There is no `rust-toolchain.toml`; `rustup update` is all you ever need. The minimum
supported version is set in `Cargo.toml` (`rust-version`) and checked in CI.

Useful tools:

```sh
cargo install --locked cargo-nextest bacon cargo-insta
```

You also need `tmux` and `git` on the `PATH`. The integration tests start real, isolated tmux servers.

## Everyday loop

```sh
bacon            # clippy on every save (press t for tests, n for nextest)
cargo nextest run --workspace --all-features
cargo run -- ls  # run the CLI from source
```

To try the frame without touching your real setup, isolate everything:

```sh
export THE_DEANER_HOME=/tmp/the-deaner-dev THE_DEANER_SOCKET=the-deaner-dev THE_DEANER_FRAME_SOCKET=the-deaner-dev-frame
cargo run
```

## Tests

| Kind | Where | Notes |
|---|---|---|
| Unit | `crates/*/src/**` | Parsers, ordering, config, snapshot rules |
| tmux integration | `crates/the-deaner-tmux/tests` | Real tmux via `TestServer` (own socket, no user config) |
| git integration | `crates/the-deaner-git/src` | Temp repos, real `git worktree` |
| Engine end-to-end | `crates/the-deaner/tests/engine.rs` | Create, hooks, save, kill server, restore with `--resume`, sleep, remove |
| UI snapshots | `crates/the-deaner-ui/tests` | `insta` + ratatui `TestBackend` |
| Docs freshness | `crates/the-deaner/tests/docs.rs` | Generated pages must match the code |

Snapshot workflow: after a UI change, run `cargo insta test --review` and accept or reject each diff.

## Generated docs

`keybindings.md` and `cli.md` are generated from the keymap table and the clap definitions. After
changing either, regenerate them:

```sh
UPDATE_DOCS=1 cargo test -p the-deaner --test docs
```

CI fails if they are out of date. The docs are plain markdown; GitHub renders them, diagrams included.

The same files are compiled into the binary as the built-in guide (`Tab` in the cheat sheet,
`the-deaner guide`; the page list is `guide_pages()` in `crates/the-deaner/src/cheatsheet.rs`), so a
doc change ships with the next release. `the-deaner-ui`'s `markdown` module draws them: headings,
lists, code, tables wrapped to fit, and mermaid diagrams drawn as text by
[mmdflux](https://github.com/kevinswiber/mmdflux). A left-to-right diagram too wide for the screen is
redrawn top to bottom; one that still doesn't fit is shown as its source. A test checks that every
page draws and that every diagram is drawn (not shown as source) at 140 columns. If mmdflux lays a
diagram out badly, an invisible link (`a ~~~ b`) can nudge it without changing what it says, as in
the README's "How it works".

## Fast builds

- The workspace is split into five crates, so an edit rebuilds only the crates downstream of it.
  Dependencies are compiled once and cached.
- Dev profile: `debug = "line-tables-only"` for our crates, no debug info for dependencies.
  Artifacts are smaller and links are faster.
- No C dependencies: git and tmux are called as programs, not linked.
- Linux: lld is the default linker on x86_64 and is fast. For more speed, install `mold` and
  add to `~/.cargo/config.toml`:

  ```toml
  [target.x86_64-unknown-linux-gnu]
  rustflags = ["-C", "link-arg=-fuse-ld=mold"]
  ```

## Releases

1. Bump `version` in the workspace `Cargo.toml` (and `Cargo.lock`, by building), commit, push.
2. Wait for CI on `main` to pass.
3. Tag `vX.Y.Z` and push the tag.

The release workflow builds Linux (x86_64, aarch64) and macOS (arm64) tarballs, builds the Arch
package from the Linux x86_64 binary, publishes the GitHub release with everything and a `.sha256`
for each file, and then pushes the updated formula to `sean-d/homebrew-tap`.

`the-deaner upgrade` and the sidebar's `↑` notice read that release, so they depend on its shape:
the tag is `v<version>`, and the Arch package is named `the-deaner-<version>-1-<arch>.pkg.tar.zst`
with a `.sha256` beside it. The release shows up on GitHub a little before its files finish
uploading; an upgrade run in that gap says there's no Arch package yet and changes nothing.

To try the upgrade path without a real release, build with a lower `version` in `Cargo.toml` and
run `the-deaner upgrade --check` with that binary (and throwaway `THE_DEANER_HOME`,
`THE_DEANER_SOCKET` and `THE_DEANER_FRAME_SOCKET`).
