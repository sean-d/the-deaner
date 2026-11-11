//! Turns `tmux list-keys` output into task-oriented shortcuts ("New tab:
//! Ctrl-Space c"), so the cheat sheet shows the user's real bindings rather than
//! tmux's defaults.

/// One line of `tmux list-keys`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyBinding {
    pub table: String,
    pub key: String,
    pub command: String,
}

/// A thing the user can do in the terminal pane, and the keys that do it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TmuxShortcut {
    pub keys: String,
    pub what: &'static str,
    /// Extra words to match when searching.
    pub terms: &'static str,
}

fn next_token(s: &str) -> Option<(&str, &str)> {
    let s = s.trim_start();
    if s.is_empty() {
        return None;
    }
    let end = s.find(char::is_whitespace).unwrap_or(s.len());
    Some((&s[..end], s[end..].trim_start()))
}

/// Parse `tmux list-keys`. Lines that don't look like bindings are skipped.
#[must_use]
pub fn parse_list_keys(out: &str) -> Vec<KeyBinding> {
    out.lines()
        .filter_map(|line| {
            let (word, mut rest) = next_token(line)?;
            if word != "bind-key" {
                return None;
            }
            let mut table = "prefix".to_string();
            loop {
                let (token, after) = next_token(rest)?;
                match token {
                    "-T" => {
                        let (t, after) = next_token(after)?;
                        table = t.to_string();
                        rest = after;
                    }
                    "-r" => rest = after,
                    key => {
                        // tmux escapes keys like \; and \" when printing them.
                        let key = key
                            .strip_prefix('\\')
                            .filter(|k| !k.is_empty())
                            .unwrap_or(key);
                        return Some(KeyBinding {
                            table,
                            key: key.to_string(),
                            command: after.to_string(),
                        });
                    }
                }
            }
        })
        .collect()
}

/// A tmux key name made readable: `M-s` is `Alt-s`, `C-a` is `Ctrl-a`.
#[must_use]
pub fn key_label(key: &str) -> String {
    key.replace("M-", "Alt-")
        .replace("C-", "Ctrl-")
        .replace("S-", "Shift-")
}

fn is_mouse(key: &str) -> bool {
    [
        "Mouse",
        "Wheel",
        "DoubleClick",
        "TripleClick",
        "SecondClick",
    ]
    .iter()
    .any(|m| key.contains(m))
}

type Matcher = fn(&str) -> bool;

/// Keys for a command: any no-prefix binding first, then the prefix one.
fn keys_for(bindings: &[KeyBinding], prefix: &str, matches: Matcher) -> Option<String> {
    let find = |table: &str| {
        bindings.iter().find(|b| {
            // Menus mention commands like kill-pane, but the key only opens the menu.
            b.table == table
                && !is_mouse(&b.key)
                && !b.command.starts_with("display-menu")
                && matches(&b.command)
        })
    };
    let direct = find("root").map(|b| key_label(&b.key));
    let prefixed = find("prefix").map(|b| format!("{prefix} {}", key_label(&b.key)));
    match (direct, prefixed) {
        (Some(d), Some(p)) => Some(format!("{d} or {p}")),
        (d, p) => d.or(p),
    }
}

/// `Ctrl-h Ctrl-j Ctrl-k Ctrl-l` reads better as `Ctrl-h/j/k/l`.
fn join_directions(labels: &[String]) -> String {
    let split = |l: &str| {
        l.rfind('-')
            .map(|i| (l[..=i].to_string(), l[i + 1..].to_string()))
    };
    if let Some((first, _)) = labels.first().and_then(|l| split(l))
        && labels.iter().all(|l| l.starts_with(&first))
    {
        let tails: Vec<&str> = labels.iter().map(|l| &l[first.len()..]).collect();
        return format!("{first}{}", tails.join("/"));
    }
    labels.join("/")
}

fn pane_movement(bindings: &[KeyBinding], prefix: &str) -> Option<String> {
    let dirs = [
        "select-pane -L",
        "select-pane -D",
        "select-pane -U",
        "select-pane -R",
    ];
    let keys_in = |table: &str| -> Option<Vec<String>> {
        dirs.iter()
            .map(|d| {
                bindings
                    .iter()
                    .find(|b| b.table == table && !is_mouse(&b.key) && b.command.contains(d))
                    .map(|b| key_label(&b.key))
            })
            .collect()
    };
    if let Some(keys) = keys_in("root") {
        return Some(join_directions(&keys));
    }
    keys_in("prefix").map(|keys| format!("{prefix} {}", join_directions(&keys)))
}

