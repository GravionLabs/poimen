//! Draws a frame of the TUI: the sidebar on the left, the pane, the status line at the bottom.

use poimen_client::protocol::{AgentStatus, AttachMode, SplitDirection};
use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

use super::App;
use super::sidebar::{Row, RowKind, SegmentKind};

/// Columns of the sidebar, its right border included.
pub const SIDEBAR_WIDTH: u16 = 32;
/// Below this window width the sidebar is left out, so the pane keeps room.
pub const MIN_WIDTH_FOR_SIDEBAR: u16 = 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Areas {
    /// The sidebar's rows (without the border column).
    pub sidebar: Option<Rect>,
    /// One line above the panes: the tabs of the focused space.
    pub tabbar: Rect,
    /// Where the split tree of the focused tab is laid out.
    pub pane: Rect,
    pub status: Rect,
}

/// Splits the window: status line at the bottom, the sidebar left of the pane when shown and the
/// window is wide enough, the tab bar on top of the pane column.
pub fn layout(window: Rect, sidebar: bool) -> Areas {
    let body_h = window.height.saturating_sub(1);
    let status = Rect::new(
        window.x,
        window.y + body_h,
        window.width,
        window.height.min(1),
    );
    let show = sidebar && window.width >= MIN_WIDTH_FOR_SIDEBAR;
    let (side, x, width) = if show {
        (
            Some(Rect::new(window.x, window.y, SIDEBAR_WIDTH - 1, body_h)),
            window.x + SIDEBAR_WIDTH,
            window.width - SIDEBAR_WIDTH,
        )
    } else {
        (None, window.x, window.width)
    };
    // The bar needs a line of its own; a one-line body is all pane.
    let bar_h = u16::from(body_h > 1);
    Areas {
        sidebar: side,
        tabbar: Rect::new(x, window.y, width, bar_h),
        pane: Rect::new(x, window.y + bar_h, width, body_h - bar_h),
        status,
    }
}

/// The first row index drawn so that the selected row fits into `height` lines.
pub fn first_visible(rows: &[Row], selected: Option<&str>, height: u16) -> usize {
    let Some(at) = rows
        .iter()
        .position(|r| r.terminal_id.as_deref() == selected && selected.is_some())
    else {
        return 0;
    };
    let mut first = 0;
    let used = |from: usize| -> u16 { rows[from..=at].iter().map(Row::height).sum() };
    while first < at && used(first) > height {
        first += 1;
    }
    first
}

/// The terminal of the sidebar row at line `y` of the sidebar.
pub fn row_at(app: &App, sidebar: Rect, y: u16) -> Option<String> {
    let mut line = 0;
    let first = first_visible(&app.rows, app.selected.as_deref(), sidebar.height);
    for row in &app.rows[first..] {
        let h = row.height();
        if y < line + h {
            return row.terminal_id.clone();
        }
        line += h;
    }
    None
}

fn status_color(status: Option<AgentStatus>) -> Color {
    match status {
        Some(AgentStatus::Working) => Color::Yellow,
        Some(AgentStatus::Blocked) => Color::Red,
        Some(AgentStatus::Done) => Color::Green,
        Some(AgentStatus::Idle) => Color::Blue,
        _ => Color::DarkGray,
    }
}

fn row_lines(row: &Row) -> Vec<Line<'static>> {
    let indent = "  ".repeat(usize::from(row.depth));
    row.lines
        .iter()
        .enumerate()
        .map(|(i, segments)| {
            // Lines after the first are indented past the icon.
            let mut spans = vec![Span::raw(if i == 0 {
                indent.clone()
            } else {
                format!("{indent}  ")
            })];
            let mut previous: Option<SegmentKind> = None;
            for s in segments {
                if previous.is_some() {
                    let sep = if previous == Some(SegmentKind::Text) && s.kind == SegmentKind::Text
                    {
                        " · "
                    } else {
                        " "
                    };
                    spans.push(Span::styled(sep, Style::default().fg(Color::DarkGray)));
                }
                let mut style = match s.kind {
                    SegmentKind::Dot | SegmentKind::State => {
                        Style::default().fg(status_color(row.status))
                    }
                    SegmentKind::Name if row.kind == RowKind::Space => {
                        Style::default().add_modifier(Modifier::BOLD)
                    }
                    SegmentKind::Name => Style::default(),
                    SegmentKind::Text => Style::default().fg(Color::DarkGray),
                };
                if s.bold {
                    style = style.add_modifier(Modifier::BOLD);
                }
                if s.dim {
                    style = style.add_modifier(Modifier::DIM);
                }
                spans.push(Span::styled(s.text.clone(), style));
                previous = Some(s.kind);
            }
            Line::from(spans)
        })
        .collect()
}

