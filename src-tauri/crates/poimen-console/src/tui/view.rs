//! Draws a frame of the TUI: the sidebar on the left, the pane, the status line at the bottom.

use poimen_client::protocol::{AgentStatus, AttachMode};
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
    pub pane: Rect,
    pub status: Rect,
}

/// Splits the window: status line at the bottom, the sidebar left of the pane when shown and the
/// window is wide enough.
pub fn layout(window: Rect, sidebar: bool) -> Areas {
    let body_h = window.height.saturating_sub(1);
    let status = Rect::new(
        window.x,
        window.y + body_h,
        window.width,
        window.height.min(1),
    );
    let show = sidebar && window.width >= MIN_WIDTH_FOR_SIDEBAR;
    if !show {
        return Areas {
            sidebar: None,
            pane: Rect::new(window.x, window.y, window.width, body_h),
            status,
        };
    }
    Areas {
        sidebar: Some(Rect::new(window.x, window.y, SIDEBAR_WIDTH - 1, body_h)),
        pane: Rect::new(
            window.x + SIDEBAR_WIDTH,
            window.y,
            window.width - SIDEBAR_WIDTH,
            body_h,
        ),
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
    for row in &app.rows[first..] {
        let selected = row.terminal_id.is_some() && row.terminal_id == app.selected;
        for line in row_lines(row) {
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
            y += 1;
        }
    }
}

fn draw_pane(f: &mut Frame, area: Rect, app: &App) {
    let Some(pane) = &app.pane else {
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
    };
    pane.grid.render(area, f.buffer_mut(), &app.palette);
    if pane.controls()
        && let Some((x, y)) = pane.grid.cursor()
        && x < area.width
        && y < area.height
    {
        f.set_cursor_position((area.x + x, area.y + y));
    }
}

/// The status line: the selected terminal and its mode, then a question, a notice or the keys.
pub fn status_text(app: &App) -> (String, String) {
    let name = app.selected_name().unwrap_or("-");
    let state = match &app.pane {
        Some(p) if p.exited.is_some() => match p.exited {
            Some(Some(code)) => format!("exited with {code}"),
            _ => "exited".into(),
        },
        Some(p) if p.mode == AttachMode::Control => "control".into(),
        Some(_) => "observing".into(),
        None => "detached".into(),
    };
    let scrolled = app
        .pane
        .as_ref()
        .map(|p| p.grid.display_offset())
        .filter(|o| *o > 0)
        .map(|o| format!(" · scrolled {o}"))
        .unwrap_or_default();
    let left = format!(" {name} · {state}{scrolled} ");
    let p = app.config.prefix.label();
    let k = &app.config.keys;
    let right = if app.take_over.is_some() {
        "another client has the control: take over? [y/N]".to_owned()
    } else if app.prefix_pending {
        format!(
            "{p} … {} detach · {} sidebar · {}/{} select · {p} sends {p}",
            k.detach.label(),
            k.sidebar.label(),
            k.previous.label(),
            k.next.label()
        )
    } else if let Some(n) = &app.notice {
        n.clone()
    } else {
        format!(
            "{p} {} detach · {p} {}/{} select",
            k.detach.label(),
            k.previous.label(),
            k.next.label()
        )
    };
    (left, right)
}

pub fn draw(f: &mut Frame, app: &App) {
    let areas = layout(f.area(), app.sidebar);
    if let Some(side) = areas.sidebar {
        draw_sidebar(f.buffer_mut(), side, app);
    }
    draw_pane(f, areas.pane, app);
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
        assert_eq!(a.pane, Rect::new(32, 0, 68, 29));
        assert_eq!(a.status, Rect::new(0, 29, 100, 1));
        let b = layout(w, false);
        assert_eq!(b.sidebar, None);
        assert_eq!(b.pane, Rect::new(0, 0, 100, 29));
        // Too narrow: no sidebar even when it is on.
        assert_eq!(layout(Rect::new(0, 0, 50, 10), true).sidebar, None);
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
