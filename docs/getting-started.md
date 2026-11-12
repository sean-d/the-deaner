# Getting started

## Requirements

- **tmux 3.3 or newer** (`pacman -S tmux` on Arch/Omarchy, `brew install tmux` on macOS)
- **git**
- Optional: **gh** (GitHub CLI, logged in) for pull request status

## Install

From source (needs a Rust toolchain, stable):

```sh
cargo install --locked --git https://github.com/sean-d/the-deaner the-deaner
```

From a local checkout:

```sh
cargo install --locked --path crates/the-deaner
```

`--locked` builds with the exact dependency versions in `Cargo.lock`, the ones CI tested.

Packages for Arch-based distros and Homebrew are on the way.

## First run

```sh
the-deaner add ~/dev/shop-api ~/dev/notes   # optional: or browse for them with `a` in the sidebar
the-deaner install-hooks                    # optional: agent status for claude
the-deaner                                  # open the frame
```

`the-deaner` opens a full-window frame: the sidebar on the left, a terminal on the right.

1. Move to a repo with `j`/`k` and press `Enter`. the-deaner starts a tmux session for it with your
   default tabs (`nvim`, `claude` and a `shell`) and shows it on the right.
2. Press `Alt-s` to jump between the sidebar and the terminal.
3. In the terminal you are in plain tmux (your `~/.tmux.conf`, your prefix). Switch tabs the way
   you always do.
4. Press `Alt-/` anywhere to search every shortcut: try typing "hide" or "new tab".

Close the terminal window whenever you like. Everything keeps running. Run `the-deaner` again to come back.

## Alt keys (macOS and Linux)

the-deaner's frame keys use Alt (`Alt-s`, `Alt-e`, `Alt-/`), and so do many tmux configs.

**macOS:** your terminal must send Option as Alt. Otherwise Option types special characters
(`Option-s` gives `ß`).

| Terminal | Setting |
|---|---|
| Ghostty | `macos-option-as-alt = true` (already the default on U.S. layouts) |
| Alacritty | `option_as_alt = "Both"` under `[window]` |
| kitty | `macos_option_as_alt yes` |
| iTerm2 | Profiles → Keys → Left/Right Option key: **Esc+** |
| Terminal.app | Settings → Profiles → Keyboard → **Use Option as Meta key** |

These settings are macOS-only. Linux terminals ignore them, so one shared terminal config
works on both.

**Ghostty on macOS and Alt-arrows:** by default Ghostty sends `Alt-Left` and `Alt-Right` as `ESC b` and
`ESC f` (jump a word in the shell). Terminals read those as `Alt-b` and `Alt-f`, so tmux bindings on
Alt-arrows never fire. To send real Alt-arrows instead:

```text
keybind = alt+arrow_left=unbind
keybind = alt+arrow_right=unbind
```

**Linux:** Alt works out of the box in Ghostty, Alacritty, kitty and foot. The only thing that
can get in the way is your window manager: if Hyprland (or another compositor) binds `Alt+S`
or `Alt+E`, it takes the key before the terminal sees it. Either unbind it there or pick other
frame keys:

```toml
# ~/.config/the-deaner/config.toml
[keys]
focus = "M-o"            # any tmux key name: M- = Alt, C- = Ctrl, F1..F12
toggle_sidebar = "M-i"
```

Frame keys are caught before anything inside the terminal sees them, so don't pick your tmux
prefix or a key an app you use needs.

If a frame key does nothing, check which program is catching it: the window manager, then the
terminal, then tmux.

## Mouse: links and copying

tmux has the mouse inside the-deaner, so a few clicks need a modifier to reach your terminal instead
(these are Ghostty's; most terminals use Shift the same way):

| To | Do |
|---|---|
| Open a link in your browser | `Ctrl+Shift+click` it (`Cmd+Shift+click` on macOS) |
| Copy text | Drag to select and let go: it's copied. `Shift+drag` uses the terminal's own selection instead |
| Copy a word | Double-click it |

Plain clicks go to the-deaner: click a workspace in the sidebar to open it, drag the border to
resize the sidebar, scroll with the wheel.

## Omarchy / Hyprland

Bind a key to open the-deaner in your terminal, for example in `~/.config/hypr/bindings.conf`:

```ini
bind = SUPER, D, exec, ghostty -e the-deaner
```

(Use `alacritty -e the-deaner` if Alacritty is your terminal.)

The frame sets `RGB` and extended-keys support for its panes. If colors or Shift+Enter in claude
misbehave in the inner tmux, add this to your `~/.tmux.conf`:

```tmux
set -as terminal-features ",*:RGB"
set -g extended-keys on
set -as terminal-features ",*:extkeys"
```

## Updating

```sh
cargo install --locked --force --git https://github.com/sean-d/the-deaner the-deaner
```

Running sessions are not affected. The sidebar picks up the new binary the next time it restarts
(or run `the-deaner shutdown`, then `the-deaner`).
