use super::*;

fn fresh(cols: u16, rows: u16) -> Emulator {
    Emulator::new(cols, rows, 1000)
}

fn replay(e: &Emulator) -> Emulator {
    let (cols, rows) = e.size();
    let mut copy = Emulator::new(cols, rows, 1000);
    copy.feed(&e.snapshot());
    copy
}

fn row_text(e: &Emulator, line: i32) -> String {
    let grid = e.term.grid();
    (0..grid.columns())
        .map(|c| grid[Line(line)][Column(c)].c)
        .collect::<String>()
        .trim_end()
        .to_string()
}

#[test]
fn snapshot_restores_alt_screen_mouse_and_paste_modes() {
    let mut e = fresh(40, 10);
    e.feed(b"\x1b[?1049h\x1b[?1006h\x1b[?2004h");
    let copy = replay(&e);
    for flag in [
        TermMode::ALT_SCREEN,
        TermMode::SGR_MOUSE,
        TermMode::BRACKETED_PASTE,
    ] {
        assert!(copy.mode().contains(flag), "{flag:?}");
    }
    assert!(!copy.mode().contains(TermMode::MOUSE_MODE));
}

#[test]
fn snapshot_restores_each_mouse_mode_and_the_other_input_modes() {
    let cases: [(&[u8], TermMode); 4] = [
        (
            b"\x1b[?1000h\x1b[?1006h",
            TermMode::MOUSE_REPORT_CLICK | TermMode::SGR_MOUSE,
        ),
        (b"\x1b[?1002h", TermMode::MOUSE_DRAG),
        (
            b"\x1b[?1003h\x1b[?1004h",
            TermMode::MOUSE_MOTION | TermMode::FOCUS_IN_OUT,
        ),
        (
            b"\x1b[?1h\x1b=\x1b[?25l",
            TermMode::APP_CURSOR | TermMode::APP_KEYPAD,
        ),
    ];
    for (input, expected) in cases {
        let mut e = fresh(40, 10);
        e.feed(input);
        assert!(e.mode().contains(expected), "setup {input:?}");
        let copy = replay(&e);
        assert_eq!(copy.mode(), e.mode(), "{input:?}");
    }
}

#[test]
fn snapshot_restores_cells_cursor_style_title_and_region() {
    let mut e = fresh(20, 8);
    e.feed(b"\x1b]2;my title\x07\x1b[2;6r\x1b[3 q\x1b[1;31mred\x1b[0m\r\nline2\x1b[4;5H");
    let copy = replay(&e);
    assert_eq!(row_text(&copy, 0), "red");
    assert_eq!(row_text(&copy, 1), "line2");
    assert_eq!(copy.title().as_deref(), Some("my title"));
    assert_eq!(copy.scroll_region(), Some((1, 5)));
    assert_eq!(copy.cursor(), (3, 4));
    let style = copy.term.cursor_style();
    assert_eq!(
        (style.shape, style.blinking),
        (CursorShape::Underline, true)
    );
    let cell = &copy.term.grid()[Line(0)][Column(0)];
    assert_eq!(cell.fg, Color::Named(NamedColor::Red));
    assert!(cell.flags.contains(Flags::BOLD));
}

#[test]
fn snapshot_includes_scrollback() {
    let mut e = fresh(10, 3);
    for i in 0..8 {
        e.feed(format!("row{i}\r\n").as_bytes());
    }
    let copy = replay(&e);
    let grid = copy.term.grid();
    assert_eq!(grid.history_size(), e.term.grid().history_size());
    assert_eq!(
        row_text(&copy, i32::try_from(-(grid.history_size() as i64)).unwrap()),
        "row0"
    );
}

#[test]
fn scrollback_limit_is_respected() {
    let mut e = Emulator::new(10, 3, 5);
    for i in 0..50 {
        e.feed(format!("row{i}\r\n").as_bytes());
    }
    assert_eq!(e.term.grid().history_size(), 5);
}

