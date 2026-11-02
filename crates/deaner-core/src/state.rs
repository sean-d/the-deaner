//! Persisted UI state (`~/.local/state/deaner/state.toml`): known repos, pin
//! order, collapsed groups, sidebar width.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoEntry {
    /// Main checkout root (or a plain folder).
    pub path: PathBuf,
    /// Display name, also the base of the tmux session name.
    pub name: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct State {
    pub repos: Vec<RepoEntry>,
    /// Pinned repo paths, in the user's order. Pinned repos never move on their own.
    pub pins: Vec<PathBuf>,
    /// Repo groups collapsed in the sidebar.
    pub collapsed: Vec<PathBuf>,
    /// Last sidebar width the user dragged to.
    pub sidebar_width: Option<u16>,
    /// Session the terminal pane showed last, reattached first.
    pub last_session: Option<String>,
}

impl State {
    /// # Errors
    /// Returns the TOML error for invalid syntax.
    pub fn from_toml(text: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(text)
    }

    #[must_use]
    pub fn to_toml(&self) -> String {
        toml::to_string_pretty(self).expect("state always serializes")
    }

    #[must_use]
    pub fn repo(&self, path: &Path) -> Option<&RepoEntry> {
        self.repos.iter().find(|r| r.path == path)
    }

    /// Add a repo if it is not known yet. Returns the entry either way.
    pub fn add_repo(&mut self, path: PathBuf, name: String) -> &RepoEntry {
        if let Some(i) = self.repos.iter().position(|r| r.path == path) {
            return &self.repos[i];
        }
        self.repos.push(RepoEntry { path, name });
        self.repos.last().expect("just pushed")
    }

    /// Forget a repo (does not touch the disk or its sessions).
    pub fn remove_repo(&mut self, path: &Path) -> bool {
        let before = self.repos.len();
        self.repos.retain(|r| r.path != path);
        self.pins.retain(|p| p != path);
        self.collapsed.retain(|p| p != path);
        before != self.repos.len()
    }

    #[must_use]
    pub fn is_pinned(&self, path: &Path) -> bool {
        self.pins.iter().any(|p| p == path)
    }

    /// Pin (appending to the end of the pinned list) or unpin. Returns the new pinned state.
    pub fn toggle_pin(&mut self, path: &Path) -> bool {
        if self.is_pinned(path) {
            self.pins.retain(|p| p != path);
            false
        } else {
            self.pins.push(path.to_path_buf());
            true
        }
    }

    /// Move a pinned repo up (`delta < 0`) or down. Clamps at the ends; does
    /// nothing for unpinned repos. Returns whether anything moved.
    pub fn move_pin(&mut self, path: &Path, delta: isize) -> bool {
        let Some(from) = self.pins.iter().position(|p| p == path) else {
            return false;
        };
        let last = self.pins.len() as isize - 1;
        let to = (from as isize + delta).clamp(0, last) as usize;
        if to == from {
            return false;
        }
        let item = self.pins.remove(from);
        self.pins.insert(to, item);
        true
    }

    #[must_use]
    pub fn is_collapsed(&self, path: &Path) -> bool {
        self.collapsed.iter().any(|p| p == path)
    }

    pub fn toggle_collapsed(&mut self, path: &Path) {
        if self.is_collapsed(path) {
            self.collapsed.retain(|p| p != path);
        } else {
            self.collapsed.push(path.to_path_buf());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state_with(names: &[&str]) -> State {
        let mut s = State::default();
        for n in names {
            s.add_repo(PathBuf::from(format!("/r/{n}")), (*n).to_string());
        }
        s
    }

    #[test]
    fn add_is_idempotent() {
        let mut s = state_with(&["a"]);
        s.add_repo("/r/a".into(), "renamed".into());
        assert_eq!(s.repos.len(), 1);
        assert_eq!(s.repos[0].name, "a");
    }

    #[test]
    fn pin_order_is_user_controlled() {
        let mut s = state_with(&["a", "b", "c"]);
        for n in ["c", "a", "b"] {
            assert!(s.toggle_pin(Path::new(&format!("/r/{n}"))));
        }
        let names =
            |s: &State| -> Vec<String> { s.pins.iter().map(|p| p.display().to_string()).collect() };
        assert_eq!(names(&s), ["/r/c", "/r/a", "/r/b"]);

        assert!(s.move_pin(Path::new("/r/b"), -1));
        assert_eq!(names(&s), ["/r/c", "/r/b", "/r/a"]);
        assert!(!s.move_pin(Path::new("/r/c"), -1), "clamped at the top");
        assert!(s.move_pin(Path::new("/r/c"), 10));
        assert_eq!(names(&s), ["/r/b", "/r/a", "/r/c"]);

        assert!(!s.toggle_pin(Path::new("/r/a")));
        assert_eq!(names(&s), ["/r/b", "/r/c"]);
    }

    #[test]
    fn remove_repo_drops_pin_and_collapse() {
        let mut s = state_with(&["a", "b"]);
        s.toggle_pin(Path::new("/r/a"));
        s.toggle_collapsed(Path::new("/r/a"));
        assert!(s.remove_repo(Path::new("/r/a")));
        assert!(s.pins.is_empty() && s.collapsed.is_empty());
        assert!(!s.remove_repo(Path::new("/r/zzz")));
    }

    #[test]
    fn toml_round_trip() {
        let mut s = state_with(&["a", "b"]);
        s.toggle_pin(Path::new("/r/b"));
        s.sidebar_width = Some(28);
        let back = State::from_toml(&s.to_toml()).unwrap();
        assert_eq!(back, s);
        assert_eq!(State::from_toml("").unwrap(), State::default());
    }
}
