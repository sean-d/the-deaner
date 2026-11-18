//! The new-tab picker on its own (`the-deaner tab` without a tab name, `Alt-n` in the
//! frame): drawing and keys are in [`the_deaner_ui::tabpicker`].

use anyhow::Result;
use ratatui::crossterm::event::{self, Event};
use the_deaner_core::TabSpec;
use the_deaner_ui::tabpicker::{PickAction, TabPicker};

/// Let the user pick one of `choices` for `workspace`; `None` when they close it.
///
/// # Errors
/// When the terminal can't be set up.
pub fn run(workspace: &str, choices: Vec<TabSpec>) -> Result<Option<TabSpec>> {
    let mut picker = TabPicker::new(workspace, choices);
    let mut terminal = ratatui::try_init()
        .map_err(|e| anyhow::anyhow!("can't show the list here ({e}); name the tab instead"))?;
    let result = (|| -> Result<Option<TabSpec>> {
        loop {
            terminal.draw(|f| picker.draw(f))?;
            if let Event::Key(key) = event::read()? {
                match picker.handle_key(key) {
                    PickAction::Close => return Ok(None),
                    PickAction::Pick(i) => return Ok(picker.choices().get(i).cloned()),
                    PickAction::None => {}
                }
            }
        }
    })();
    ratatui::restore();
    result
}