/// The marks at the end of a row: `!` (red) for terminals that need you, `*` (cyan) for unread
/// ones; a space row counts those of its terminals, a terminal row shows the mark alone.
fn marks_of(app: &App, index: usize) -> Line<'static> {
    let row = &app.rows[index];
    let ids: Vec<&str> = match (&row.kind, &row.terminal_id) {
        (_, Some(id)) => vec![id.as_str()],
        (RowKind::Space, None) => app.rows[index + 1..]
            .iter()
            .take_while(|r| r.kind != RowKind::Space)
            .filter_map(|r| r.terminal_id.as_deref())
            .collect(),
        _ => Vec::new(),
    };
    let ringing = ids
        .iter()
        .filter(|id| app.attention.has_attention(id))
        .count();
    let unread = ids.iter().filter(|id| app.attention.is_unread(id)).count();
    let counted = |symbol: &str, n: usize| {
        if row.terminal_id.is_some() {
            symbol.to_owned()
        } else {
            format!("{symbol}{n}")
        }
    };
    let mut spans = Vec::new();
    if ringing > 0 {
        spans.push(Span::styled(
            counted("!", ringing),
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        ));
    }
    if unread > 0 {
        if !spans.is_empty() {
            spans.push(Span::raw(" "));
        }
        spans.push(Span::styled(
            counted("*", unread),
            Style::default().fg(Color::Cyan),
        ));
    }
    Line::from(spans)
}

/// The notification list over the pane area: age, terminal (and agent), what happened, the text.
fn draw_notifications(buf: &mut Buffer, area: Rect, app: &App) {
    let Some(selected) = app.list else {
        return;
    };
    let entries = app.log.entries();
    let height = u16::try_from(entries.len().max(1) + 2)
        .unwrap_or(u16::MAX)
        .min(area.height);
    if height < 3 || area.width < 20 {
        return;
    }
    let rect = Rect::new(area.x, area.y, area.width, height);
    let plain = Style::default();
    for y in rect.top()..rect.bottom() {
        buf.set_string(rect.x, y, " ".repeat(usize::from(rect.width)), plain);
    }
    let title = " Notifications · Enter focuses · Esc closes ";
    buf.set_stringn(
        rect.x,
        rect.y,
        title,
        usize::from(rect.width),
        plain.add_modifier(Modifier::REVERSED | Modifier::BOLD),
    );
    if entries.is_empty() {
        buf.set_stringn(
            rect.x + 1,
            rect.y + 1,
            "no notifications",
            usize::from(rect.width - 1),
            Style::default().fg(Color::DarkGray),
        );
    }
    let visible = usize::from(height - 2);
    let first = selected.saturating_sub(visible - 1);
    for (i, e) in entries.iter().enumerate().skip(first).take(visible) {
        let y = rect.y + 1 + u16::try_from(i - first).unwrap_or(0);
        let who = match &e.agent {
            Some(a) => format!("{} ({a})", e.name),
            None => e.name.clone(),
        };
        let line = format!(
            " {:>6}  {} · {} · {}",
            super::attention::age(app.now_ms, e.at_ms),
            who,
            e.kind.label(),
            e.text
        );
        let color = match e.kind {
            super::attention::Kind::Blocked => Color::Red,
            super::attention::Kind::Done => Color::Green,
        };
        let mut style = Style::default().fg(color);
        if i == selected {
            style = style.add_modifier(Modifier::REVERSED);
            buf.set_style(Rect::new(rect.x, y, rect.width, 1), style);
        }
        buf.set_stringn(rect.x, y, &line, usize::from(rect.width), style);
    }
}

fn draw_sidebar(buf: &mut Buffer, area: Rect, app: &App) {
    let border = Rect::new(area.right(), area.y, 1, area.height);
    for y in border.top()..border.bottom() {
        buf[(border.x, y)]
            .set_symbol("│")
            .set_style(Style::default().fg(Color::DarkGray));
    }
    if app.rows.is_empty() {
        buf.set_string(
            area.x,
            area.y,
            "no spaces",
            Style::default().fg(Color::DarkGray),
        );
        return;
    }
    let first = first_visible(&app.rows, app.selected.as_deref(), area.height);
    let mut y = area.y;
    for (index, row) in app.rows.iter().enumerate().skip(first) {
        let selected = row.terminal_id.is_some() && row.terminal_id == app.selected;
        let marks = marks_of(app, index);
        for (n, line) in row_lines(row).into_iter().enumerate() {
            if y >= area.bottom() {
                return;
            }
            if selected {
                buf.set_style(
                    Rect::new(area.x, y, area.width, 1),
                    Style::default().add_modifier(Modifier::REVERSED),
                );
            }
            buf.set_line(area.x, y, &line, area.width);
            if n == 0 && !marks.spans.is_empty() {
                let width = u16::try_from(marks.width()).unwrap_or(0);
                if width < area.width {
                    buf.set_line(area.right() - width, y, &marks, width);
                }
            }
            y += 1;
        }
    }
}

