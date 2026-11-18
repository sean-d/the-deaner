//! The new-tab picker on its own, for the frame's new-tab key (`Alt-n`): the same choices
//! as `t` in the sidebar, in a small popup over whatever you're doing. No IO.

use crate::theme::colors;
use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use the_deaner_core::TabSpec;

/// What the caller should do after a key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PickAction {
    None,
    Close,
    /// Open this choice (an index into the choices).
    Pick(usize),
}

pub struct TabPicker {
    /// The workspace the tab goes into, for the title.
    workspace: String,
    choices: Vec<TabSpec>,
    index: usize,
}

impl TabPicker {
    #[must_use]
    pub fn new(workspace: impl Into<String>, choices: Vec<TabSpec>) -> Self {
        Self {
            workspace: workspace.into(),
            choices,
            index: 0,
        }
    }

    #[must_use]
    pub fn choices(&self) -> &[TabSpec] {
        &self.choices
    }

    /// The rows the popup needs: title, gap, one per choice, gap, hint.
    #[must_use]
    pub fn height(&self) -> usize {
        self.choices.len() + 4
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> PickAction {
        if key.kind == KeyEventKind::Release {
            return PickAction::None;
        }
        let last = self.choices.len().saturating_sub(1);
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => return PickAction::Close,
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                return PickAction::Close;
            }
            KeyCode::Down | KeyCode::Char('j') => self.index = (self.index + 1).min(last),
            KeyCode::Up | KeyCode::Char('k') => self.index = self.index.saturating_sub(1),
            KeyCode::Enter if !self.choices.is_empty() => return PickAction::Pick(self.index),
            KeyCode::Char(c @ '1'..='9') => {
                let i = (c as usize) - ('1' as usize);
                if i < self.choices.len() {
                    return PickAction::Pick(i);
                }
            }
            _ => {}
        }
        PickAction::None
    }

    pub fn draw(&self, frame: &mut Frame<'_>) {
        let area = frame.area();
        frame.buffer_mut().set_style(area, crate::theme::base());
        let muted = Style::new().fg(colors::DIM);
        let mut lines = vec![
            Line::from(vec![
                Span::styled(
                    " New tab",
                    Style::new().fg(colors::GOLD).add_modifier(Modifier::BOLD),
                ),
                Span::styled(format!(" · {}", self.workspace), muted),
            ]),
            Line::raw(""),
        ];
        for (i, c) in self.choices.iter().enumerate() {
            let number = if i < 9 {
                format!(" {} ", i + 1)
            } else {
                "   ".into()
            };
            let cmd = c.command.as_deref().unwrap_or("shell");
            let mut line = Line::from(vec![
                Span::styled(number, muted),
                Span::raw(format!("{:<8} ", c.name)),
                Span::styled(cmd.to_string(), muted),
            ]);
            if i == self.index {
                line = line.style(Style::new().bg(colors::SEL));
            }
            lines.push(line);
        }
        lines.push(Line::raw(""));
        lines.push(Line::styled(" j/k · Enter or 1-9 · Esc", muted));
        frame.render_widget(Paragraph::new(lines), area);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    fn picker() -> TabPicker {
        TabPicker::new(
            "temp",
            vec![
                TabSpec::new("nvim", Some("nvim")),
                TabSpec::new("claude", Some("claude")),
                TabSpec::new("shell", None),
            ],
        )
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn keys_choose_and_close() {
        let mut p = picker();
        assert_eq!(p.handle_key(key(KeyCode::Enter)), PickAction::Pick(0));
        p.handle_key(key(KeyCode::Char('j')));
        p.handle_key(key(KeyCode::Down));
        p.handle_key(key(KeyCode::Down)); // stops at the last one
        assert_eq!(p.handle_key(key(KeyCode::Enter)), PickAction::Pick(2));
        p.handle_key(key(KeyCode::Char('k')));
        assert_eq!(p.handle_key(key(KeyCode::Enter)), PickAction::Pick(1));
        assert_eq!(p.handle_key(key(KeyCode::Char('3'))), PickAction::Pick(2));
        assert_eq!(p.handle_key(key(KeyCode::Char('9'))), PickAction::None);
        assert_eq!(p.handle_key(key(KeyCode::Esc)), PickAction::Close);
        assert_eq!(p.handle_key(key(KeyCode::Char('q'))), PickAction::Close);
    }

    #[test]
    fn draws_the_choices_in_the_palette() {
        let p = picker();
        let mut term = Terminal::new(TestBackend::new(30, p.height() as u16)).unwrap();
        term.draw(|f| p.draw(f)).unwrap();
        let screen = term.backend().to_string();
        for want in ["New tab · temp", "1 nvim", "2 claude", "3 shell", "Esc"] {
            assert!(screen.contains(want), "{want} in\n{screen}");
        }
        for cell in term.backend().buffer().content() {
            for c in [cell.fg, cell.bg] {
                assert!(colors::ALL.contains(&c), "{c:?} outside the palette");
            }
        }
    }
}
