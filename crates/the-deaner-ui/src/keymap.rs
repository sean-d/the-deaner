//! The keymap: one table drives key handling, the in-app `?` overlay, and the
//! generated `docs/keybindings.md`. Change a binding here and all three
//! follow; CI fails if the docs file is stale.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use the_deaner_core::config::FrameKeys;

/// Everything the sidebar can do from the keyboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Down,
    Up,
    Top,
    Bottom,
    HalfPageDown,
    HalfPageUp,
    Open,
    NextAttention,
    Peek,
    ToggleCollapse,
    Filter,
    NewWorktree,
    NewTab,
    AddRepo,
    TogglePin,
    PinUp,
    PinDown,
    Archive,
    Park,
    Save,
    Refresh,
    Help,
    Quit,
    Toast,
}

/// A key as written in the table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Char(char),
    /// The letter with Ctrl held.
    Ctrl(char),
    Enter,
    Space,
    Up,
    Down,
    Home,
    End,
}

impl Key {
    fn label(self) -> String {
        match self {
            Self::Char(c) => c.to_string(),
            Self::Ctrl(c) => format!("Ctrl-{c}"),
            Self::Enter => "Enter".into(),
            Self::Space => "Space".into(),
            Self::Up => "↑".into(),
            Self::Down => "↓".into(),
            Self::Home => "Home".into(),
            Self::End => "End".into(),
        }
    }

    fn matches(self, key: &KeyEvent) -> bool {
        if key.modifiers.contains(KeyModifiers::ALT) {
            return false;
        }
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match (self, key.code) {
            (Self::Ctrl(a), KeyCode::Char(b)) => ctrl && a == b,
            _ if ctrl => false,
            (Self::Char(a), KeyCode::Char(b)) => a == b,
            (Self::Enter, KeyCode::Enter)
            | (Self::Space, KeyCode::Char(' '))
            | (Self::Up, KeyCode::Up)
            | (Self::Down, KeyCode::Down)
            | (Self::Home, KeyCode::Home)
            | (Self::End, KeyCode::End) => true,
            _ => false,
        }
    }
}

pub struct Binding {
    pub keys: &'static [Key],
    pub action: Action,
    pub section: &'static str,
    pub help: &'static str,
}

use Key::{Char, Ctrl, Down as KDown, End, Enter, Home, Space, Up as KUp};

