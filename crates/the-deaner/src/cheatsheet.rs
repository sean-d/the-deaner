//! `the-deaner cheatsheet`: the searchable shortcut popup.

use crate::engine::Engine;
use anyhow::Result;
use ratatui::crossterm::event::{self, Event};
use the_deaner_core::tmux_keys::{TmuxShortcut, parse_list_keys, shortcuts};
use the_deaner_ui::cheatsheet::{Cheatsheet, entries};

/// The user's real terminal shortcuts. Empty only if tmux can't be asked at all.
///
/// With no server running, tmux starts one just for the command, loads the user's
/// config, and exits again. So both answers must come from one invocation: a second
/// call would start over, or find nothing.
#[must_use]
pub fn terminal_shortcuts(engine: &Engine) -> Vec<TmuxShortcut> {
    let Ok(out) = engine
        .tmux
        .run(&["list-keys", ";", "show-options", "-gv", "prefix"])
    else {
        return Vec::new();
    };
    let Some((keys, prefix)) = out.trim_end().rsplit_once('\n') else {
        return Vec::new();
    };
    shortcuts(&parse_list_keys(keys), prefix.trim())
}

/// Run until the user closes it.
///
/// # Errors
/// When the terminal can't be set up.
pub fn run(engine: &Engine) -> Result<()> {
    let icons = the_deaner_ui::icons::for_style(engine.config.icons);
    let mut sheet = Cheatsheet::new(entries(
        &engine.config.keys,
        &terminal_shortcuts(engine),
        icons,
    ))
    .with_version(crate::upgrade::CURRENT);
    let mut terminal = ratatui::init();
    let result = (|| -> Result<()> {
        loop {
            terminal.draw(|f| sheet.draw(f))?;
            if let Event::Key(key) = event::read()?
                && sheet.handle_key(key)
            {
                return Ok(());
            }
        }
    })();
    ratatui::restore();
    result
}
