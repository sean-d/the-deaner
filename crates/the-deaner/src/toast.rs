//! `the-deaner toast`: bread goes in, toast comes out. `b` in the sidebar opens it in a popup.

use anyhow::Result;
use ratatui::crossterm::{
    cursor, event, execute, queue,
    style::Print,
    terminal::{self, Clear, ClearType},
};
use std::io::{Write, stdout};
use std::time::Duration;

type Rgb = (u8, u8, u8);

const BREAD: Rgb = (240, 218, 166);
const TOAST: Rgb = (184, 112, 52);
const CRUST: Rgb = (116, 66, 28);
const STEEL: Rgb = (196, 202, 212);
const DARK: Rgb = (120, 126, 140);
const GLOW: [Rgb; 2] = [(255, 128, 32), (255, 186, 64)];
const WORDS: Rgb = (232, 220, 192);

const W: usize = 36;
const H: usize = 14;
/// Where the toaster's top edge sits; the slots are cut into it.
const TOP: usize = 7;
/// Left edge of each slot's opening (8 wide).
const SLOTS: [usize; 2] = [8, 21];

/// A tiny character canvas with a color per cell.
struct Canvas {
    cells: Vec<Vec<(char, Option<Rgb>)>>,
}

impl Canvas {
    fn new() -> Self {
        Self {
            cells: vec![vec![(' ', None); W]; H],
        }
    }

    fn put(&mut self, row: usize, col: usize, text: &str, color: Rgb) {
        for (i, ch) in text.chars().enumerate() {
            if let Some(cell) = self.cells.get_mut(row).and_then(|r| r.get_mut(col + i)) {
                *cell = (ch, Some(color));
            }
        }
    }

    fn lines(&self) -> Vec<String> {
        self.cells
            .iter()
            .map(|row| {
                let mut out = String::new();
                for (ch, color) in row {
                    match color {
                        Some((r, g, b)) => out.push_str(&format!("\x1b[38;2;{r};{g};{b}m{ch}")),
                        None => out.push(*ch),
                    }
                }
                out.push_str("\x1b[0m");
                out
            })
            .collect()
    }
}

/// One frame: slices `rise` rows above the slots (0 = all the way in), how done they are,
/// whether the lever is down, and whether the slots glow.
struct Frame {
    rise: usize,
    toasted: bool,
    lever_down: bool,
    glow: Option<usize>,
    crumbs: bool,
    caption: &'static str,
}

fn draw(f: &Frame) -> Vec<String> {
    let mut c = Canvas::new();
    // Slices are 4 rows (a crusty top, then bread) with their top `rise` rows above the slot;
    // whatever is lower is inside the toaster.
    let inside = if f.toasted { TOAST } else { BREAD };
    for &slot in &SLOTS {
        for i in 0..4 {
            let Some(row) = (TOP + i).checked_sub(f.rise) else {
                continue;
            };
            if row >= TOP {
                break;
            }
            if i == 0 {
                c.put(row, slot, ".~~~~~~.", CRUST);
            } else {
                c.put(row, slot, "|", CRUST);
                c.put(row, slot + 1, "::::::", inside);
                c.put(row, slot + 7, "|", CRUST);
            }
        }
    }
    if f.crumbs {
        c.put(1, 4, "'", TOAST);
        c.put(0, 17, ".", TOAST);
        c.put(2, 30, "'", TOAST);
        c.put(0, 28, "*ding*", WORDS);
    }
    // The toaster: a top edge with two slots cut in, a body, a lever, feet.
    c.put(TOP, 3, ".---[        ]---[        ]---.", STEEL);
    for &slot in &SLOTS {
        let (fill, color) = match f.glow {
            Some(i) => ("~~~~~~~~", GLOW[i % 2]),
            None => ("________", DARK),
        };
        c.put(TOP, slot, fill, color);
    }
    for row in TOP + 1..TOP + 5 {
        c.put(row, 3, "|", STEEL);
        c.put(row, 33, "|", STEEL);
    }
    c.put(TOP + 2, 11, "T O A S T E R", DARK);
    let lever = if f.lever_down { TOP + 3 } else { TOP + 1 };
    c.put(lever, 34, "=o", STEEL);
    c.put(TOP + 5, 3, "'-----------------------------'", STEEL);
    c.put(TOP + 6, 6, "()", DARK);
    c.put(TOP + 6, 28, "()", DARK);
    let mut lines = c.lines();
    lines.push(String::new());
    lines.push(format!(
        "\x1b[38;2;{};{};{}m{:^W$}\x1b[0m",
        WORDS.0, WORDS.1, WORDS.2, f.caption
    ));
    lines
}

