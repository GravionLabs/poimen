//! The TUI's pane grid drawn as cells, against fixtures: `tests/fixtures/tui/<case>.ansi` is the
//! terminal output, `<case>.txt` the expected screen. Its first line is the drawn area
//! (`<cols>x<rows>`, optionally ` of <cols>x<rows>` for a terminal of another size), then one line
//! per row with trailing blanks trimmed.

use std::path::Path;

use poimen_console::tui::grid::{Grid, Palette};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier};

fn size(text: &str) -> (u16, u16) {
    let (c, r) = text.trim().split_once('x').expect("<cols>x<rows>");
    (c.parse().unwrap(), r.parse().unwrap())
}

fn fixture(case: &str) -> (Vec<u8>, Rect, (u16, u16), Vec<String>) {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/tui");
    let input = std::fs::read(dir.join(format!("{case}.ansi"))).unwrap();
    let expected = std::fs::read_to_string(dir.join(format!("{case}.txt"))).unwrap();
    let mut lines = expected.lines();
    let header = lines.next().unwrap();
    let (area, grid) = match header.split_once(" of ") {
        Some((a, g)) => (size(a), size(g)),
        None => (size(header), size(header)),
    };
    (
        input,
        Rect::new(0, 0, area.0, area.1),
        grid,
        lines.map(str::to_owned).collect(),
    )
}

fn screen(buf: &Buffer) -> Vec<String> {
    let a = buf.area;
    (a.top()..a.bottom())
        .map(|y| {
            (a.left()..a.right())
                .map(|x| buf[(x, y)].symbol())
                .collect::<String>()
                .trim_end()
                .to_owned()
        })
        .collect()
}

fn render(case: &str, palette: &Palette) -> Buffer {
    let (input, area, (cols, rows), expected) = fixture(case);
    let mut grid = Grid::new(cols, rows);
    grid.feed(&input);
    let mut buf = Buffer::empty(area);
    grid.render(area, &mut buf, palette);
    assert_eq!(screen(&buf), expected, "fixture {case}");
    buf
}

#[test]
fn colors_and_attributes() {
    let buf = render("colors", &Palette::default());
    let red = &buf[(0, 0)];
    assert_eq!(red.fg, Color::Indexed(1));
    assert!(red.modifier.contains(Modifier::BOLD));
    // Default colors stay the local terminal's.
    let plain = &buf[(4, 0)];
    assert_eq!((plain.fg, plain.bg), (Color::Reset, Color::Reset));
    assert!(buf[(0, 1)].modifier.contains(Modifier::UNDERLINED));
    assert_eq!(buf[(6, 1)].fg, Color::Rgb(1, 2, 3));
    assert!(buf[(10, 1)].modifier.contains(Modifier::REVERSED));
    assert_eq!(buf[(0, 2)].bg, Color::Indexed(2));
}

#[test]
fn a_user_scheme_colors_the_cells() {
    let mut ansi = [(0, 0, 0); 16];
    ansi[1] = (0xff, 0x55, 0x55);
    let palette = Palette {
        foreground: Some((0xf8, 0xf8, 0xf2)),
        background: Some((0x28, 0x2a, 0x36)),
        cursor: None,
        ansi: Some(ansi),
    };
    let buf = render("colors", &palette);
    assert_eq!(buf[(0, 0)].fg, Color::Rgb(0xff, 0x55, 0x55));
    assert_eq!(buf[(4, 0)].bg, Color::Rgb(0x28, 0x2a, 0x36));
    // Inverse swaps the scheme's colors instead of asking the local terminal.
    let inv = &buf[(10, 1)];
    assert_eq!(inv.fg, Color::Rgb(0x28, 0x2a, 0x36));
    assert_eq!(inv.bg, Color::Rgb(0xf8, 0xf8, 0xf2));
    assert!(!inv.modifier.contains(Modifier::REVERSED));
    // Blank cells past the text get the scheme background too.
    assert_eq!(buf[(19, 3)].bg, Color::Rgb(0x28, 0x2a, 0x36));
}

#[test]
fn wide_and_combining_characters() {
    let buf = render("wide", &Palette::default());
    assert_eq!(buf[(0, 0)].symbol(), "日");
    assert_eq!(buf[(1, 0)].symbol(), "");
    assert_eq!(buf[(2, 0)].symbol(), "本");
}

#[test]
fn alternate_screen() {
    render("alt-screen", &Palette::default());
}

#[test]
fn larger_terminal_is_clipped() {
    render("clipped", &Palette::default());
}

#[test]
fn smaller_terminal_is_padded() {
    render("padded", &Palette::default());
}
