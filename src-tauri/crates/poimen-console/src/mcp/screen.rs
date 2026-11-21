//! `read_terminal` (#1154): the last lines of any terminal as plain text. The server's attach
//! snapshot (screen and scrollback as bytes) is replayed into a private emulator and read back
//! as text, so escape sequences are gone and wide and wrapped lines read as on screen. The
//! attachment observes: it sends no input and never takes the control.

use super::{Fail, runs, server_error, text_arg};
use alacritty_terminal::event::{Event, EventListener};
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::{Config, Term};
use alacritty_terminal::vte::ansi::{Processor, StdSyncHandler};
use poimen_client::Client;
use poimen_client::protocol::{AttachMode, TerminalStatus, decode_bytes};
use serde_json::{Value, json};

/// The scrollback the private emulator keeps; the server's own limit is the lower one.
const HISTORY: usize = 20_000;

#[derive(Clone)]
struct Quiet;

impl EventListener for Quiet {
    fn send_event(&self, _: Event) {}
}

struct Size {
    cols: usize,
    rows: usize,
}

impl Dimensions for Size {
    fn total_lines(&self) -> usize {
        self.rows
    }
    fn screen_lines(&self) -> usize {
        self.rows
    }
    fn columns(&self) -> usize {
        self.cols
    }
}

/// The text of the whole scrollback and screen after replaying `snapshot` into a terminal of
/// `cols` x `rows`: one string per logical line (wrapped rows joined), trailing spaces cut,
/// trailing blank lines dropped.
pub(super) fn text_lines(snapshot: &[u8], cols: u16, rows: u16) -> Vec<String> {
    let mut term = Term::new(
        Config {
            scrolling_history: HISTORY,
            ..Config::default()
        },
        &Size {
            cols: usize::from(cols.max(1)),
            rows: usize::from(rows.max(1)),
        },
        Quiet,
    );
    Processor::<StdSyncHandler>::new().advance(&mut term, snapshot);

    let grid = term.grid();
    let top = -i32::try_from(grid.history_size()).unwrap_or(i32::MAX);
    let bottom = i32::try_from(grid.screen_lines()).unwrap_or(i32::MAX);
    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();
    for l in top..bottom {
        let row = &grid[Line(l)];
        let mut wrapped = false;
        for c in 0..grid.columns() {
            let cell = &row[Column(c)];
            if cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
                continue;
            }
            current.push(cell.c);
            if let Some(extra) = cell.zerowidth() {
                current.extend(extra);
            }
            wrapped = cell.flags.contains(Flags::WRAPLINE);
        }
        if wrapped {
            // The row continues in the next one: no cut of the padding in between.
            continue;
        }
        lines.push(current.trim_end().to_string());
        current.clear();
    }
    if !current.is_empty() {
        lines.push(current.trim_end().to_string());
    }
    while lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }
    lines
}

/// The last `count` lines joined by newlines, at most `max_bytes` of them (whole lines from the
/// end; one line longer than the limit keeps its tail). The flag says the answer was cut.
pub(super) fn tail(lines: &[String], count: usize, max_bytes: usize) -> (String, usize, bool) {
    let start = lines.len().saturating_sub(count);
    let mut picked: Vec<&str> = Vec::new();
    let mut bytes = 0usize;
    let mut cut = false;
    for line in lines[start..].iter().rev() {
        let add = line.len() + usize::from(!picked.is_empty());
        if bytes + add > max_bytes {
            if picked.is_empty() {
                let mut from = line.len() - max_bytes.min(line.len());
                while !line.is_char_boundary(from) {
                    from += 1;
                }
                picked.push(&line[from..]);
            }
            cut = true;
            break;
        }
        bytes += add;
        picked.push(line);
    }
    picked.reverse();
    let n = picked.len();
    (picked.join("\n"), n, cut)
}

/// How many lines `read_terminal` returns without `lines`, and at most.
const READ_LINES_DEFAULT: usize = 50;
const READ_LINES_MAX: usize = 500;
/// The most bytes of text `read_terminal` returns.
const READ_MAX_BYTES: usize = 64 * 1024;

/// `read_terminal {terminal, lines?}` (#1154): the end of the screen and scrollback as text,
/// from an observer attachment (no input, no control, no change of the controller).
pub(super) fn read_terminal(client: &Client, args: &Value) -> Result<Value, Fail> {
    runs::only_keys(args, &["terminal", "lines"])?;
    let terminal = text_arg(args, "terminal")
        .ok_or_else(|| Fail::Tool("missing argument `terminal`".into()))?;
    let count = match args.get("lines") {
        None | Some(Value::Null) => READ_LINES_DEFAULT,
        Some(v) => v
            .as_u64()
            .filter(|n| (1..=READ_LINES_MAX as u64).contains(n))
            .ok_or_else(|| Fail::Tool(format!("lines must be 1 to {READ_LINES_MAX}")))?
            as usize,
    };
    let known = client.list_terminals(None).map_err(server_error)?;
    let info = known
        .iter()
        .find(|t| t.id == terminal)
        .ok_or_else(|| Fail::Tool(format!("terminal {terminal} not found")))?;
    let attachment = client
        .attach(terminal, AttachMode::Observe, false)
        .map_err(server_error)?;
    let snapshot = decode_bytes(&attachment.info.snapshot)
        .map_err(|e| Fail::Tool(format!("the snapshot cannot be read: {e}")))?;
    let lines = text_lines(&snapshot, attachment.info.cols, attachment.info.rows);
    drop(attachment);
    let (text, returned, cut) = tail(&lines, count, READ_MAX_BYTES);
    let mut answer = json!({
        "terminal": terminal,
        "state": info.status,
        "running": info.status == TerminalStatus::Running,
        "lines": returned,
        "text": text,
    });
    if cut {
        answer["note"] = json!(format!(
            "cut to the last {READ_MAX_BYTES} bytes; ask for fewer `lines` for the rest"
        ));
    }
    Ok(answer)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_escapes_joins_wrapped_rows_and_wide_cells() {
        let snap = b"\x1b[31mred\x1b[0m line\r\n0123456789abcdef\r\n\xe6\x97\xa5\xe6\x9c\xac x\r\n\r\n\r\n";
        let lines = text_lines(snap, 10, 4);
        assert_eq!(lines, ["red line", "0123456789abcdef", "日本 x"]);
    }

    #[test]
    fn tail_counts_lines_and_cuts_at_the_byte_limit() {
        let lines: Vec<String> = ["a", "bb", "ccc"].map(String::from).to_vec();
        assert_eq!(tail(&lines, 2, 100), ("bb\nccc".into(), 2, false));
        assert_eq!(tail(&lines, 3, 6), ("bb\nccc".into(), 2, true));
        assert_eq!(tail(&lines, 3, 2), ("cc".into(), 1, true));
    }
}