fn frames() -> Vec<(Frame, u64)> {
    let f = |rise, toasted, lever_down, glow, crumbs, caption| Frame {
        rise,
        toasted,
        lever_down,
        glow,
        crumbs,
        caption,
    };
    vec![
        (f(4, false, false, None, false, "bread"), 700),
        (f(2, false, false, None, false, "in it goes"), 350),
        (f(0, false, true, None, false, "*clunk*"), 500),
        (f(0, false, true, Some(0), false, "toasting ."), 400),
        (f(0, false, true, Some(1), false, "toasting . ."), 400),
        (f(0, false, true, Some(0), false, "toasting . . ."), 400),
        (f(0, false, true, Some(1), false, "toasting . . . ."), 400),
        (f(6, true, false, None, true, "just give'r"), 0),
    ]
}

/// Play the toaster, then wait for any key.
///
/// # Errors
/// When the terminal can't be drawn to.
pub fn run() -> Result<()> {
    let mut out = stdout();
    terminal::enable_raw_mode()?;
    execute!(out, cursor::Hide)?;
    let result = (|| -> Result<()> {
        for (frame, millis) in frames() {
            let (cols, rows) = terminal::size().unwrap_or((W as u16, H as u16 + 2));
            let lines = draw(&frame);
            let left = usize::from(cols).saturating_sub(W) / 2;
            let top = usize::from(rows).saturating_sub(lines.len()) / 2;
            queue!(out, Clear(ClearType::All))?;
            for (i, line) in lines.iter().enumerate() {
                let y = u16::try_from(top + i).unwrap_or(0);
                let x = u16::try_from(left).unwrap_or(0);
                queue!(out, cursor::MoveTo(x, y), Print(line))?;
            }
            out.flush()?;
            if millis > 0 {
                // A key skips ahead to the toast.
                if event::poll(Duration::from_millis(millis))? {
                    let _ = event::read();
                }
            }
        }
        // Wait for a key before closing.
        loop {
            if let event::Event::Key(_) = event::read()? {
                return Ok(());
            }
        }
    })();
    let _ = execute!(out, cursor::Show);
    let _ = terminal::disable_raw_mode();
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(lines: &[String]) -> String {
        let mut s = lines.join("\n");
        while let Some(start) = s.find('\x1b') {
            let end = s[start..].find('m').map_or(s.len(), |e| start + e + 1);
            s.replace_range(start..end, "");
        }
        s
    }

    #[test]
    fn bread_goes_in_and_toast_comes_out() {
        let all = frames();
        let first = plain(&draw(&all[0].0));
        let last = plain(&draw(&all.last().unwrap().0));
        assert!(
            first.contains("|::::::|") && first.contains("T O A S T E R"),
            "{first}"
        );
        assert!(
            last.contains("*ding*") && last.contains(".~~~~~~."),
            "{last}"
        );
        // While toasting, nothing sticks out of the slots.
        let toasting = plain(&draw(&all[3].0));
        assert!(
            !toasting.contains("::::::") && toasting.contains("~~~~~~~~"),
            "{toasting}"
        );
        // Every frame fits the canvas.
        for (frame, _) in &all {
            for line in plain(&draw(frame)).lines() {
                assert!(line.chars().count() <= W, "{line}");
            }
        }
    }
}