#[test]
fn alt_screen_keeps_the_primary_screen_for_when_it_is_left() {
    let mut e = fresh(20, 5);
    e.feed(b"shell prompt\r\n\x1b[?1049h\x1b[Halt content");
    let mut copy = replay(&e);
    assert!(copy.mode().contains(TermMode::ALT_SCREEN));
    assert_eq!(row_text(&copy, 0), "alt content");
    copy.feed(b"\x1b[?1049l");
    assert!(!copy.mode().contains(TermMode::ALT_SCREEN));
    assert_eq!(row_text(&copy, 0), "shell prompt");
}

#[test]
fn a_sequence_split_across_feeds_is_still_seen() {
    let mut e = fresh(20, 5);
    e.feed(b"primary\r\n\x1b[?10");
    e.feed(b"49h\x1b[2;4");
    e.feed(b"r");
    let mut copy = replay(&e);
    assert_eq!(e.scroll_region(), Some((1, 3)));
    copy.feed(b"\x1b[?1049l");
    assert_eq!(row_text(&copy, 0), "primary");
}

#[test]
fn wrapped_lines_stay_one_logical_line() {
    let mut e = fresh(5, 4);
    e.feed(b"abcdefgh\r\nnext");
    let copy = replay(&e);
    assert_eq!(row_text(&copy, 0), "abcde");
    assert_eq!(row_text(&copy, 1), "fgh");
    assert!(
        copy.term.grid()[Line(0)][Column(4)]
            .flags
            .contains(Flags::WRAPLINE)
    );
}

#[test]
fn resize_resets_the_region_and_changes_the_size() {
    let mut e = fresh(20, 8);
    e.feed(b"\x1b[2;5r");
    e.resize(30, 12);
    assert_eq!(e.size(), (30, 12));
    assert_eq!(e.scroll_region(), None);
}

#[test]
fn full_reset_clears_the_region() {
    let mut e = fresh(20, 8);
    e.feed(b"\x1b[2;5r\x1bc");
    assert_eq!(e.scroll_region(), None);
}

#[test]
fn scanner_ignores_text_inside_strings() {
    let mut s = Scanner::default();
    assert!(s.scan(b"\x1b]0;title ?1049h r\x07").is_empty());
    assert_eq!(s.scan(b"\x1b[?1049h"), vec![Hit::EnterAlt(7)]);
}

fn replies(input: &[u8]) -> String {
    let mut e = fresh(20, 5);
    e.feed(input);
    String::from_utf8(e.take_replies(&Colors::default())).unwrap()
}

#[test]
fn cursor_position_report_is_one_based() {
    assert_eq!(replies(b"\x1b[3;7H\x1b[6n"), "\x1b[3;7R");
}

#[test]
fn device_attributes_are_answered() {
    assert_eq!(replies(b"\x1b[c"), "\x1b[?6c");
    assert!(replies(b"\x1b[>c").starts_with("\x1b[>0;"));
}

#[test]
fn color_queries_come_from_the_scheme_and_a_program_setting_wins() {
    let colors = Colors {
        foreground: Rgb { r: 1, g: 2, b: 3 },
        background: Rgb {
            r: 0xaa,
            g: 0xbb,
            b: 0xcc,
        },
        ..Colors::default()
    };
    let mut e = fresh(20, 5);
    e.feed(b"\x1b]10;?\x1b\\\x1b]11;?\x07\x1b]4;1;?\x07\x1b]4;232;?\x07");
    let out = String::from_utf8(e.take_replies(&colors)).unwrap();
    assert_eq!(
        out,
        "\x1b]10;rgb:0101/0202/0303\x1b\\\x1b]11;rgb:aaaa/bbbb/cccc\x07\
         \x1b]4;1;rgb:cdcd/0000/0000\x07\x1b]4;232;rgb:0808/0808/0808\x07"
    );
    e.feed(b"\x1b]11;rgb:10/20/30\x07\x1b]11;?\x07");
    assert_eq!(
        String::from_utf8(e.take_replies(&colors)).unwrap(),
        "\x1b]11;rgb:1010/2020/3030\x07"
    );
}

