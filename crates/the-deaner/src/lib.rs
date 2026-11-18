//! the-deaner: a tmux-backed command center for running coding agents side by side.
//!
//! The binary is a thin wrapper around [`cli::run`]; everything is in this
//! library so integration tests can drive it against throwaway tmux servers.

pub mod browse;
pub mod changelog;
pub mod cheatsheet;
pub mod claude_status;
pub mod cli;
pub mod engine;
pub mod frame;
pub mod heat;
pub mod hooks;
pub mod paths;
pub mod peek;
pub mod poller;
pub mod projects;
pub mod reports;
pub mod sidebar;
pub mod tabpicker;
pub mod titles;
pub mod toast;
pub mod upgrade;
