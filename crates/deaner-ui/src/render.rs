//! Drawing the sidebar.

use crate::app::{App, Mode, PromptKind};
use crate::keymap::{BINDINGS, keys_label, sections};
use deaner_core::AgentState;
use deaner_core::pr::{Checks, PrState};
use deaner_core::sidebar::{RepoGroup, Row, WorkspaceView, WsStatus};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, List, ListItem, Paragraph, Wrap};
use ratcn::{InputWidget, ToastPosition, ToasterWidget};
use unicode_width::UnicodeWidthStr;

const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

struct Palette {
    muted: Style,
    accent: Style,
    warn: Style,
    bad: Style,
    good: Style,
    busy: Style,
    border: Style,
}

fn palette(app: &App) -> Palette {
    let t = &app.theme;
    Palette {
        muted: Style::new().fg(t.muted_foreground),
        accent: Style::new().fg(t.accent),
        warn: Style::new().fg(t.warning),
        bad: Style::new().fg(t.destructive),
        good: Style::new().fg(Color::Green),
        busy: Style::new().fg(Color::Cyan),
        border: Style::new().fg(t.border),
    }
}

/// Draw the whole sidebar into `frame`.
pub fn draw(frame: &mut Frame<'_>, app: &mut App) {
    let area = frame.area();
    let p = palette(app);
    let [header, list_area, footer] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .areas(area);

    frame.render_widget(header_line(app, &p, header.width), header);

    app.list_area = list_area;
    let items: Vec<ListItem<'_>> = app
        .rows
        .iter()
        .map(|row| ListItem::new(row_line(app, &p, *row, list_area.width)))
        .collect();
    if items.is_empty() {
        let hint = Paragraph::new(vec![
            Line::raw(""),
            Line::styled(" No repos yet.", p.muted),
            Line::styled(" Press a to add one,", p.muted),
            Line::styled(" or run `deaner add <path>`.", p.muted),
        ])
        .wrap(Wrap { trim: false });
        frame.render_widget(hint, list_area);
    } else {
        let list = List::new(items).highlight_style(Style::new().add_modifier(Modifier::REVERSED));
        frame.render_stateful_widget(list, list_area, &mut app.list);
    }

    draw_footer(frame, app, &p, footer);

    match &app.mode {
        Mode::Normal | Mode::Filter(_) => {}
        Mode::Help(scroll) => draw_help(frame, &p, area, *scroll),
        Mode::Prompt { kind, input } => {
            let (title, placeholder) = match kind {
                PromptKind::NewWorktree { repo_name, .. } => {
                    (format!(" New worktree · {repo_name} "), "branch name")
                }
                PromptKind::AddRepo => (" Add repo or folder ".to_string(), "~/dev/project"),
            };
            let rect = popup(area, 5);
            let block = dialog_block(&p, &title);
            let inner = block.inner(rect);
            frame.render_widget(Clear, rect);
            frame.render_widget(block, rect);
            let [field, hint] = Layout::vertical([Constraint::Length(2), Constraint::Length(1)])
                .areas(inner.inner(ratatui::layout::Margin::new(1, 0)));
            frame.render_widget(
                InputWidget::new(input)
                    .themed(&app.theme)
                    .focused(true)
                    .placeholder(placeholder),
                field,
            );
            frame.render_widget(Line::styled("Enter ok · Esc cancel", p.muted), hint);
        }
        Mode::Picker { ws, choices, index } => {
            let height = u16::try_from(choices.len())
                .unwrap_or(u16::MAX)
                .saturating_add(2)
                .min(area.height);
            let rect = popup(area, height);
            let block = dialog_block(&p, &format!(" New tab · {} ", ws.name));
            let inner = block.inner(rect);
            frame.render_widget(Clear, rect);
            frame.render_widget(block, rect);
            let lines: Vec<Line<'_>> = choices
                .iter()
                .enumerate()
                .map(|(i, c)| {
                    let number = if i < 9 {
                        format!("{} ", i + 1)
                    } else {
                        "  ".into()
                    };
                    let cmd = c.command.as_deref().unwrap_or("shell");
                    let mut line = Line::from(vec![
                        Span::styled(number, p.muted),
                        Span::raw(format!("{:<8} ", c.name)),
                        Span::styled(cmd.to_string(), p.muted),
                    ]);
                    if i == *index {
                        line = line.style(Style::new().add_modifier(Modifier::REVERSED));
                    }
                    line
                })
                .collect();
            frame.render_widget(Paragraph::new(lines), inner);
        }
        Mode::Confirm { message, .. } => {
            let text_width = area.width.saturating_sub(4).max(1);
            let lines =
                u16::try_from(message.width().div_ceil(usize::from(text_width))).unwrap_or(1);
            let rect = popup(area, lines + 4);
            let block = dialog_block(&p, " Confirm ");
            let inner = block.inner(rect);
            frame.render_widget(Clear, rect);
            frame.render_widget(block, rect);
            let para = Paragraph::new(vec![
                Line::raw(message.clone()),
                Line::raw(""),
                Line::from(vec![
                    Span::styled("y", p.accent),
                    Span::raw(" yes · "),
                    Span::styled("n", p.accent),
                    Span::raw(" no"),
                ]),
            ])
            .wrap(Wrap { trim: true });
            frame.render_widget(para, inner.inner(ratatui::layout::Margin::new(1, 0)));
        }
    }

    if !app.toasts.is_empty() {
        let toaster = ToasterWidget::new(&app.toasts, app.clock)
            .themed(&app.theme)
            .position(ToastPosition::BottomCenter)
            .toast_width(area.width.saturating_sub(2).max(10))
            .max_visible_toasts(3)
            .inset(1, 1);
        frame.render_widget(toaster, area);
    }
}