/// The tab bar: the tabs of the focused terminal's space, the focused one reversed.
fn draw_tabbar(buf: &mut Buffer, area: Rect, app: &App) {
    if area.height == 0 {
        return;
    }
    let Some(tab) = app.current_tab() else {
        return;
    };
    let mut x = area.x;
    for (i, t) in app.model.space_tabs(&tab.space_id).iter().enumerate() {
        let label = format!(" {}:{} ", i + 1, t.name);
        let style = if t.tab_id == tab.tab_id {
            Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        let room = usize::from(area.right().saturating_sub(x));
        if room == 0 {
            break;
        }
        let (nx, _) = buf.set_stringn(x, area.y, &label, room, style);
        x = nx;
    }
}

/// Whether a border runs along the focused pane (it is drawn in the accent color).
fn touches(border: Rect, pane: Rect) -> bool {
    Rect::new(
        border.x.saturating_sub(1),
        border.y.saturating_sub(1),
        border.width + 2,
        border.height + 2,
    )
    .intersects(pane)
}

fn draw_panes(f: &mut Frame, area: Rect, app: &App) {
    if app.selected.is_none() {
        let text = if app.model.terminals.is_empty() {
            "No terminals. Create one with `poimen new <space>` or in the app."
        } else {
            "No terminal attached. Select one with the prefix and ↑/↓."
        };
        f.buffer_mut().set_stringn(
            area.x + 1,
            area.y + area.height / 2,
            text,
            usize::from(area.width.saturating_sub(2)),
            Style::default().fg(Color::DarkGray),
        );
        return;
    }
    let tree = app.layout(area);
    let focus = app.selected.as_deref();
    for leaf in &tree.leaves {
        let rect = leaf.rect;
        if rect.width == 0 || rect.height == 0 {
            continue;
        }
        let Some(pane) = app.pane(&leaf.terminal_id) else {
            let text = if Some(&leaf.terminal_id) == app.own_terminal.as_ref() {
                "poimen runs here"
            } else {
                "not attached"
            };
            f.buffer_mut().set_stringn(
                rect.x + 1,
                rect.y + rect.height / 2,
                text,
                usize::from(rect.width.saturating_sub(2)),
                Style::default().fg(Color::DarkGray),
            );
            continue;
        };
        pane.grid.render(rect, f.buffer_mut(), &app.palette);
        if Some(leaf.terminal_id.as_str()) == focus
            && pane.controls()
            && let Some((x, y)) = pane.grid.cursor()
            && x < rect.width
            && y < rect.height
        {
            f.set_cursor_position((rect.x + x, rect.y + y));
        }
    }
    let focused = focus.and_then(|id| tree.rect_of(id));
    for b in &tree.borders {
        let (symbol, len) = match b.direction {
            SplitDirection::Right => ("│", b.rect.height),
            SplitDirection::Down => ("─", b.rect.width),
        };
        let accent = focused.is_some_and(|r| touches(b.rect, r));
        let style = Style::default().fg(if accent { Color::Cyan } else { Color::DarkGray });
        for i in 0..len {
            let (x, y) = match b.direction {
                SplitDirection::Right => (b.rect.x, b.rect.y + i),
                SplitDirection::Down => (b.rect.x + i, b.rect.y),
            };
            f.buffer_mut()[(x, y)].set_symbol(symbol).set_style(style);
        }
    }
}

/// The status line: the selected terminal and its mode, then a question, a notice or the keys.
pub fn status_text(app: &App) -> (String, String) {
    let name = app.selected_name().unwrap_or("-");
    let state = match app.focused() {
        Some(p) if p.exited.is_some() => match p.exited {
            Some(Some(code)) => format!("exited with {code}"),
            _ => "exited".into(),
        },
        Some(p) if p.mode == AttachMode::Control => "control".into(),
        Some(_) => "observing".into(),
        None => "detached".into(),
    };
    let scrolled = app
        .focused()
        .map(|p| p.grid.display_offset())
        .filter(|o| *o > 0)
        .map(|o| format!(" · scrolled {o}"))
        .unwrap_or_default();
    let marked = match app.attention.count() {
        0 => String::new(),
        n => format!(" · {n} to see"),
    };
    let left = format!(" {name} · {state}{scrolled}{marked} ");
    let p = app.config.prefix.label();
    let k = &app.config.keys;
    let right = if app.take_over.is_some() {
        "another client has the control: take over? [y/N]".to_owned()
    } else if app.prefix_pending {
        format!(
            "{p} … {} tab · {}/{} tabs · {} close tab · {} {} split · {}{}{}{} focus · {}{}{}{} resize · {} close pane · {}/{} select · {} notifications · {} sidebar · {} detach · {p} sends {p}",
            k.new_tab.label(),
            k.previous_tab.label(),
            k.next_tab.label(),
            k.close_tab.label(),
            k.split_right.label(),
            k.split_down.label(),
            k.focus_left.label(),
            k.focus_down.label(),
            k.focus_up.label(),
            k.focus_right.label(),
            k.resize_left.label(),
            k.resize_down.label(),
            k.resize_up.label(),
            k.resize_right.label(),
            k.close_pane.label(),
            k.previous.label(),
            k.next.label(),
            k.notifications.label(),
            k.sidebar.label(),
            k.detach.label()
        )
    } else if let Some(n) = &app.notice {
        n.clone()
    } else {
        format!(
            "{p} {} tab · {p} {}/{} tabs · {p} {} detach",
            k.new_tab.label(),
            k.previous_tab.label(),
            k.next_tab.label(),
            k.detach.label()
        )
    };
    (left, right)
}

pub fn draw(f: &mut Frame, app: &App) {
    let areas = layout(f.area(), app.sidebar);
    if let Some(side) = areas.sidebar {
        draw_sidebar(f.buffer_mut(), side, app);
    }
    draw_tabbar(f.buffer_mut(), areas.tabbar, app);
    draw_panes(f, areas.pane, app);
    draw_notifications(f.buffer_mut(), areas.pane, app);
    let (left, right) = status_text(app);
    let bar = Style::default().add_modifier(Modifier::REVERSED);
    let buf = f.buffer_mut();
    buf.set_style(areas.status, bar);
    buf.set_stringn(
        areas.status.x,
        areas.status.y,
        &left,
        usize::from(areas.status.width),
        bar.add_modifier(Modifier::BOLD),
    );
    let used = u16::try_from(left.chars().count()).unwrap_or(u16::MAX);
    if used + 1 < areas.status.width {
        buf.set_stringn(
            areas.status.x + used + 1,
            areas.status.y,
            &right,
            usize::from(areas.status.width - used - 1),
            bar,
        );
    }
    if let Some(p) = &app.command_palette {
        super::palette::draw(f, f.area(), p);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::sidebar::Segment;

    fn row(id: Option<&str>, lines: usize) -> Row {
        Row {
            kind: if id.is_some() {
                RowKind::Terminal
            } else {
                RowKind::Space
            },
            terminal_id: id.map(str::to_owned),
            depth: 0,
            status: None,
            lines: vec![
                vec![Segment {
                    kind: SegmentKind::Name,
                    text: "x".into(),
                    bold: false,
                    dim: false
                }];
                lines
            ],
        }
    }

    #[test]
    fn layout_with_and_without_the_sidebar() {
        let w = Rect::new(0, 0, 100, 30);
        let a = layout(w, true);
        assert_eq!(a.sidebar, Some(Rect::new(0, 0, 31, 29)));
        assert_eq!(a.tabbar, Rect::new(32, 0, 68, 1));
        assert_eq!(a.pane, Rect::new(32, 1, 68, 28));
        assert_eq!(a.status, Rect::new(0, 29, 100, 1));
        let b = layout(w, false);
        assert_eq!(b.sidebar, None);
        assert_eq!(b.tabbar, Rect::new(0, 0, 100, 1));
        assert_eq!(b.pane, Rect::new(0, 1, 100, 28));
        // Too narrow: no sidebar even when it is on.
        assert_eq!(layout(Rect::new(0, 0, 50, 10), true).sidebar, None);
        // A body of one line is all pane.
        let tiny = layout(Rect::new(0, 0, 80, 2), false);
        assert_eq!(tiny.tabbar.height, 0);
        assert_eq!(tiny.pane, Rect::new(0, 0, 80, 1));
    }

    #[test]
    fn first_visible_keeps_the_selection_on_screen() {
        let rows = vec![
            row(None, 2),
            row(Some("a"), 2),
            row(Some("b"), 2),
            row(Some("c"), 2),
        ];
        assert_eq!(first_visible(&rows, Some("a"), 4), 0);
        assert_eq!(first_visible(&rows, Some("c"), 4), 2);
        assert_eq!(first_visible(&rows, Some("c"), 100), 0);
        assert_eq!(first_visible(&rows, None, 4), 0);
        assert_eq!(first_visible(&rows, Some("gone"), 4), 0);
    }
}
