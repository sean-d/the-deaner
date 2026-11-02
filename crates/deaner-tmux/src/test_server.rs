//! Isolated tmux servers for tests.

use crate::Tmux;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static COUNTER: AtomicU32 = AtomicU32::new(0);

/// A private tmux server on its own socket, killed on drop.
///
/// It loads a minimal config (`/bin/sh` panes, no user `~/.tmux.conf`) so
/// tests behave the same on every machine.
pub struct TestServer {
    pub tmux: Tmux,
    pub socket: String,
    config: PathBuf,
}

impl TestServer {
    /// # Panics
    /// If the temporary config file cannot be written.
    #[must_use]
    pub fn new() -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or_default();
        let socket = format!(
            "deaner-test-{}-{}-{nanos}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        );
        let config = std::env::temp_dir().join(format!("{socket}.conf"));
        std::fs::write(
            &config,
            "set -g default-shell /bin/sh\nset -g default-command /bin/sh\nset -g base-index 0\nset -g history-limit 2000\n",
        )
        .expect("write test tmux config");
        Self {
            tmux: Tmux::named(socket.clone()).with_config(&config),
            socket,
            config,
        }
    }
}

impl Default for TestServer {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.tmux.kill_server();
        let _ = std::fs::remove_file(&self.config);
    }
}
