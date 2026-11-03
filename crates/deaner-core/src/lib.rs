//! Pure building blocks for deaner: data model, configuration, persisted state,
//! parsers for git/gh/tmux output, sidebar ordering, and snapshot logic.
//!
//! Apart from [`process`] (a subprocess-with-deadline helper shared by the tmux
//! and git crates), nothing here spawns processes or touches the filesystem, so
//! almost all of it is unit-testable.

pub mod agent;
pub mod config;
pub mod git;
pub mod names;
pub mod pane;
pub mod pr;
pub mod process;
pub mod sidebar;
pub mod snapshot;
pub mod state;
pub mod tmux_keys;

pub use agent::{AgentState, HookEvent, HookPayload};
pub use config::{AgentDef, Config, RepoConfig, TabKind, TabSpec};
pub use git::{GitStatus, Worktree};
pub use pane::PaneInfo;
pub use pr::{BranchStatus, Checks, PrState, PrStatus};
pub use sidebar::{Row, WorkspaceView};
pub use snapshot::Snapshot;
pub use state::{RepoEntry, State};