pub const BINDINGS: &[Binding] = &[
    Binding {
        keys: &[Char('j'), KDown],
        action: Action::Down,
        section: "Navigate",
        help: "Move down",
    },
    Binding {
        keys: &[Char('k'), KUp],
        action: Action::Up,
        section: "Navigate",
        help: "Move up",
    },
    Binding {
        keys: &[Char('g'), Home],
        action: Action::Top,
        section: "Navigate",
        help: "Jump to top",
    },
    Binding {
        keys: &[Char('G'), End],
        action: Action::Bottom,
        section: "Navigate",
        help: "Jump to bottom",
    },
    Binding {
        keys: &[Ctrl('d')],
        action: Action::HalfPageDown,
        section: "Navigate",
        help: "Half a page down",
    },
    Binding {
        keys: &[Ctrl('u')],
        action: Action::HalfPageUp,
        section: "Navigate",
        help: "Half a page up",
    },
    Binding {
        keys: &[Enter],
        action: Action::Open,
        section: "Navigate",
        help: "Open workspace in the terminal (starts or unparks it)",
    },
    Binding {
        keys: &[Char('n')],
        action: Action::NextAttention,
        section: "Navigate",
        help: "Go to next agent needing you",
    },
    Binding {
        keys: &[Char('v')],
        action: Action::Peek,
        section: "Navigate",
        help: "Peek at an agent's output",
    },
    Binding {
        keys: &[Space],
        action: Action::ToggleCollapse,
        section: "Navigate",
        help: "Collapse / expand repo",
    },
    Binding {
        keys: &[Char('/')],
        action: Action::Filter,
        section: "Navigate",
        help: "Filter by repo or branch (Esc clears)",
    },
    Binding {
        keys: &[Char('w')],
        action: Action::NewWorktree,
        section: "Workspaces",
        help: "New worktree + session for a branch",
    },
    Binding {
        keys: &[Char('t')],
        action: Action::NewTab,
        section: "Workspaces",
        help: "New tab (agent, editor, shell)",
    },
    Binding {
        keys: &[Char('a')],
        action: Action::AddRepo,
        section: "Workspaces",
        help: "Open a project: browse folders, add and start it",
    },
    Binding {
        keys: &[Char('x')],
        action: Action::Archive,
        section: "Workspaces",
        help: "Archive worktree / forget repo (asks first)",
    },
    Binding {
        keys: &[Char('P')],
        action: Action::Park,
        section: "Workspaces",
        help: "Park: save and stop the session",
    },
    Binding {
        keys: &[Char('S')],
        action: Action::Save,
        section: "Workspaces",
        help: "Save a snapshot now",
    },
    Binding {
        keys: &[Char('p')],
        action: Action::TogglePin,
        section: "Pins",
        help: "Pin / unpin repo",
    },
    Binding {
        keys: &[Char('K')],
        action: Action::PinUp,
        section: "Pins",
        help: "Move pinned repo up",
    },
    Binding {
        keys: &[Char('J')],
        action: Action::PinDown,
        section: "Pins",
        help: "Move pinned repo down",
    },
    Binding {
        keys: &[Char('r')],
        action: Action::Refresh,
        section: "Other",
        help: "Refresh now, GitHub included",
    },
    Binding {
        keys: &[Char('?')],
        action: Action::Help,
        section: "Other",
        help: "Show / hide this help",
    },
    Binding {
        keys: &[Char('b')],
        action: Action::Toast,
        section: "Other",
        help: "Make toast",
    },
    Binding {
        keys: &[Char('q')],
        action: Action::Quit,
        section: "Other",
        help: "Detach: all keeps running, `the-deaner` comes back (quits when standalone)",
    },
];

/// The action bound to a key press, if any. Ctrl/Alt chords are never sidebar actions.
#[must_use]
pub fn action_for(key: &KeyEvent) -> Option<Action> {
    BINDINGS
        .iter()
        .find(|b| b.keys.iter().any(|k| k.matches(key)))
        .map(|b| b.action)
}

/// `Key1 / Key2` label for a binding.
#[must_use]
pub fn keys_label(b: &Binding) -> String {
    b.keys
        .iter()
        .map(|k| k.label())
        .collect::<Vec<_>>()
        .join(" / ")
}

/// Sections in table order.
#[must_use]
pub fn sections() -> Vec<&'static str> {
    let mut out: Vec<&'static str> = Vec::new();
    for b in BINDINGS {
        if !out.contains(&b.section) {
            out.push(b.section);
        }
    }
    out
}

/// Turn a tmux key name (`M-s`, `C-a`) into something readable (`Alt-s`, `Ctrl-a`).
#[must_use]
pub fn tmux_key_label(key: &str) -> String {
    the_deaner_core::tmux_keys::key_label(key)
}

/// Extra words people might search for when looking up a sidebar action.
#[must_use]
pub const fn search_terms(action: Action) -> &'static str {
    match action {
        Action::Open => "start switch go project repo",
        Action::Peek => "view look preview output scrollback see read",
        Action::NextAttention => "jump waiting needs attention orange finished done agent claude",
        Action::ToggleCollapse => "fold tree group",
        Action::Filter => "search find",
        Action::NewWorktree => "branch parallel create",
        Action::NewTab => {
            "window add open claude nvim neovim vim editor shell agent terminal right"
        }
        Action::AddRepo => {
            "add repo folder directory new select file browser browse system favorite"
        }
        Action::Archive => "delete remove worktree forget",
        Action::Park => "pause suspend stop",
        Action::Save => "snapshot backup",
        Action::TogglePin => "favorite top order",
        Action::PinUp | Action::PinDown => "reorder order favorite",
        Action::Help => "keys shortcuts cheat sheet",
        Action::Down | Action::Up | Action::Top | Action::Bottom => "navigate list",
        Action::HalfPageDown | Action::HalfPageUp => "scroll page more list navigate",
        Action::Quit => "quit exit leave detach close away shutdown stop",
        Action::Refresh => "",
        Action::Toast => "bread toaster fun easter egg",
    }
}