#[test]
fn the_detection_query_is_answered_ahead_of_the_following_da1() {
    assert_eq!(
        replies(b"\x1b]7501;?\x1b\\\x1b[c"),
        format!("{DETECTION_REPLY}\x1b[?6c")
    );
    assert_eq!(replies(b"\x1b]7501;?\x07"), DETECTION_REPLY);
    // What came before the query is answered before it, also in one chunk.
    assert_eq!(
        replies(b"\x1b[c\x1b]7501;?\x07\x1b[c"),
        format!("\x1b[?6c{DETECTION_REPLY}\x1b[?6c")
    );
    // Split over feeds.
    let mut e = fresh(10, 3);
    e.feed(b"\x1b]7501;");
    e.feed(b"?\x1b");
    e.feed(b"\\\x1b[c");
    assert_eq!(
        String::from_utf8(e.take_replies(&Colors::default())).unwrap(),
        format!("{DETECTION_REPLY}\x1b[?6c")
    );
}

#[test]
fn the_detection_answer_is_also_given_separately_and_carries_no_report() {
    let mut e = fresh(10, 3);
    e.feed(b"\x1b]7501;state=blocked:msg=aGk\x07\x1b[c\x1b]7501;?\x07");
    let a = e.take_answers(&Colors::default());
    assert_eq!(a.own, DETECTION_REPLY.as_bytes());
    assert_eq!(a.all, format!("\x1b[?6c{DETECTION_REPLY}").into_bytes());
    assert!(!a.own.windows(3).any(|w| w == b"msg"));
    // A report is not a query and a query with more is not either.
    assert!(replies(b"\x1b]7501;state=idle\x07\x1b]7501;?x\x07").is_empty());
    assert!(e.take_answers(&Colors::default()).own.is_empty());
}

#[test]
fn replies_are_taken_once() {
    let mut e = fresh(20, 5);
    e.feed(b"\x1b[c");
    assert_eq!(e.take_replies(&Colors::default()), b"\x1b[?6c");
    assert!(e.take_replies(&Colors::default()).is_empty());
}

/// The input of `src/app/terminal-view/unicode.spec.ts` (#502): wide characters without
/// their spacer cells, a row that relies on autowrap, the cursor last. A change here must
/// be copied there, so xterm.js stays tested against what the server sends.
#[test]
fn snapshot_of_wide_characters_and_a_wrapped_row() {
    let mut e = fresh(8, 4);
    e.feed("✅⚠🚀ab\r\n0123456789\x1b[1;6H".as_bytes());
    assert_eq!(
        String::from_utf8(e.dump_screen()).unwrap(),
        "✅⚠🚀ab\r\n0123456789\r\n\x1b[1;6H"
    );
    let copy = replay(&e);
    assert_eq!(row_text(&copy, 0), "✅ ⚠🚀 ab");
    assert_eq!(row_text(&copy, 1), "01234567");
    assert_eq!(row_text(&copy, 2), "89");
    assert_eq!(copy.cursor(), (0, 5));
}

// --- OSC 7501 -------------------------------------------------------------------------

fn reports(bytes: &[u8]) -> Vec<Report> {
    let mut s = Scanner::default();
    s.scan(bytes)
        .into_iter()
        .filter_map(|h| match h {
            Hit::Report(r) => Some(r),
            _ => None,
        })
        .collect()
}

fn b64(text: &str) -> String {
    poimen_protocol::encode_bytes(text.as_bytes())
}

#[test]
fn a_report_is_parsed_with_either_terminator() {
    for end in ["\x07", "\x1b\\"] {
        let got = reports(format!("a\x1b]7501;state=working:app=codex{end}b").as_bytes());
        assert_eq!(got.len(), 1, "{end:?}");
        assert_eq!(got[0].state, Some(ReportState::Working));
        assert_eq!(got[0].app.as_deref(), Some("codex"));
        assert_eq!(got[0].id, None);
    }
}

