//! Agent status without hooks: reads Claude Code's session files and fills in panes the
//! hooks said nothing about. The matching rules are in
//! [`the_deaner_core::claude_session`]; this is the IO.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;
use the_deaner_core::PaneInfo;
use the_deaner_core::claude_session::{self, ClaudeSession};
use the_deaner_core::process::output_with_timeout;

/// Where Claude keeps one file per running claude: `sessions/` beside its settings.
#[must_use]
pub fn sessions_dir() -> PathBuf {
    crate::hooks::claude_settings_path().with_file_name("sessions")
}

/// Every session file in `dir` that parses. A missing directory is just none.
#[must_use]
pub fn read_sessions(dir: &Path) -> Vec<ClaudeSession> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .filter_map(|p| std::fs::read_to_string(p).ok())
        .filter_map(|s| ClaudeSession::parse(&s))
        .collect()
}

/// The terminal each of `pids` runs on, for those still running: where its stdin points
/// on Linux (no process to spawn), `ps` elsewhere.
fn ttys(pids: &[u32]) -> HashMap<u32, String> {
    if Path::new("/proc/self/fd/0").exists() {
        return pids
            .iter()
            .filter_map(|&pid| {
                let tty = std::fs::read_link(format!("/proc/{pid}/fd/0")).ok()?;
                let tty = tty.to_str()?.strip_prefix("/dev/")?.to_string();
                Some((pid, tty))
            })
            .collect();
    }
    let list: Vec<String> = pids.iter().map(u32::to_string).collect();
    let Ok(out) = output_with_timeout(
        Command::new("ps").args(["-o", "pid=,tty=", "-p", &list.join(",")]),
        Duration::from_secs(2),
    ) else {
        return HashMap::new();
    };
    // Exits non-zero when some pid is gone; what it printed still counts.
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| {
            let mut f = l.split_whitespace();
            Some((f.next()?.parse().ok()?, f.next()?.to_string()))
        })
        .collect()
}

/// Fill in `panes` from Claude's session files where the hooks are silent or behind.
pub fn fill(panes: &mut [PaneInfo], shells: &[String]) {
    let sessions = read_sessions(&sessions_dir());
    claude_session::fill(panes, &sessions, shells, ttys);
}