/// One frame binding: key label, description, extra search words.
pub struct FrameBinding {
    pub key: String,
    pub help: &'static str,
    pub terms: &'static str,
}

/// The frame bindings (tmux root table) with their descriptions.
#[must_use]
pub fn frame_bindings(keys: &FrameKeys) -> Vec<FrameBinding> {
    vec![
        FrameBinding {
            key: tmux_key_label(&keys.cheatsheet),
            help: "Open this searchable shortcut cheat sheet",
            terms: "help keys shortcuts which-key",
        },
        FrameBinding {
            key: tmux_key_label(&keys.focus),
            help: "Move focus between the sidebar and the terminal",
            terms: "navigate switch jump left right panel side",
        },
        FrameBinding {
            key: tmux_key_label(&keys.toggle_sidebar),
            help: "Hide / show the sidebar",
            terms: "panel left side projects list explorer collapse fullscreen",
        },
    ]
}

/// The full `keybindings.md` page, generated from [`BINDINGS`] and the default frame keys.
#[must_use]
pub fn markdown() -> String {
    let mut s = String::from(
        "<!-- Generated by `the-deaner keys --markdown`. Do not edit by hand; CI checks it is current. -->\n\n\
         # Keybindings\n\n\
         ## Frame keys\n\n\
         These work from anywhere in the the-deaner window (sidebar or terminal). \
         They are bound in the-deaner's outer tmux frame and can be changed under `[keys]` in `config.toml`.\n\n\
         | Key | Action |\n|---|---|\n",
    );
    for b in frame_bindings(&FrameKeys::default()) {
        s.push_str(&format!("| `{}` | {} |\n", b.key, b.help));
    }
    s.push_str(
        "\nYou can also click a sidebar row to open it, scroll the list with the wheel, \
         and drag the border between the sidebar and the terminal to resize. The width is remembered.\n",
    );
    s.push_str("\n## Sidebar keys\n\nThese work while the sidebar has focus.\n");
    for section in sections() {
        s.push_str(&format!("\n### {section}\n\n| Key | Action |\n|---|---|\n"));
        for b in BINDINGS.iter().filter(|b| b.section == section) {
            let keys = b
                .keys
                .iter()
                .map(|k| format!("`{}`", k.label()))
                .collect::<Vec<_>>()
                .join(" / ");
            s.push_str(&format!("| {keys} | {} |\n", b.help));
        }
    }
    s.push_str(
        "\n### In prompts and pickers\n\n\
         | Key | Action |\n|---|---|\n\
         | `Enter` | Confirm |\n\
         | `Esc` | Cancel |\n\
         | `j` / `k` / `↑` / `↓` | Move in a picker |\n\
         | `1`–`9` | Pick an item directly |\n\
         | `y` / `n` | Answer a confirmation |\n",
    );
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_key_is_bound_once() {
        let mut seen: Vec<Key> = Vec::new();
        for b in BINDINGS {
            for k in b.keys {
                assert!(!seen.contains(k), "{k:?} bound twice");
                seen.push(*k);
            }
        }
    }

    #[test]
    fn resolves_keys() {
        let ev = |c| KeyEvent::new(c, KeyModifiers::NONE);
        assert_eq!(action_for(&ev(KeyCode::Char('j'))), Some(Action::Down));
        assert_eq!(action_for(&ev(KeyCode::Down)), Some(Action::Down));
        assert_eq!(action_for(&ev(KeyCode::Char('K'))), Some(Action::PinUp));
        assert_eq!(
            action_for(&ev(KeyCode::Char(' '))),
            Some(Action::ToggleCollapse)
        );
        assert_eq!(
            action_for(&KeyEvent::new(KeyCode::Char('j'), KeyModifiers::CONTROL)),
            None
        );
        assert_eq!(action_for(&ev(KeyCode::Char('z'))), None);
    }

    #[test]
    fn markdown_lists_every_binding() {
        let md = markdown();
        for b in BINDINGS {
            assert!(md.contains(b.help), "missing {}", b.help);
        }
        assert!(md.contains("`Alt-s`"));
    }
}
