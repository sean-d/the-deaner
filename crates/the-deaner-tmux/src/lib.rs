//! Thin, typed wrapper over the `tmux` CLI.
//!
//! Every call shells out to `tmux`; there is no long-lived connection. Servers
//! are addressed by socket name (`tmux -L <name>`), or inherited from `$TMUX`
//! when running inside a pane (agent hooks do this).
//!
//! With the `test-support` feature, [`TestServer`] gives each test its own
//! throwaway server that ignores the user's `~/.tmux.conf`.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::Duration;
use the_deaner_core::pane::{PaneInfo, pane_format};
use the_deaner_core::process::output_with_timeout;

#[cfg(feature = "test-support")]
mod test_server;
#[cfg(feature = "test-support")]
pub use test_server::TestServer;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("could not run tmux: {0} (is tmux installed and on PATH?)")]
    Spawn(std::io::Error),
    #[error("tmux {args}: no answer within {}s", TIMEOUT.as_secs())]
    Timeout { args: String },
    #[error("tmux {args}: {stderr}")]
    Failed { args: String, stderr: String },
}

pub type Result<T> = std::result::Result<T, Error>;

/// tmux answers in milliseconds; anything this slow means a wedged server, and
/// the-deaner must not hang with it.
pub const TIMEOUT: Duration = Duration::from_secs(5);

