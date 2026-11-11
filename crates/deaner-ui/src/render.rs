//! Drawing the sidebar.

use crate::app::{App, Mode, PromptKind};
use crate::icons::Icons;
use crate::keymap::{BINDINGS, keys_label, sections};
use crate::theme::colors;
use deaner_core::pr::{Checks, PrState};
use deaner_core::sidebar::{RepoGroup, Row, Section, WorkspaceView, WsStatus};
use deaner_core::{AgentState, OpenStyle};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
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
    branch: Style,
    merged: Style,
    attention: Style,
    border: Style,
}

/// Selected row: a background, so each span keeps its own color.
const SELECTED: Style = Style::new().bg(colors::SEL);
/// Where the name sits in a workspace row's spans (see `ws_line`).
const WS_LABEL: usize = 3;

fn palette(_app: &App) -> Palette {
    Palette {
        muted: Style::new().fg(colors::DIM),
        accent: Style::new().fg(colors::GOLD),
        warn: Style::new().fg(colors::GOLD),
        bad: Style::new().fg(colors::RED),
        good: Style::new().fg(colors::GREEN),
        busy: Style::new().fg(colors::BUSY),
        branch: Style::new().fg(colors::BLUE),
        merged: Style::new().fg(colors::VIOLET),
        attention: Style::new().fg(colors::ATTENTION),
        border: Style::new().fg(colors::LINE),
    }
}

/// Draw the whole sidebar into `frame`.
pub fn draw(frame: &mut Frame<'_>, app: &mut App) {
    let full = frame.area();
    frame.buffer_mut().set_style(full, crate::theme::base());
    let area = frame.area();
    let p = palette(app);
    let [list_area, footer] =
        Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(area);

    app.list_area = list_area;
    let items: Vec<ListItem<'_>> = app
        .rows
        .iter()
        .enumerate()
        .map(|(i, row)| row_item(app, &p, *row, i, list_area.width))
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
        // Rows style themselves: the cursor and the open project look different.
        let list = List::new(items);
        frame.render_stateful_widget(list, list_area, &mut app.list);
    }

    draw_footer(frame, app, &p, footer);

    match &app.mode {
        Mode::Normal | Mode::Filter(_) => {}
        Mode::Help(scroll) => draw_help(frame, &p, app.icons, area, *scroll),
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
                        line = line.style(SELECTED);
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
        _ => frame.render_widget(status_line(app, p, area.width), area),
    }
}

/// Footer: the help hint, and how many agents are waiting on you or working.
fn status_line<'a>(app: &App, p: &Palette, width: u16) -> Line<'a> {
    let all = || app.groups.iter().flat_map(|g| g.workspaces.iter());
    let awaiting = all().filter(|w| w.agent == AgentState::Awaiting).count();
    let busy = all().filter(|w| w.agent == AgentState::Busy).count();
    let mut right: Vec<Span<'a>> = Vec::new();
    if awaiting > 0 {
        right.push(Span::styled(
            format!("{} {awaiting} ", app.icons.awaiting),
            p.attention.add_modifier(Modifier::BOLD),
        ));
    }
    if busy > 0 {
        right.push(Span::styled(
            format!("{} {busy} ", SPINNER[(app.tick as usize) % SPINNER.len()]),
            p.busy,
        ));
    }
    justify(vec![Span::styled(" ? help", p.muted)], right, width)
}

