//! `the-deaner cheatsheet`: the searchable shortcut popup.

use crate::engine::Engine;
use anyhow::Result;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind};
use the_deaner_core::tmux_keys::{TmuxShortcut, parse_list_keys, shortcuts};
use the_deaner_ui::cheatsheet::{Cheatsheet, entries};
use the_deaner_ui::guide::{Guide, GuideAction, GuidePage};

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

/// The docs, built into the binary: the same markdown files as on GitHub.
#[must_use]
pub fn guide_pages() -> Vec<GuidePage> {
    macro_rules! page {
        ($slug:literal, $title:literal, $file:literal) => {
            GuidePage {
                slug: $slug,
                title: $title,
                source: include_str!(concat!("../../../", $file)),
            }
        };
    }
    vec![
        page!("overview", "Overview", "README.md"),
        page!(
            "getting-started",
            "Getting started",
            "docs/getting-started.md"
        ),
        page!("workflows", "Workflows", "docs/workflows.md"),
        page!("keybindings", "Keybindings", "docs/keybindings.md"),
        page!("configuration", "Configuration", "docs/configuration.md"),
        page!("agents", "Agents and hooks", "docs/agents.md"),
        page!("cli", "CLI reference", "docs/cli.md"),
        page!("architecture", "Architecture", "docs/architecture.md"),
        page!("development", "Development", "docs/development.md"),
    ]
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Showing {
    Keys,
    Guide,
}

/// Run until the user closes it.
///
/// # Errors
/// When the terminal can't be set up.
pub fn run(engine: &Engine) -> Result<()> {
    run_from(engine, Showing::Keys, None)
}

/// `the-deaner guide [page]`: the same popup, opened on the guide.
///
/// # Errors
/// When the page doesn't exist or the terminal can't be set up.
pub fn run_guide(engine: &Engine, page: Option<&str>) -> Result<()> {
    run_from(engine, Showing::Guide, page)
}

fn run_from(engine: &Engine, start: Showing, page: Option<&str>) -> Result<()> {
    let mut guide = Guide::new(guide_pages()).with_version(crate::upgrade::CURRENT);
    if let Some(name) = page
        && !guide.open(name)
    {
        anyhow::bail!(
            "no guide page `{name}`; pages: {}",
            guide.slugs().join(", ")
        );
    }
    let icons = the_deaner_ui::icons::for_style(engine.config.icons);
    let mut sheet = Cheatsheet::new(entries(
        &engine.config.keys,
        &terminal_shortcuts(engine),
        icons,
    ))
    .with_version(crate::upgrade::CURRENT);
    let mut showing = start;
    let mut terminal = ratatui::init();
    let result = (|| -> Result<()> {
        loop {
            terminal.draw(|f| match showing {
                Showing::Keys => sheet.draw(f),
                Showing::Guide => guide.draw(f),
            })?;
            let Event::Key(key) = event::read()? else {
                continue;
            };
            match showing {
                Showing::Keys if key.code == KeyCode::Tab && key.kind != KeyEventKind::Release => {
                    showing = Showing::Guide;
                }
                Showing::Keys => {
                    if sheet.handle_key(key) {
                        return Ok(());
                    }
                }
                Showing::Guide => match guide.handle_key(key) {
                    GuideAction::Close => return Ok(()),
                    GuideAction::Keys => showing = Showing::Keys,
                    GuideAction::None => {}
                },
            }
        }
    })();
    ratatui::restore();
    result
}
