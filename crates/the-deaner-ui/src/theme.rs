//! the-deaner's own colors. They're fixed, not taken from the terminal theme: the point is
//! that what needs you, what's open and what's running are obvious at a glance, the
//! same on every theme. The palette started from party-hud's (github.com/mangow314/party-hud).
//!
//! Only the background is left to the terminal. Everything drawn uses a color from
//! [`colors`], which a test enforces.

use ratatui::style::Color;
use ratcn::Theme;

/// The palette. Every color the sidebar and its popups draw comes from here.
pub mod colors {
    use ratatui::style::Color;

    /// Ordinary text: warm cream.
    pub const FG: Color = Color::Rgb(232, 220, 192);
    /// De-emphasized text: stopped checkouts, hints, inactive repos.
    pub const DIM: Color = Color::Rgb(138, 143, 168);
    /// The open project's bar and names, headings, dialog titles, changed files.
    pub const GOLD: Color = Color::Rgb(226, 178, 74);
    /// "This project needs you": an agent is waiting, or finished while you were elsewhere.
    pub const ATTENTION: Color = Color::Rgb(255, 153, 0);
    pub const GREEN: Color = Color::Rgb(143, 191, 106);
    pub const RED: Color = Color::Rgb(208, 96, 90);
    pub const BLUE: Color = Color::Rgb(127, 167, 232);
    /// An agent at work.
    pub const BUSY: Color = Color::Rgb(150, 170, 255);
    pub const CYAN: Color = Color::Rgb(140, 207, 224);
    /// Merged pull requests.
    pub const VIOLET: Color = Color::Rgb(183, 138, 208);
    /// Borders and rules.
    pub const LINE: Color = Color::Rgb(58, 66, 102);
    /// The cursor's row.
    pub const SEL: Color = Color::Rgb(42, 51, 82);
    /// Behind the open project: a dark gold.
    pub const VIEW: Color = Color::Rgb(78, 60, 24);
    /// Text drawn on a filled accent (toasts, buttons).
    pub const INK: Color = Color::Rgb(20, 22, 34);

    /// Everything above, for the test that keeps drawing inside the palette.
    pub const ALL: [Color; 15] = [
        FG,
        DIM,
        GOLD,
        ATTENTION,
        GREEN,
        RED,
        BLUE,
        BUSY,
        CYAN,
        VIOLET,
        LINE,
        SEL,
        VIEW,
        INK,
        Color::Reset,
    ];
}

pub use colors::ATTENTION;

/// Under everything: cream text on the terminal's background.
#[must_use]
pub fn base() -> ratatui::style::Style {
    ratatui::style::Style::new().fg(colors::FG)
}

/// The ratcn theme for widgets the-deaner doesn't draw itself (fields, toasts, dialogs),
/// in the same palette. Surfaces stay on the terminal's background.
#[must_use]
pub fn the_deaner() -> Theme {
    use colors::{DIM, FG, GOLD, INK, LINE, RED};
    let mut theme = Theme::terminal();
    theme.foreground = FG;
    theme.muted_foreground = DIM;
    theme.background = Color::Reset;
    theme.surface = Color::Reset;
    theme.field = Color::Reset;
    theme.primary = GOLD;
    theme.primary_foreground = INK;
    theme.secondary = Color::Reset;
    theme.secondary_foreground = FG;
    theme.accent = GOLD;
    theme.destructive = RED;
    theme.destructive_foreground = INK;
    theme.warning = GOLD;
    theme.border = LINE;
    theme.ring = GOLD;
    theme.cursor = GOLD;
    theme
}