#[test]
fn a_report_split_at_every_byte_is_the_same_report() {
    let text = format!(
        "x\x1b]7501;state=blocked:app=aider:msg={}\x1b\\y",
        b64("Allow? é")
    );
    let whole = reports(text.as_bytes());
    assert_eq!(whole.len(), 1);
    assert_eq!(whole[0].msg.as_deref(), Some("Allow? é"));
    for cut in 1..text.len() {
        let mut s = Scanner::default();
        let (a, b) = text.as_bytes().split_at(cut);
        let mut hits = s.scan(a);
        hits.extend(s.scan(b));
        assert_eq!(hits, vec![Hit::Report(whole[0].clone())], "cut at {cut}");
    }
    let mut s = Scanner::default();
    let mut hits = Vec::new();
    for byte in text.as_bytes() {
        hits.extend(s.scan(std::slice::from_ref(byte)));
    }
    assert_eq!(hits, vec![Hit::Report(whole[0].clone())]);
}

#[test]
fn pairs_follow_the_spec() {
    let got =
        reports(b"\x1b]7501; state = idle :junk:=x:Bad=1:nokey:foo=bar:app=a:app=b:title=aGk\x07");
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].state, Some(ReportState::Idle));
    assert_eq!(got[0].app.as_deref(), Some("b"));
    // Padding is optional.
    assert_eq!(got[0].title.as_deref(), Some("hi"));
    let padded = reports(b"\x1b]7501;state=idle:title=aGk=\x07");
    assert_eq!(padded[0].title.as_deref(), Some("hi"));
    // The last duplicate wins, also for the state.
    let dup = reports(b"\x1b]7501;state=idle:state=done\x07");
    assert_eq!(dup[0].state, Some(ReportState::Done));
    // A value with characters outside the set makes a malformed pair: skipped.
    let bad = reports(b"\x1b]7501;state=idle:app=a b\x07");
    assert_eq!(bad[0].app, None);
    let rich = reports(b"\x1b]7501;state=blocked:progress=40:kind=permission\x07");
    assert_eq!(rich[0].progress, Some(40));
    assert_eq!(rich[0].kind.as_deref(), Some("permission"));
    // Out of range, not a number or an unknown kind: ignored, the state still counts.
    for body in [
        "progress=101",
        "progress=-1",
        "progress=4.5",
        "progress=",
        "kind=other",
    ] {
        let got = reports(format!("\x1b]7501;state=working:{body}\x07").as_bytes());
        assert_eq!(got[0].state, Some(ReportState::Working), "{body}");
        assert_eq!(
            (got[0].progress, got[0].kind.as_deref()),
            (None, None),
            "{body}"
        );
    }
    let child = reports(b"\x1b]7501;state=working:id=sub-1\x07");
    assert_eq!(child[0].id.as_deref(), Some("sub-1"));
}

#[test]
fn a_child_id_follows_the_segment_rules() {
    let ok = reports(b"\x1b]7501;state=idle:id=run/issue-12\x07");
    assert_eq!(ok[0].id.as_deref(), Some("run/issue-12"));
    let seg33 = format!("\x1b]7501;state=idle:id={}\x07", "a".repeat(33));
    let deep = format!("\x1b]7501;state=idle:id={}\x07", ["a"; 9].join("/"));
    for bad in ["id=a//b", "id=/a", "id=a/", "id=a=b"] {
        let text = format!("\x1b]7501;state=idle:{bad}\x07");
        assert!(reports(text.as_bytes()).is_empty(), "{bad}");
    }
    assert!(reports(seg33.as_bytes()).is_empty());
    assert!(reports(deep.as_bytes()).is_empty());
}

