//! deaner: a tmux-backed command center for running coding agents side by side.
//!
//! The binary is a thin wrapper around [`cli::run`]; everything is in this
//! library so integration tests can drive it against throwaway tmux servers.

pub mod browse;
pub mod cheatsheet;
pub mod cli;
pub mod engine;
pub mod frame;
pub mod hooks;
pub mod paths;
pub mod poller;
pub mod sidebar;
