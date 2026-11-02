//! Colors come from the terminal. Everything is drawn with its default background
//! and the 16 ANSI colors, so the UI follows whatever theme the terminal has
//! (and changes with it). A fixed RGB value would stop matching some theme.

use ratatui::style::Color;
use ratcn::Theme;

/// ratcn's terminal theme, minus the fixed dark fills it uses for fields and surfaces.
#[must_use]
pub fn terminal() -> Theme {
    let mut theme = Theme::terminal();
    theme.surface = Color::Reset;
    theme.field = Color::Reset;
    theme.secondary = Color::Reset;
    theme.ring = Color::White;
    theme
}