#[test]
fn invalid_reports_are_dropped_whole() {
    let long_msg = b64(&"a".repeat(2049));
    let long_title = b64(&"a".repeat(193));
    let cases = [
        "state=nope".to_string(),
        "app=codex".to_string(),
        "state=idle:id=".to_string(),
        "state=idle:id=a b".to_string(),
        "state=idle:msg=ab-_".to_string(),
        format!("state=idle:msg={}", b64("a\nb")),
        format!("state=idle:msg={}", b64("a\u{1b}b")),
        "state=idle:msg=/w==".to_string(),
        format!("state=idle:msg={long_msg}"),
        format!("state=idle:title={long_title}"),
        format!("state=idle:app={}", "a".repeat(33)),
        format!("state=idle:{}=1", "a".repeat(17)),
        format!("state=idle:app={}", "a".repeat(5000)),
    ];
    for body in cases {
        let text = format!("\x1b]7501;{body}\x07");
        assert!(reports(text.as_bytes()).is_empty(), "{body:.60}");
    }
    // The limits themselves are fine.
    let ok = format!(
        "\x1b]7501;state=idle:app={}:msg={}:title={}\x07",
        "a".repeat(32),
        b64(&"a".repeat(2048)),
        b64(&"b".repeat(192))
    );
    assert_eq!(reports(ok.as_bytes()).len(), 1);
}

#[test]
fn a_sequence_over_4096_bytes_is_dropped_and_the_next_one_read() {
    let pad = "a".repeat(4096);
    let text = format!("\x1b]7501;state=idle:x={pad}\x07\x1b]7501;state=done\x07");
    let got = reports(text.as_bytes());
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].state, Some(ReportState::Done));
    // Exactly 4096 bytes is still read.
    let fill = "a".repeat(4096 - "7501;state=idle:x=".len());
    let exact = format!("\x1b]7501;state=idle:x={fill}\x07");
    assert_eq!(reports(exact.as_bytes()).len(), 1);
}

#[test]
fn other_strings_and_aborted_sequences_report_nothing() {
    assert!(reports(b"\x1bP7501;state=idle\x07").is_empty());
    assert!(reports(b"\x1b]7500;state=idle\x07").is_empty());
    assert!(reports(b"\x1b]7501;state=idle\x18").is_empty());
    assert!(reports(b"\x1b]7501;state=idle\x1bxy\x07").is_empty());
    // An aborted report does not swallow the next one.
    assert_eq!(
        reports(b"\x1b]7501;state=idle\x1b]7501;state=done\x07").len(),
        1
    );
    assert!(reports(b"\x1b]7501;state=idle").is_empty());
}

#[test]
fn prompts_and_ris_are_signals_decstr_is_not() {
    let mut e = fresh(20, 5);
    e.feed(b"\x1b]133;A\x07\x1b]133;A;k=1\x1b\\\x1b]133;B\x07\x1b]133;C\x07");
    assert_eq!(e.take_signals(), vec![Signal::Prompt, Signal::Prompt]);
    e.feed(b"\x1b[!p");
    assert_eq!(e.take_signals(), vec![]);
    e.feed(b"\x1b]7501;state=working\x07\x1bc");
    let signals = e.take_signals();
    assert_eq!(signals.len(), 2);
    assert_eq!(signals[1], Signal::Ris);
}
#[test]
fn a_report_does_not_change_the_grid() {
    let mut e = fresh(20, 3);
    e.feed(b"ab\x1b]7501;state=working\x07cd");
    let mut plain = fresh(20, 3);
    plain.feed(b"abcd");
    assert_eq!(e.snapshot(), plain.snapshot());
}

#[test]
fn tone_is_parsed_and_an_unknown_value_is_ignored() {
    let got = reports(b"\x1b]7501;state=done:tone=warning\x07");
    assert_eq!(got[0].state, Some(ReportState::Done));
    assert_eq!(got[0].tone.as_deref(), Some("warning"));
    for body in ["tone=error", "tone=", "tone=Warning"] {
        let got = reports(format!("\x1b]7501;state=done:{body}\x07").as_bytes());
        assert_eq!(got[0].state, Some(ReportState::Done), "{body}");
        assert_eq!(got[0].tone, None, "{body}");
    }
}