fn popup(area: Rect, height: u16) -> Rect {
    let height = height.min(area.height);
    let y = area.y + area.height.saturating_sub(height) / 3;
    Rect::new(area.x, y, area.width, height)
}

fn dialog_block<'a>(p: &Palette, title: &str) -> Block<'a> {
    Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(p.border)
        .title(Span::styled(title.to_string(), p.accent))
}

fn header_line<'a>(app: &App, p: &Palette, width: u16) -> Line<'a> {
    let all = || app.groups.iter().flat_map(|g| g.workspaces.iter());
    let awaiting = all().filter(|w| w.agent == AgentState::Awaiting).count();
    let busy = all().filter(|w| w.agent == AgentState::Busy).count();
    let mut right: Vec<Span<'a>> = Vec::new();
    if awaiting > 0 {
        right.push(Span::styled(
            format!("!{awaiting} "),
            p.warn.add_modifier(Modifier::BOLD),
        ));
    }
    if busy > 0 {
        right.push(Span::styled(
            format!("{}{busy} ", SPINNER[(app.tick as usize) % SPINNER.len()]),
            p.busy,
        ));
    }
    let left = vec![Span::styled(
        " deaner",
        Style::new().add_modifier(Modifier::BOLD),
    )];
    justify(left, right, width)
}

fn draw_footer(frame: &mut Frame<'_>, app: &App, p: &Palette, area: Rect) {
    match &app.mode {
        Mode::Filter(input) => {
            frame.render_widget(
                InputWidget::new(input)
                    .themed(&app.theme)
                    .focused(true)
                    .prefix(Span::styled("/", p.accent)),
                area,
            );
        }
        _ if !app.filter.is_empty() => {
            frame.render_widget(
                Line::from(vec![
                    Span::styled(" /", p.accent),
                    Span::raw(app.filter.clone()),
                    Span::styled("  Esc clears", p.muted),
                ]),
                area,
            );
        }
        _ => frame.render_widget(Line::styled(" ? help", p.muted), area),
    }
}

fn row_line<'a>(app: &App, p: &Palette, row: Row, width: u16) -> Line<'a> {
    match row {
        Row::Repo { group } => repo_line(app, p, &app.groups[group], width),
        Row::Workspace { group, ws } => ws_line(app, p, &app.groups[group].workspaces[ws], width),
    }
}

fn repo_line<'a>(app: &App, p: &Palette, g: &RepoGroup, width: u16) -> Line<'a> {
    let caret = if g.collapsed && app.filter.is_empty() {
        "▸"
    } else {
        "▾"
    };
    let mut left = vec![Span::styled(format!("{caret} "), p.muted)];
    if g.pinned {
        left.push(Span::styled("★ ", p.warn));
    }
    left.push(Span::styled(
        g.name.clone(),
        Style::new().add_modifier(Modifier::BOLD),
    ));
    let mut right = Vec::new();
    if g.collapsed
        && let Some(glyph) = agent_glyph(app, p, g.agent(), WsStatus::Live)
    {
        right.push(glyph);
        right.push(Span::raw(" "));
    }
    justify(left, right, width)
}

