//! `deaner peek <address>`: the end of a tab's output, with its colors, in a pager
//! (`v` in the sidebar opens it in a popup). Scroll up for more; `q` closes.

use crate::engine::Engine;
use anyhow::{Context, Result};
use std::io::Write;
use std::process::{Command, Stdio};

/// How much history to include above what's on screen.
const HISTORY_LINES: &str = "-2000";

/// Show the tab at `addr` (`<session>:<tab>` or `<session>`, as `deaner ls` prints them).
///
/// # Errors
/// When the address doesn't match a running tab, or its output can't be read.
pub fn run(engine: &Engine, addr: &str) -> Result<()> {
    let pane = engine.resolve(addr)?;
    let text = engine
        .tmux
        .run(&[
            "capture-pane",
            "-p",
            "-e",
            "-J",
            "-S",
            HISTORY_LINES,
            "-t",
            &pane.pane_id,
        ])
        .with_context(|| format!("reading {}", pane.address()))?;
    let text = trim_trailing_blank_lines(&text);
    let title = format!("peek · {} · q closes", pane.address());

    let pager = Command::new("less")
        // Colors, start at the end, scroll with the wheel, no ~ below short output.
        .args(["-R", "+G", "--mouse", "-~", "-P", &prompt(&title)])
        .stdin(Stdio::piped())
        .spawn();
    let Ok(mut pager) = pager else {
        // No less: print it and wait for a key.
        print!("{text}\n\n{title} (press Enter)");
        std::io::stdout().flush()?;
        let _ = std::io::stdin().read_line(&mut String::new());
        return Ok(());
    };
    if let Some(mut stdin) = pager.stdin.take() {
        // A closed pipe just means the pager quit early.
        let _ = stdin.write_all(text.as_bytes());
    }
    pager.wait()?;
    Ok(())
}

/// Drop the empty lines below the last output (a TUI leaves the rest of its screen blank).
fn trim_trailing_blank_lines(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let keep = lines
        .iter()
        .rposition(|l| !strip_escapes(l).trim().is_empty())
        .map_or(0, |i| i + 1);
    let mut out = lines[..keep].join("\n");
    out.push('\n');
    out
}

/// Text without its color escape sequences (`ESC [ ... m`), to tell blank lines apart.
fn strip_escapes(line: &str) -> String {
    let mut out = String::new();
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            for c in chars.by_ref() {
                if c.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// A `less` prompt showing `text` as is: `?`, `:`, `.`, `%` and `\` mean something there.
fn prompt(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        if matches!(c, '?' | ':' | '.' | '%' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blank_lines_at_the_end_go_even_with_colors_on_them() {
        let text = "line one\n\x1b[32mdone\x1b[0m\n\n\x1b[0m   \n\n";
        assert_eq!(
            trim_trailing_blank_lines(text),
            "line one\n\x1b[32mdone\x1b[0m\n"
        );
        assert_eq!(trim_trailing_blank_lines("\n\n"), "\n");
    }

    #[test]
    fn the_prompt_shows_the_address_literally() {
        assert_eq!(
            prompt("peek · shop-api:claude · q closes"),
            "peek · shop-api\\:claude · q closes"
        );
        assert_eq!(prompt("50% done."), "50\\% done\\.");
    }
}