/// One row, styled. The project shown on the right stays marked while the cursor moves,
/// in one of two styles (`open_project` in the config):
///
/// - **Block**: a dark gold background with a bar down its left edge, over the repo row
///   and all its workspaces; names in gold.
/// - **Box**: a thin gold box around it, the repo name in the top border.
///
/// Either way the open workspace's name is bold gold, and the cursor is a navy row.
fn row_item<'a>(app: &App, p: &Palette, row: Row, index: usize, width: u16) -> ListItem<'a> {
    let selected = app.list.selected() == Some(index);
    let is_open = |group: usize| app.groups[group].workspaces.iter().any(|w| w.current);
    let (in_block, open) = match row {
        Row::Repo { group } => (is_open(group), false),
        Row::Workspace { group, ws } => (is_open(group), app.groups[group].workspaces[ws].current),
        Row::BoxEnd { .. } => (true, false),
        Row::Header { .. } | Row::Gap { .. } => (false, false),
    };
    let boxed = app.open_style == OpenStyle::Box;
    let gold = Style::new().fg(colors::GOLD);
    let bar = || Span::styled("▌", gold);
    let mut line = match row {
        Row::BoxEnd { .. } => Line::styled(
            format!("╰{}╯", "─".repeat(usize::from(width.saturating_sub(2)))),
            gold,
        ),
        Row::Repo { group } if in_block && boxed => box_top(app, p, &app.groups[group], width),
        Row::Repo { .. } if in_block => {
            let mut line = row_line(app, p, row, width.saturating_sub(1));
            line.spans.insert(0, bar());
            line
        }
        Row::Workspace { .. } if in_block && boxed => {
            // The two spaces of indent become the box's left side; its right side closes it.
            let mut line = row_line(app, p, row, width.saturating_sub(1));
            line.spans[0] = Span::styled("│ ", gold);
            line.spans.push(Span::styled("│", gold));
            line
        }
        Row::Workspace { .. } if in_block => {
            // Workspace rows start with two spaces of indent: the bar takes the first.
            let mut line = row_line(app, p, row, width);
            line.spans[0] = Span::raw(" ");
            line.spans.insert(0, bar());
            line
        }
        _ => row_line(app, p, row, width),
    };
    if in_block && !boxed {
        // Names take the accent; orange still means "needs you".
        let name = match row {
            Row::Repo { group } => {
                let repo = &app.groups[group].name;
                line.spans.iter_mut().find(|s| s.content == repo.as_str())
            }
            _ => line.spans.get_mut(WS_LABEL + 1),
        };
        if let Some(name) = name
            && name.style.fg != Some(crate::theme::ATTENTION)
        {
            name.style = name.style.fg(colors::GOLD);
        }
    }
    if open {
        let at = if boxed { WS_LABEL } else { WS_LABEL + 1 };
        if let Some(name) = line.spans.get_mut(at) {
            if name.style.fg != Some(crate::theme::ATTENTION) {
                name.style = name.style.fg(colors::GOLD);
            }
            name.style = name.style.add_modifier(Modifier::BOLD);
        }
    }
    if selected {
        return ListItem::new(line).style(SELECTED);
    }
    if in_block && !boxed {
        return ListItem::new(line).style(Style::new().bg(colors::VIEW));
    }
    ListItem::new(line)
}

/// `╭ deaner ──────╮`: the open project's repo row, as the top of its box.
fn box_top<'a>(app: &App, p: &Palette, g: &RepoGroup, width: u16) -> Line<'a> {
    let gold = Style::new().fg(colors::GOLD);
    let attention = g.workspaces.iter().any(|w| app.needs_attention(w));
    let name_style = if attention { p.attention } else { gold };
    let mut spans = vec![Span::styled("╭ ", gold)];
    if g.collapsed && app.filter.is_empty() {
        spans.push(Span::styled(format!("{} ", app.icons.collapsed), p.muted));
    }
    spans.push(Span::styled(
        g.name.clone(),
        name_style.add_modifier(Modifier::BOLD),
    ));
    let used: usize = spans.iter().map(|s| s.content.width()).sum();
    let rule = usize::from(width).saturating_sub(used + 2);
    spans.push(Span::styled(format!(" {}╮", "─".repeat(rule)), gold));
    Line::from(spans)
}

fn row_line<'a>(app: &App, p: &Palette, row: Row, width: u16) -> Line<'a> {
    match row {
        Row::Gap { .. } => Line::raw(""),
        Row::Header { section } => heading_line(app, p, section, width),
        Row::Repo { group } => repo_line(app, p, &app.groups[group], width),
        Row::Workspace { group, ws } => ws_line(app, p, &app.groups[group].workspaces[ws], width),
        Row::BoxEnd { .. } => Line::raw(""),
    }
}

