//! Sidebar icons.
//!
//! The Nerd Font set sticks to Octicons and Font Awesome, whose codepoints are the
//! same in Nerd Fonts v2 and v3, so it works with older patched fonts too.
//! `icons = "unicode"` in config.toml selects the plain set for other fonts.

use the_deaner_core::config::IconStyle;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Icons {
    pub expanded: &'static str,
    pub collapsed: &'static str,
    pub pinned: &'static str,
    pub active: &'static str,
    pub inactive: &'static str,
    pub branch: &'static str,
    pub detached: &'static str,
    pub folder: &'static str,
    pub pull_request: &'static str,
    pub merged: &'static str,
    pub checks_passing: &'static str,
    pub checks_failing: &'static str,
    pub checks_pending: &'static str,
    pub changed: &'static str,
    pub awaiting: &'static str,
    pub idle: &'static str,
    pub compacting: &'static str,
    pub parked: &'static str,
}

pub const NERD: Icons = Icons {
    expanded: "\u{f47c}",       // oct chevron-down
    collapsed: "\u{f460}",      // oct chevron-right
    pinned: "\u{f435}",         // oct pin
    active: "\u{f111}",         // fa circle
    inactive: "\u{f10c}",       // fa circle-o
    branch: "\u{f418}",         // oct git-branch
    detached: "\u{f417}",       // oct git-commit
    folder: "\u{f413}",         // oct file-directory
    pull_request: "\u{f407}",   // oct git-pull-request
    merged: "\u{f419}",         // oct git-merge
    checks_passing: "\u{f42e}", // oct check
    checks_failing: "\u{f467}", // oct x
    checks_pending: "\u{f43a}", // oct clock
    changed: "\u{f448}",        // oct pencil
    awaiting: "\u{f0f3}",       // fa bell
    idle: "✻",
    compacting: "◐",
    parked: "\u{f04c}", // fa pause
};

pub const UNICODE: Icons = Icons {
    expanded: "▾",
    collapsed: "▸",
    pinned: "★",
    active: "●",
    inactive: "○",
    branch: "⎇",
    detached: "◇",
    folder: "▫",
    pull_request: "⇄",
    merged: "⇉",
    checks_passing: "✓",
    checks_failing: "✗",
    checks_pending: "…",
    changed: "✎",
    awaiting: "!",
    idle: "✻",
    compacting: "◐",
    parked: "‖",
};

#[must_use]
pub const fn for_style(style: IconStyle) -> &'static Icons {
    match style {
        IconStyle::Nerd => &NERD,
        IconStyle::Unicode => &UNICODE,
    }
}