fn join_args<S: AsRef<OsStr>>(args: &[S]) -> String {
    args.iter()
        .map(|a| a.as_ref().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Exact-match target for a session (works for session, window and pane targets).
#[must_use]
pub fn session_target(name: &str) -> String {
    format!("={name}:")
}

/// Exact-match target for a window by index.
#[must_use]
pub fn window_target(session: &str, index: u32) -> String {
    format!("={session}:{index}")
}

/// Where an option lives.
#[derive(Debug, Clone, Copy)]
pub enum Scope<'a> {
    Server,
    Session(&'a str),
    Pane(&'a str),
}

#[derive(Debug, Clone)]
enum Socket {
    /// Use `$TMUX` (or the default server).
    Inherit,
    Named(String),
}

/// Handle to one tmux server.
#[derive(Debug, Clone)]
pub struct Tmux {
    socket: Socket,
    config: Option<PathBuf>,
}

/// A new window or pane: its id (`%3`) and window index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Created {
    pub pane_id: String,
    pub window_index: u32,
}

impl Tmux {
    /// Server on `tmux -L <name>`.
    #[must_use]
    pub fn named(name: impl Into<String>) -> Self {
        Self {
            socket: Socket::Named(name.into()),
            config: None,
        }
    }

    /// The server this process runs inside (`$TMUX`), e.g. from an agent hook.
    #[must_use]
    pub const fn inherit() -> Self {
        Self {
            socket: Socket::Inherit,
            config: None,
        }
    }

    /// Start the server with this config file instead of `~/.tmux.conf`.
    #[must_use]
    pub fn with_config(mut self, path: impl Into<PathBuf>) -> Self {
        self.config = Some(path.into());
        self
    }

    #[must_use]
    pub fn socket_name(&self) -> Option<&str> {
        match &self.socket {
            Socket::Named(n) => Some(n),
            Socket::Inherit => None,
        }
    }

    /// A `tmux` command addressed at this server, for callers that need to
    /// spawn it themselves (e.g. `attach` with an inherited terminal).
    #[must_use]
    pub fn command(&self) -> Command {
        let mut cmd = Command::new("tmux");
        if let Socket::Named(name) = &self.socket {
            cmd.arg("-L").arg(name);
            // A server started from inside another tmux must not inherit its socket.
            cmd.env_remove("TMUX");
        }
        if let Some(cfg) = &self.config {
            cmd.arg("-f").arg(cfg);
        }
        cmd
    }

    fn output<S: AsRef<OsStr>>(&self, args: &[S]) -> Result<Output> {
        output_with_timeout(self.command().args(args), TIMEOUT).map_err(|e| {
            if e.kind() == std::io::ErrorKind::TimedOut {
                Error::Timeout {
                    args: join_args(args),
                }
            } else {
                Error::Spawn(e)
            }
        })
    }

    /// Run a tmux command and return stdout.
    ///
    /// # Errors
    /// [`Error::Failed`] when tmux exits non-zero.
    pub fn run<S: AsRef<OsStr>>(&self, args: &[S]) -> Result<String> {
        let out = self.output(args)?;
        if out.status.success() {
            Ok(String::from_utf8_lossy(&out.stdout).into_owned())
        } else {
            Err(Error::Failed {
                args: join_args(args),
                stderr: String::from_utf8_lossy(&out.stderr).trim().to_string(),
            })
        }
    }

    /// Run a command, reporting only success.
    #[must_use]
    pub fn ok<S: AsRef<OsStr>>(&self, args: &[S]) -> bool {
        self.output(args).is_ok_and(|o| o.status.success())
    }

    /// Whether the server is running.
    #[must_use]
    pub fn is_running(&self) -> bool {
        self.ok(&["list-sessions"])
    }

    /// Session names. Empty when the server is not running.
    #[must_use]
    pub fn sessions(&self) -> Vec<String> {
        self.run(&["list-sessions", "-F", "#{session_name}"])
            .map(|s| s.lines().map(str::to_string).collect())
            .unwrap_or_default()
    }

    #[must_use]
    pub fn has_session(&self, name: &str) -> bool {
        self.ok(&["has-session", "-t", &session_target(name)])
    }

    /// Create a detached session whose first window is `window_name` in `cwd`.
    ///
    /// # Errors
    /// When tmux refuses (e.g. duplicate name).
    pub fn new_session(&self, name: &str, cwd: &Path, window_name: &str) -> Result<Created> {
        let out = self.run(&[
            OsStr::new("new-session"),
            OsStr::new("-d"),
            OsStr::new("-P"),
            OsStr::new("-F"),
            OsStr::new("#{pane_id} #{window_index}"),
            OsStr::new("-s"),
            OsStr::new(name),
            OsStr::new("-n"),
            OsStr::new(window_name),
            OsStr::new("-c"),
            cwd.as_os_str(),
        ])?;
        Ok(parse_created(&out))
    }

    /// Add a window at the end of `session`.
    ///
    /// # Errors
    /// When the session does not exist.
    pub fn new_window(&self, session: &str, cwd: &Path, name: &str) -> Result<Created> {
        let target = session_target(session);
        let out = self.run(&[
            OsStr::new("new-window"),
            OsStr::new("-d"),
            OsStr::new("-P"),
            OsStr::new("-F"),
            OsStr::new("#{pane_id} #{window_index}"),
            OsStr::new("-t"),
            OsStr::new(&target),
            OsStr::new("-n"),
            OsStr::new(name),
            OsStr::new("-c"),
            cwd.as_os_str(),
        ])?;
        Ok(parse_created(&out))
    }

    /// Split the window containing `target` (a pane id), returning the new pane.
    ///
    /// # Errors
    /// When the target does not exist.
    pub fn split(&self, target: &str, cwd: &Path) -> Result<Created> {
        let out = self.run(&[
            OsStr::new("split-window"),
            OsStr::new("-d"),
            OsStr::new("-P"),
            OsStr::new("-F"),
            OsStr::new("#{pane_id} #{window_index}"),
            OsStr::new("-t"),
            OsStr::new(target),
            OsStr::new("-c"),
            cwd.as_os_str(),
        ])?;
        Ok(parse_created(&out))
    }

    /// # Errors
    /// When the target does not exist.
    pub fn set_option(&self, scope: Scope<'_>, key: &str, value: &str) -> Result<()> {
        match scope {
            Scope::Server => self.run(&["set-option", "-g", key, value]),
            Scope::Session(s) => self.run(&["set-option", "-t", &session_target(s), key, value]),
            Scope::Pane(p) => self.run(&["set-option", "-p", "-t", p, key, value]),
        }
        .map(drop)
    }

    /// Remove an option; succeeds if it was not set.
    ///
    /// # Errors
    /// When the target does not exist.
    pub fn unset_option(&self, scope: Scope<'_>, key: &str) -> Result<()> {
        match scope {
            Scope::Server => self.run(&["set-option", "-gu", key]),
            Scope::Session(s) => self.run(&["set-option", "-u", "-t", &session_target(s), key]),
            Scope::Pane(p) => self.run(&["set-option", "-pu", "-t", p, key]),
        }
        .map(drop)
    }

    /// Read a single option value (`None` when unset).
    #[must_use]
    pub fn show_option(&self, scope: Scope<'_>, key: &str) -> Option<String> {
        let out = match scope {
            Scope::Server => self.run(&["show-options", "-gqv", key]),
            Scope::Session(s) => self.run(&["show-options", "-qv", "-t", &session_target(s), key]),
            Scope::Pane(p) => self.run(&["show-options", "-pqv", "-t", p, key]),
        }
        .ok()?;
        let v = out.trim_end_matches('\n');
        (!v.is_empty()).then(|| v.to_string())
    }

    /// Evaluate a format in the context of `target` (`display-message -p`).
    #[must_use]
    pub fn display(&self, target: Option<&str>, format: &str) -> Option<String> {
        let mut args = vec!["display-message", "-p"];
        if let Some(t) = target {
            args.extend(["-t", t]);
        }
        args.push(format);
        self.run(&args)
            .ok()
            .map(|s| s.trim_end_matches('\n').to_string())
    }

    /// Type `text` literally into a pane, then press Enter if `enter`.
    ///
    /// # Errors
    /// When the pane does not exist.
    pub fn send_text(&self, target: &str, text: &str, enter: bool) -> Result<()> {
        if !text.is_empty() {
            self.run(&["send-keys", "-t", target, "-l", "--", text])?;
        }
        if enter {
            self.run(&["send-keys", "-t", target, "Enter"])?;
        }
        Ok(())
    }

    /// The text currently visible in a pane.
    #[must_use]
    pub fn capture_pane(&self, target: &str) -> Option<String> {
        self.run(&["capture-pane", "-p", "-t", target]).ok()
    }

    /// Send named keys (`Escape`, `C-c`, …).
    ///
    /// # Errors
    /// When the pane does not exist.
    pub fn send_keys(&self, target: &str, keys: &[&str]) -> Result<()> {
        let mut args = vec!["send-keys", "-t", target];
        args.extend_from_slice(keys);
        self.run(&args).map(drop)
    }

    /// All panes on the server with the-deaner's options resolved. Empty when not running.
    #[must_use]
    pub fn panes(&self) -> Vec<PaneInfo> {
        self.run(&["list-panes", "-a", "-F", &pane_format()])
            .map(|out| PaneInfo::parse_all(&out))
            .unwrap_or_default()
    }

    /// # Errors
    /// When the session does not exist.
    pub fn kill_session(&self, name: &str) -> Result<()> {
        self.run(&["kill-session", "-t", &session_target(name)])
            .map(drop)
    }

    /// Start a tmux command without waiting for it (for commands that block until
    /// the user finishes, like `display-popup -E`).
    ///
    /// # Errors
    /// When tmux can't be started.
    pub fn spawn_detached(&self, args: &[&str]) -> Result<()> {
        let mut child = self
            .command()
            .args(args)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(Error::Spawn)?;
        std::thread::spawn(move || child.wait());
        Ok(())
    }

    /// Stop the server. Succeeds if it was not running.
    pub fn kill_server(&self) {
        let _ = self.output(&["kill-server"]);
    }

    /// # Errors
    /// When the layout does not fit the window.
    pub fn select_layout(&self, window: &str, layout: &str) -> Result<()> {
        self.run(&["select-layout", "-t", window, layout]).map(drop)
    }

    /// # Errors
    /// When the window does not exist.
    pub fn select_window(&self, window: &str) -> Result<()> {
        self.run(&["select-window", "-t", window]).map(drop)
    }

    /// # Errors
    /// When the pane does not exist.
    pub fn select_pane(&self, pane: &str) -> Result<()> {
        self.run(&["select-pane", "-t", pane]).map(drop)
    }

    /// Point the client on terminal `client_tty` (or the current client) at `session`.
    ///
    /// # Errors
    /// When the client or session does not exist.
    pub fn switch_client(&self, client_tty: Option<&str>, session: &str) -> Result<()> {
        let target = session_target(session);
        let mut args = vec!["switch-client"];
        if let Some(c) = client_tty {
            args.extend(["-c", c]);
        }
        args.extend(["-t", &target]);
        self.run(&args).map(drop)
    }

    /// The tty of the client used most recently: with a window at the desk and another
    /// over SSH, the one you're typing in.
    #[must_use]
    pub fn latest_client(&self) -> Option<String> {
        let out = self
            .run(&["list-clients", "-F", "#{client_activity} #{client_tty}"])
            .ok()?;
        latest_of(&out)
    }

    /// `(tty, session)` of each attached client.
    #[must_use]
    pub fn clients(&self) -> Vec<(String, String)> {
        self.run(&["list-clients", "-F", "#{client_tty} #{client_session}"])
            .map(|out| {
                out.lines()
                    .filter_map(|l| l.split_once(' '))
                    .map(|(t, s)| (t.to_string(), s.to_string()))
                    .collect()
            })
            .unwrap_or_default()
    }
}

fn parse_created(out: &str) -> Created {
    let mut parts = out.split_whitespace();
    Created {
        pane_id: parts.next().unwrap_or_default().to_string(),
        window_index: parts.next().and_then(|i| i.parse().ok()).unwrap_or(0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn targets_are_exact_match() {
        assert_eq!(session_target("api/feat/x"), "=api/feat/x:");
        assert_eq!(window_target("api", 2), "=api:2");
    }

    #[test]
    fn parses_created_output() {
        assert_eq!(
            parse_created("%12 3\n"),
            Created {
                pane_id: "%12".into(),
                window_index: 3
            }
        );
    }
}

/// The tty with the newest activity in `list-clients` output (`<activity> <tty>` lines).
fn latest_of(out: &str) -> Option<String> {
    out.lines()
        .filter_map(|l| l.split_once(' '))
        .max_by_key(|(at, _)| at.parse::<u64>().unwrap_or(0))
        .map(|(_, tty)| tty.to_string())
}

#[cfg(test)]
mod latest_client_tests {
    #[test]
    fn the_most_recently_used_client_wins() {
        let out = "1794427493 /dev/pts/1\n1794428455 /dev/pts/22\n1794428001 /dev/pts/5\n";
        assert_eq!(super::latest_of(out).as_deref(), Some("/dev/pts/22"));
        assert_eq!(super::latest_of(""), None);
    }
}
