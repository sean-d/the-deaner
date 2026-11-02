//! `deaner browse`: pick a project from the filesystem, add it, and open it.

use crate::engine::Engine;
use crate::frame::Switcher;
use crate::paths::expand;
use anyhow::{Context, Result};
use deaner_ui::browser::{Browser, BrowserAction, DirItem};
use ratatui::crossterm::event::{self, Event};
use std::path::{Path, PathBuf};

/// The folders inside `dir`, following symlinks. Unreadable entries are skipped.
#[must_use]
pub fn list_dirs(dir: &Path, added: &[PathBuf]) -> Vec<DirItem> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .filter_map(Result::ok)
        .filter(|e| e.path().is_dir())
        .map(|e| {
            let path = e.path();
            DirItem {
                name: e.file_name().to_string_lossy().into_owned(),
                is_repo: path.join(".git").exists(),
                added: added.contains(&path),
                path,
            }
        })
        .collect()
}

/// Where to start: the folder holding the most recently added project, or home.
#[must_use]
pub fn start_dir(engine: &Engine) -> PathBuf {
    engine
        .state()
        .repos
        .last()
        .and_then(|r| r.path.parent().map(Path::to_path_buf))
        .filter(|p| p.is_dir())
        .or_else(dirs::home_dir)
        .unwrap_or_else(|| PathBuf::from("/"))
}

/// Add `path` if needed, make sure it has a session, and return that session's name.
///
/// A linked worktree opens as itself, under its main repo.
///
/// # Errors
/// When the path can't be added or the session can't start.
pub fn open_project(engine: &Engine, path: &Path) -> Result<String> {
    let repo = engine.add_repo(&path.to_string_lossy())?;
    let picked = expand(&path.to_string_lossy());
    let views = engine.workspaces();
    let ws = views
        .iter()
        .find(|w| w.path == picked)
        .or_else(|| views.iter().find(|w| w.path == repo.path))
        .with_context(|| format!("{} didn't show up as a workspace", repo.path.display()))?;
    engine.ensure_session(ws)
}

/// Run until something is picked or the user closes it.
///
/// # Errors
/// When the terminal can't be set up.
pub fn run(engine: &Engine, start: Option<&str>) -> Result<()> {
    let start = start.map_or_else(|| start_dir(engine), expand);
    let added: Vec<PathBuf> = engine.state().repos.into_iter().map(|r| r.path).collect();
    let mut browser = Browser::new(
        start,
        dirs::home_dir(),
        deaner_ui::icons::for_style(engine.config.icons),
        Box::new(move |dir| dir.is_dir().then(|| list_dirs(dir, &added))),
    );
    let switcher = Switcher::detect(engine);
    let mut hint = None;
    let mut terminal = ratatui::init();
    let result = (|| -> Result<()> {
        loop {
            terminal.draw(|f| browser.draw(f))?;
            let Event::Key(key) = event::read()? else {
                continue;
            };
            match browser.handle_key(key) {
                BrowserAction::None => {}
                BrowserAction::Close => return Ok(()),
                BrowserAction::Pick(path) => {
                    let opened =
                        open_project(engine, &path).and_then(|name| switcher.show(engine, &name));
                    match opened {
                        Ok(h) => {
                            hint = h;
                            return Ok(());
                        }
                        // Stay open so another folder can be picked.
                        Err(e) => browser.set_message(format!("{e:#}")),
                    }
                }
            }
        }
    })();
    ratatui::restore();
    if let Some(h) = hint {
        println!("{h}");
    }
    result
}