/// Shortcuts for the common terminal tasks, in the order they're worth reading.
/// Anything the user has unbound is left out.
#[must_use]
pub fn shortcuts(bindings: &[KeyBinding], prefix: &str) -> Vec<TmuxShortcut> {
    let prefix = key_label(prefix);
    let table: &[(&'static str, &'static str, Matcher)] = &[
        ("New tab", "window add open create right", |c| {
            c.starts_with("new-window")
        }),
        ("Next tab", "window switch navigate move go right", |c| {
            c == "next-window"
        }),
        (
            "Previous tab",
            "window switch navigate move go back left",
            |c| c == "previous-window",
        ),
        ("Rename tab", "window title name", |c| {
            c.contains("rename-window")
        }),
        ("Close tab", "window kill delete quit", |c| {
            c.contains("kill-window")
        }),
        ("Split side by side", "pane vertical new", |c| {
            c.starts_with("split-window") && c.contains(" -h")
        }),
        ("Split stacked", "pane horizontal new", |c| {
            c.starts_with("split-window") && !c.contains(" -h")
        }),
        ("Close pane", "kill split delete", |c| {
            c.contains("kill-pane")
        }),
        ("Zoom pane", "fullscreen maximize split", |c| {
            c.starts_with("resize-pane -Z")
        }),
        ("Scroll back / copy", "history copy-mode select", |c| {
            c == "copy-mode"
        }),
    ];
    let mut out = Vec::new();
    for (what, terms, matches) in table {
        if let Some(keys) = keys_for(bindings, &prefix, *matches) {
            out.push(TmuxShortcut { keys, what, terms });
        }
        if *what == "Previous tab"
            && bindings
                .iter()
                .any(|b| b.table == "prefix" && b.command == "select-window -t :=1")
        {
            out.push(TmuxShortcut {
                keys: format!("{prefix} 0-9"),
                what: "Go to tab by number",
                terms: "window switch navigate move go jump",
            });
        }
        if *what == "Split stacked"
            && let Some(keys) = pane_movement(bindings, &prefix)
        {
            out.push(TmuxShortcut {
                keys,
                what: "Move between panes",
                terms: "split navigate switch focus left right up down",
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEYS: &str = r##"bind-key    -T copy-mode    q       send-keys -X cancel
bind-key    -T prefix \;      last-pane
bind-key    -T prefix c       new-window
bind-key    -T prefix n       next-window
bind-key    -T prefix M-n     next-window -a
bind-key    -T prefix p       previous-window
bind-key    -T prefix 1       select-window -t :=1
bind-key    -T prefix ,       command-prompt -I "#W" { rename-window "%%" }
bind-key    -T prefix &       confirm-before -p "kill-window #W? (y/n)" kill-window
bind-key    -T prefix |       split-window -h -c "#{pane_current_path}"
bind-key    -T prefix -       split-window -v -c "#{pane_current_path}"
bind-key    -T prefix >       display-menu -T "pane" "Kill" X { kill-pane } "Rename" R { rename-window }
bind-key    -T prefix x       confirm-before -p "kill-pane #P? (y/n)" kill-pane
bind-key    -T prefix z       resize-pane -Z
bind-key    -T prefix [       copy-mode
bind-key -r -T prefix Left    select-pane -L
bind-key -r -T prefix Down    select-pane -D
bind-key -r -T prefix Up      select-pane -U
bind-key -r -T prefix Right   select-pane -R
bind-key    -T root   M-Right next-window
bind-key    -T root   M-Left  previous-window
bind-key    -T root   WheelDownStatus next-window
bind-key    -T root   C-h     if-shell "$is_vim" "send-keys C-h" "select-pane -L"
bind-key    -T root   C-j     if-shell "$is_vim" "send-keys C-j" "select-pane -D"
bind-key    -T root   C-k     if-shell "$is_vim" "send-keys C-k" "select-pane -U"
bind-key    -T root   C-l     if-shell "$is_vim" "send-keys C-l" "select-pane -R"
"##;

    fn get<'a>(all: &'a [TmuxShortcut], what: &str) -> &'a str {
        &all.iter().find(|s| s.what == what).unwrap().keys
    }

    #[test]
    fn parses_tables_flags_and_escaped_keys() {
        let keys = parse_list_keys(KEYS);
        assert_eq!(keys[0].table, "copy-mode");
        assert_eq!(keys[1].key, ";", "tmux prints a backslash before ;");
        assert_eq!(keys[1].command, "last-pane");
        let left = keys.iter().find(|b| b.key == "Left").unwrap();
        assert_eq!(left.command, "select-pane -L", "-r is not the key");
        assert!(parse_list_keys("not a binding\n").is_empty());
    }

    #[test]
    fn shows_the_users_real_keys() {
        let all = shortcuts(&parse_list_keys(KEYS), "C-Space");
        assert_eq!(get(&all, "New tab"), "Ctrl-Space c");
        assert_eq!(
            get(&all, "Next tab"),
            "Alt-Right or Ctrl-Space n",
            "mouse keys are skipped"
        );
        assert_eq!(get(&all, "Go to tab by number"), "Ctrl-Space 0-9");
        assert_eq!(get(&all, "Close tab"), "Ctrl-Space &");
        assert_eq!(get(&all, "Close pane"), "Ctrl-Space x", "not the > menu");
        assert_eq!(get(&all, "Split side by side"), "Ctrl-Space |");
        assert_eq!(get(&all, "Split stacked"), "Ctrl-Space -");
        assert_eq!(get(&all, "Move between panes"), "Ctrl-h/j/k/l");
    }

    #[test]
    fn falls_back_to_prefix_movement_and_omits_unbound() {
        let keys: Vec<KeyBinding> = parse_list_keys(KEYS)
            .into_iter()
            .filter(|b| b.table != "root" && b.key != "c")
            .collect();
        let all = shortcuts(&keys, "C-b");
        assert_eq!(get(&all, "Move between panes"), "Ctrl-b Left/Down/Up/Right");
        assert!(
            !all.iter().any(|s| s.what == "New tab"),
            "unbound keys aren't invented"
        );
    }
}