fn agent_glyph<'a>(
    app: &App,
    p: &Palette,
    state: AgentState,
    status: WsStatus,
) -> Option<Span<'a>> {
    Some(match (status, state) {
        (WsStatus::Parked, _) => Span::styled("◌", p.muted),
        (WsStatus::Stopped, _) => Span::styled("·", p.muted),
        (_, AgentState::Awaiting) => Span::styled("!", p.warn.add_modifier(Modifier::BOLD)),
        (_, AgentState::Busy) => Span::styled(SPINNER[(app.tick as usize) % SPINNER.len()], p.busy),
        (_, AgentState::Compacting) => Span::styled("◐", p.busy),
        (_, AgentState::Idle) => Span::styled("○", p.good),
        (_, AgentState::None) => return None,
    })
}

fn ws_line<'a>(app: &App, p: &Palette, w: &WorkspaceView, width: u16) -> Line<'a> {
    let glyph =
        agent_glyph(app, p, w.agent, w.status).unwrap_or_else(|| Span::styled("•", p.muted));
    let label = w.branch.clone().unwrap_or_else(|| w.name.clone());
    let mut label_style = match w.status {
        WsStatus::Live => Style::new(),
        WsStatus::Parked | WsStatus::Stopped => p.muted,
    };
    if w.current {
        label_style = label_style.patch(p.accent).add_modifier(Modifier::BOLD);
    }
    let mut left = vec![
        Span::raw("  "),
        glyph,
        Span::raw(" "),
        Span::styled(label, label_style),
    ];
    if w.status == WsStatus::Parked {
        left.push(Span::styled(" parked", p.muted));
    }

    let mut right: Vec<Span<'a>> = Vec::new();
    if let Some(git) = &w.git {
        if !git.is_clean() {
            right.push(Span::styled(format!("✎{}", git.changed()), p.warn));
        }
        if git.ahead > 0 {
            right.push(Span::styled(format!("↑{}", git.ahead), p.muted));
        }
        if git.behind > 0 {
            right.push(Span::styled(format!("↓{}", git.behind), p.muted));
        }
    }
    if let Some(pr) = &w.pr {
        let style = match (pr.state, pr.checks) {
            (PrState::Merged, _) => p.accent,
            (PrState::Closed | PrState::Draft, _) | (_, Checks::None) => p.muted,
            (_, Checks::Passing) => p.good,
            (_, Checks::Failing) => p.bad,
            (_, Checks::Pending) => p.warn,
        };
        right.push(Span::styled(format!("#{}", pr.number), style));
    }
    let mut spaced = Vec::new();
    for (i, s) in right.into_iter().enumerate() {
        if i > 0 {
            spaced.push(Span::raw(" "));
        }
        spaced.push(s);
    }
    if !spaced.is_empty() {
        spaced.push(Span::raw(" "));
    }
    justify(left, spaced, width)
}

/// Lay out `left` and `right` on one line of `width`, truncating the left side with `…`.
fn justify<'a>(left: Vec<Span<'a>>, right: Vec<Span<'a>>, width: u16) -> Line<'a> {
    let width = usize::from(width);
    let right_w: usize = right.iter().map(|s| s.content.width()).sum();
    let budget = width.saturating_sub(right_w + 1);
    let mut used = 0;
    let mut spans = Vec::new();
    for s in left {
        let w = s.content.width();
        if used + w <= budget {
            used += w;
            spans.push(s);
            continue;
        }
        let mut cut = String::new();
        for ch in s.content.chars() {
            let cw = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
            if used + cw + 1 > budget {
                break;
            }
            used += cw;
            cut.push(ch);
        }
        cut.push('…');
        used += 1;
        spans.push(Span::styled(cut, s.style));
        break;
    }
    let pad = width.saturating_sub(used + right_w);
    spans.push(Span::raw(" ".repeat(pad)));
    spans.extend(right);
    Line::from(spans)
}

fn draw_help(frame: &mut Frame<'_>, p: &Palette, area: Rect, scroll: u16) {
    let mut lines: Vec<Line<'_>> = Vec::new();
    for section in sections() {
        if !lines.is_empty() {
            lines.push(Line::raw(""));
        }
        lines.push(Line::styled(section, p.accent.add_modifier(Modifier::BOLD)));
        for b in BINDINGS.iter().filter(|b| b.section == section) {
            lines.push(Line::from(vec![
                Span::styled(
                    format!("{:<9}", keys_label(b)),
                    Style::new().add_modifier(Modifier::BOLD),
                ),
                Span::raw(b.help),
            ]));
        }
    }
    lines.push(Line::raw(""));
    lines.push(Line::styled("j/k scroll · any other key closes", p.muted));
    let block = dialog_block(p, " Keys ");
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .scroll((scroll, 0))
            .block(block),
        area,
    );
}
