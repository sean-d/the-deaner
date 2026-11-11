//! Where the-deaner keeps its files.
//!
//! - config: `$XDG_CONFIG_HOME/the-deaner` (default `~/.config/the-deaner`), on macOS too.
//! - state:  `$XDG_STATE_HOME/the-deaner` (default `~/.local/state/the-deaner`).
//!
//! `THE_DEANER_HOME` overrides both (`$THE_DEANER_HOME/config`, `$THE_DEANER_HOME/state`);
//! tests use it to stay out of the real home directory.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Paths {
    pub config_dir: PathBuf,
    pub state_dir: PathBuf,
}

fn xdg(var: &str, fallback: &str) -> PathBuf {
    std::env::var_os(var)
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("/"))
                .join(fallback)
        })
}

/// Before it was the-deaner, it was deaner: move `<dir>/../deaner` to `dir` the first
/// time, so projects, pins, snapshots and config come along. Only when `dir` doesn't exist
/// yet; a failed move just starts fresh.
fn adopt_old_name(dir: &Path) {
    let old = dir.with_file_name("deaner");
    if !dir.exists() && old.is_dir() {
        let _ = std::fs::rename(&old, dir);
    }
}

impl Paths {
    #[must_use]
    pub fn from_env() -> Self {
        if let Some(home) = std::env::var_os("THE_DEANER_HOME").map(PathBuf::from) {
            return Self::under(&home);
        }
        let config_dir = xdg("XDG_CONFIG_HOME", ".config").join("the-deaner");
        let state_dir = xdg("XDG_STATE_HOME", ".local/state").join("the-deaner");
        adopt_old_name(&config_dir);
        adopt_old_name(&state_dir);
        Self {
            config_dir,
            state_dir,
        }
    }

    /// Everything under one directory (tests).
    #[must_use]
    pub fn under(root: &Path) -> Self {
        Self {
            config_dir: root.join("config"),
            state_dir: root.join("state"),
        }
    }

    #[must_use]
    pub fn config_file(&self) -> PathBuf {
        self.config_dir.join("config.toml")
    }

    #[must_use]
    pub fn state_file(&self) -> PathBuf {
        self.state_dir.join("state.toml")
    }

    #[must_use]
    pub fn snapshots_dir(&self) -> PathBuf {
        self.state_dir.join("snapshots")
    }

    #[must_use]
    pub fn latest_snapshot(&self) -> PathBuf {
        self.snapshots_dir().join("latest.toml")
    }

    #[must_use]
    pub fn parked_dir(&self) -> PathBuf {
        self.state_dir.join("parked")
    }

    #[must_use]
    pub fn frame_conf(&self) -> PathBuf {
        self.state_dir.join("frame.tmux.conf")
    }
}

/// Write `contents` to `path` atomically (temp file + rename), creating parent dirs.
///
/// # Errors
/// On any IO failure.
pub fn write_atomic(path: &Path, contents: &str) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension(format!("tmp-{}", std::process::id()));
    std::fs::write(&tmp, contents)?;
    std::fs::rename(&tmp, path)
}

/// Expand a leading `~` and make the path absolute (relative to the cwd).
#[must_use]
pub fn expand(raw: &str) -> PathBuf {
    let path = match raw.strip_prefix("~") {
        Some(rest) if rest.is_empty() || rest.starts_with('/') => dirs::home_dir()
            .unwrap_or_default()
            .join(rest.trim_start_matches('/')),
        _ => PathBuf::from(raw),
    };
    let abs = if path.is_absolute() {
        path
    } else {
        std::env::current_dir().unwrap_or_default().join(path)
    };
    abs.canonicalize().unwrap_or(abs)
}

#[cfg(test)]
mod adopt_tests {
    use super::adopt_old_name;

    #[test]
    fn the_old_deaner_folder_moves_over_once() {
        let root = tempfile::tempdir().unwrap();
        let old = root.path().join("deaner");
        let new = root.path().join("the-deaner");
        std::fs::create_dir_all(old.join("snapshots")).unwrap();
        std::fs::write(old.join("state.toml"), "pins = []").unwrap();

        adopt_old_name(&new);
        assert!(new.join("state.toml").is_file() && new.join("snapshots").is_dir());
        assert!(!old.exists());

        // Both exist: leave them alone.
        std::fs::create_dir_all(&old).unwrap();
        adopt_old_name(&new);
        assert!(old.exists() && new.join("state.toml").is_file());
    }
}