/// `● ACTIVE 5 ───────`: icon, name and how many repos, then a rule to the edge.
fn heading_line<'a>(app: &App, p: &Palette, section: Section, width: u16) -> Line<'a> {
    let (icon, style) = match section {
        Section::Pinned => (app.icons.pinned, p.warn),
        Section::Active => (app.icons.active, p.good),
        Section::Inactive => (app.icons.inactive, p.muted),
    };
    let in_section = || app.groups.iter().filter(|g| g.section == section);
    let mut spans = vec![
        Span::styled(
            format!("{icon} {}", section.label()),
            style.add_modifier(Modifier::BOLD),
        ),
        Span::styled(format!(" {}", in_section().count()), p.muted),
    ];
    if in_section()
        .flat_map(|g| &g.workspaces)
        .any(|w| app.needs_attention(w))
    {
        spans.push(Span::styled(format!(" {}", app.icons.active), p.attention));
    }
    let used: usize = spans.iter().map(|s| s.content.width()).sum();
    let rule = usize::from(width).saturating_sub(used + 2);
    if rule > 0 {
        spans.push(Span::styled(format!(" {}", "─".repeat(rule)), p.border));
    }
    Line::from(spans)
}

fn repo_line<'a>(app: &App, p: &Palette, g: &RepoGroup, width: u16) -> Line<'a> {
    let mut left = Vec::new();
    // An arrow only says something when there's a hidden list to get back.
    if g.collapsed && app.filter.is_empty() {
        left.push(Span::styled(format!("{} ", app.icons.collapsed), p.muted));
    }
    let attention = g.workspaces.iter().any(|w| app.needs_attention(w));
    let name_style = match g.section {
        _ if attention => p.attention,
        Section::Inactive => p.muted,
        Section::Pinned | Section::Active => Style::new(),
    };
    left.push(Span::styled(
        g.name.clone(),
        name_style.add_modifier(Modifier::BOLD),
    ));
    let mut right = Vec::new();
    if g.collapsed
        && let Some(mut glyph) = agent_glyph(app, p, g.agent(), WsStatus::Live)
    {
        if attention {
            glyph.style = p.attention.add_modifier(Modifier::BOLD);
        }
        right.push(glyph);
        right.push(Span::raw(" "));
    }
    justify(left, right, width)
}

/// The agent badge at the end of a row. Nothing when there's no agent to report on.
fn agent_glyph<'a>(
    app: &App,
    p: &Palette,
    state: AgentState,
    status: WsStatus,
) -> Option<Span<'a>> {
    Some(match (status, state) {
        (WsStatus::Parked, _) => Span::styled(app.icons.parked, p.muted),
        (WsStatus::Stopped, _) | (_, AgentState::None) => return None,
        (_, AgentState::Awaiting) => {
            Span::styled(app.icons.awaiting, p.warn.add_modifier(Modifier::BOLD))
        }
        (_, AgentState::Busy) => Span::styled(SPINNER[(app.tick as usize) % SPINNER.len()], p.busy),
        (_, AgentState::Compacting) => Span::styled(app.icons.compacting, p.busy),
        (_, AgentState::Idle) => Span::styled(app.icons.idle, p.good),
    })
}

/// Leading icon: the PR's state if there is one, otherwise what kind of checkout this is.
fn git_icon<'a>(app: &App, p: &Palette, w: &WorkspaceView) -> Span<'a> {
    let idle = if w.status == WsStatus::Live {
        p.branch
    } else {
        p.muted
    };
    if let Some(pr) = &w.pr {
        return match pr.state {
            PrState::Open => Span::styled(app.icons.pull_request, p.good),
            PrState::Draft => Span::styled(app.icons.pull_request, p.muted),
            PrState::Merged => Span::styled(app.icons.merged, p.merged),
            PrState::Closed => Span::styled(app.icons.pull_request, p.bad),
        };
    }
    match (&w.branch, &w.git) {
        (Some(_), _) => Span::styled(app.icons.branch, idle),
        (None, Some(_)) => Span::styled(app.icons.detached, idle),
        (None, None) => Span::styled(app.icons.folder, idle),
    }
}

/// Branch name; a short commit id when detached; the workspace name for plain folders.
fn ws_label(w: &WorkspaceView) -> String {
    w.branch
        .clone()
        .or_else(|| {
            let oid = w.git.as_ref()?.oid.as_deref()?;
            Some(oid.chars().take(7).collect())
        })
        .unwrap_or_else(|| w.name.clone())
}

fn ws_line<'a>(app: &App, p: &Palette, w: &WorkspaceView, width: u16) -> Line<'a> {
    let label_style = match w.status {
        _ if app.needs_attention(w) => p.attention.add_modifier(Modifier::BOLD),
        WsStatus::Live => Style::new(),
        WsStatus::Parked | WsStatus::Stopped => p.muted,
    };
    let left = vec![
        Span::raw("  "),
        git_icon(app, p, w),
        Span::raw(" "),
        Span::styled(ws_label(w), label_style),
    ];

    let mut right: Vec<Vec<Span<'a>>> = Vec::new();
    if let Some(git) = &w.git {
        if !git.is_clean() {
            right.push(vec![Span::styled(
                format!("{} {}", app.icons.changed, git.changed()),
                p.warn,
            )]);
        }
        if git.ahead > 0 || git.behind > 0 {
            let count = |sign: char, n: u32, style: Style| {
                Span::styled(format!("{sign}{n}"), if n > 0 { style } else { p.muted })
            };
            right.push(vec![
                count('+', git.ahead, p.good),
                Span::raw(" "),
                count('-', git.behind, p.bad),
            ]);
        }
    }
    let mut ci = Vec::new();
    if let Some(pr) = &w.pr {
        ci.push(Span::styled(format!("#{}", pr.number), p.muted));
    }
    let check = match w.checks {
        Checks::None => None,
        Checks::Passing => Some((app.icons.checks_passing, p.good)),
        Checks::Failing => Some((app.icons.checks_failing, p.bad)),
        Checks::Pending => Some((app.icons.checks_pending, p.warn)),
    };
    if let Some((glyph, style)) = check {
        if !ci.is_empty() {
            ci.push(Span::raw(" "));
        }
        ci.push(Span::styled(glyph, style));
    }
    if !ci.is_empty() {
        right.push(ci);
    }
    if let Some(mut glyph) = agent_glyph(app, p, w.agent, w.status) {
        if app.needs_attention(w) {
            glyph.style = p.attention.add_modifier(Modifier::BOLD);
        }
        right.push(vec![glyph]);
    }
    let mut spaced = Vec::new();
    for (i, part) in right.into_iter().enumerate() {
        if i > 0 {
            spaced.push(Span::raw(" "));
        }
        spaced.extend(part);
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

fn draw_help(frame: &mut Frame<'_>, p: &Palette, icons: &Icons, area: Rect, scroll: u16) {
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
    lines.push(Line::styled("Icons", p.accent.add_modifier(Modifier::BOLD)));
    for (icon, style, meaning) in [
        (icons.branch, p.branch, "Branch (dim when stopped)"),
        (icons.detached, p.branch, "Detached HEAD"),
        (icons.folder, p.branch, "Plain folder"),
        (icons.pull_request, p.good, "Open pull request"),
        (icons.merged, p.merged, "Last pull request, merged"),
        (icons.changed, p.warn, "Changed files"),
        ("+ -", p.good, "Commits ahead / behind"),
        (icons.checks_passing, p.good, "CI passing"),
        (icons.checks_failing, p.bad, "CI failing"),
        (icons.checks_pending, p.warn, "CI running"),
        ("Orange", p.attention, "Needs you, or finished elsewhere"),
        (SPINNER[0], p.busy, "Agent working"),
        (icons.awaiting, p.warn, "Agent waiting on you"),
        (icons.idle, p.good, "Agent idle"),
        (icons.compacting, p.busy, "Agent compacting"),
        (icons.parked, p.muted, "Parked"),
    ] {
        lines.push(Line::from(vec![
            Span::styled(format!("{icon:<9}"), style),
            Span::raw(meaning),
        ]));
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
