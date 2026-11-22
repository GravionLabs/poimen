use super::*;
use poimen_protocol::encode_bytes;

#[test]
fn the_host_binary_is_looked_for_next_to_the_server() {
    let exe = Path::new("/opt/poimen/poimen-server");
    assert_eq!(
        host_beside(exe, false, Channel::Stable),
        PathBuf::from("/opt/poimen/poimen-pty-host")
    );
    assert_eq!(
        host_beside(exe, false, Channel::Rc),
        PathBuf::from("/opt/poimen/poimen-pty-host-rc")
    );
    assert_eq!(
        host_beside(Path::new(r"C:\p\poimen-server.exe"), true, Channel::Stable)
            .file_name()
            .unwrap(),
        "poimen-pty-host.exe"
    );
    assert_eq!(
        host_beside(Path::new(""), false, Channel::Stable),
        PathBuf::from("poimen-pty-host")
    );
}

#[test]
fn the_endpoint_is_in_the_hosts_directory_next_to_the_socket() {
    assert_eq!(
        endpoint_for(Path::new("/run/u/poimen/server.sock"), "t12"),
        PathBuf::from("/run/u/poimen/hosts/t12")
    );
}

static NO_ENV: std::collections::BTreeMap<String, String> = std::collections::BTreeMap::new();

fn spec<'a>(command: &'a [String], term: Option<&'a str>) -> Spec<'a> {
    Spec {
        terminal_id: "t3",
        log: Path::new("/run/server.log"),
        socket: Path::new("/run/s.sock"),
        command,
        cwd: "/w",
        cols: 100,
        rows: 30,
        term,
        locale_set: false,
        env: &NO_ENV,
    }
}

#[test]
fn the_start_line_carries_the_command_and_poimens_environment() {
    let command = vec!["sh".to_string(), "-l".to_string()];
    let start = start_line(&spec(&command, Some("xterm-kitty")), PathBuf::from("/e"));
    assert_eq!(start.endpoint, PathBuf::from("/e"));
    assert_eq!(start.spawn.argv, command);
    assert_eq!(start.spawn.cwd, "/w");
    assert_eq!((start.spawn.cols, start.spawn.rows), (100, 30));
    let mut expected = std::collections::BTreeMap::from([
        ("POIMEN_SOCKET".to_string(), "/run/s.sock".to_string()),
        ("POIMEN_TERMINAL_ID".to_string(), "t3".to_string()),
    ]);
    if let Some(ctype) = default_lc_ctype() {
        expected.insert("LC_CTYPE".to_string(), ctype.to_string());
    }
    assert_eq!(start.spawn.env, expected);
    assert_eq!(start.orphan_timeout_ms, 86_400_000);
}

#[test]
fn extra_env_is_added_but_cannot_override_poimens_variables() {
    let command = vec!["sh".to_string()];
    let env = std::collections::BTreeMap::from([
        (
            "CLAUDE_CODE_DISABLE_ALTERNATE_SCREEN".to_string(),
            "1".to_string(),
        ),
        ("POIMEN_TERMINAL_ID".to_string(), "evil".to_string()),
        ("POIMEN_SOCKET".to_string(), "/evil".to_string()),
    ]);
    let start = start_line(
        &Spec {
            env: &env,
            ..spec(&command, Some("xterm"))
        },
        PathBuf::from("/e"),
    );
    let got = &start.spawn.env;
    assert_eq!(
        got.get("CLAUDE_CODE_DISABLE_ALTERNATE_SCREEN")
            .map(String::as_str),
        Some("1")
    );
    assert_eq!(
        got.get("POIMEN_TERMINAL_ID").map(String::as_str),
        Some("t3")
    );
    assert_eq!(
        got.get("POIMEN_SOCKET").map(String::as_str),
        Some("/run/s.sock")
    );
}

#[test]
fn a_missing_or_dumb_term_becomes_xterm_256color() {
    let command = vec!["sh".to_string()];
    for term in [None, Some(""), Some("dumb")] {
        let start = start_line(&spec(&command, term), PathBuf::from("/e"));
        assert_eq!(
            start.spawn.env.get("TERM").map(String::as_str),
            Some("xterm-256color"),
            "{term:?}"
        );
    }
}

#[test]
fn a_server_without_a_locale_gives_its_children_a_utf8_ctype() {
    let command = vec!["sh".to_string()];
    let start = start_line(&spec(&command, None), PathBuf::from("/e"));
    let expected = if cfg!(target_os = "macos") {
        Some("UTF-8")
    } else if cfg!(unix) {
        Some("C.UTF-8")
    } else {
        None
    };
    assert_eq!(
        start.spawn.env.get("LC_CTYPE").map(String::as_str),
        expected
    );
}

#[test]
fn each_locale_variable_of_the_server_suppresses_the_default() {
    for name in ["LC_ALL", "LC_CTYPE", "LANG"] {
        let set = locale_set(|k| (k == name).then(|| "de_DE.ISO-8859-1".to_string()));
        assert!(set, "{name} counts as a locale");
        let command = vec!["sh".to_string()];
        let start = start_line(
            &Spec {
                locale_set: set,
                ..spec(&command, None)
            },
            PathBuf::from("/e"),
        );
        assert_eq!(start.spawn.env.get("LC_CTYPE"), None, "{name}");
    }
    assert!(!locale_set(|_| None), "no variable is no locale");
}

#[test]
fn an_empty_locale_variable_counts_as_unset() {
    for name in ["LC_ALL", "LC_CTYPE", "LANG"] {
        let set = locale_set(|k| (k == name).then(String::new));
        assert!(!set, "an empty {name} is unset");
    }
}

#[test]
fn a_missing_host_binary_is_a_clear_error() {
    let host = PtyHost::Binary(PathBuf::from("/nonexistent/poimen-pty-host"));
    let start = start_line(&spec(&["sh".into()], None), PathBuf::from("/e"));
    let err = launch(&host, &start, None).unwrap_err();
    assert_eq!(
        err,
        "poimen-pty-host is missing: /nonexistent/poimen-pty-host (it ships next to poimen-server)"
    );
}

fn reader(read: u64) -> (Feed, mpsc::Receiver<Chunk>) {
    let (tx, rx) = mpsc::channel();
    let feed = Feed {
        endpoint: PathBuf::from("/e"),
        log: PathBuf::from("/e.server.log"),
        read: Arc::new(AtomicU64::new(read)),
        tx,
        queued: Arc::default(),
        replay_until: 0,
    };
    (feed, rx)
}

fn output(seq: u64, text: &str) -> String {
    String::from_utf8(to_line(&HostMessage::Output {
        seq,
        bytes: encode_bytes(text.as_bytes()),
    }))
    .unwrap()
}

fn data(rx: &mpsc::Receiver<Chunk>) -> Vec<String> {
    rx.try_iter()
        .map(|c| match c {
            Chunk::Data(d) => String::from_utf8(d).unwrap(),
            Chunk::Replay(d) => format!("replay {}", String::from_utf8(d).unwrap()),
            Chunk::End(code) => format!("end {code:?}"),
        })
        .collect()
}

#[test]
fn records_are_read_strictly_by_seq_and_none_twice() {
    let (reader, rx) = reader(0);
    let lines = [
        output(1, "a"),
        output(2, "b"),
        output(2, "b"),
        output(1, "a"),
        output(3, "c"),
    ]
    .concat();
    let ended = reader.pump(&mut lines.as_bytes());
    assert_eq!(ended, Ended::Lost);
    assert_eq!(data(&rx), ["a", "b", "c"]);
    assert_eq!(reader.read.load(Ordering::SeqCst), 3);
}

fn gap(from: u64, to: u64) -> String {
    String::from_utf8(to_line(&HostMessage::Gap { from, to })).unwrap()
}

#[test]
fn a_reattached_connection_goes_on_after_the_last_seq_read_and_marks_the_gap() {
    let (reader, rx) = reader(5);
    let lines = [
        output(4, "old"),
        output(5, "old"),
        gap(6, 7),
        output(8, "new"),
    ]
    .concat();
    reader.pump(&mut lines.as_bytes());
    assert_eq!(
        data(&rx),
        [
            "\r\n[poimen: earlier output lost, 2 records left the host's buffer]\r\n",
            "new"
        ]
    );
    assert_eq!(reader.read.load(Ordering::SeqCst), 8);
}

#[test]
fn a_gap_already_behind_the_last_record_read_leaves_no_marker() {
    let (reader, rx) = reader(9);
    let lines = [gap(1, 7), output(10, "x")].concat();
    reader.pump(&mut lines.as_bytes());
    assert_eq!(data(&rx), ["x"]);
}

#[test]
fn the_ring_of_an_adopted_host_is_a_replay_up_to_its_hello_and_live_after() {
    let (mut reader, rx) = reader(0);
    reader.replay_until = 3;
    let lines = [gap(1, 1), output(2, "b"), output(3, "c"), output(4, "d")].concat();
    reader.pump(&mut lines.as_bytes());
    assert_eq!(
        data(&rx),
        [
            "replay [poimen: earlier output lost, 1 record left the host's buffer]\r\n",
            "replay b",
            "replay c",
            "d"
        ]
    );
}

#[test]
fn exit_replaced_and_noise_end_or_pass_as_documented() {
    let (reader, rx) = reader(0);
    let lines = [
        "not json\n".to_string(),
        "{\"type\":\"title\",\"text\":\"x\"}\n".to_string(),
        output(1, "x"),
        "{\"type\":\"exit\",\"code\":7}\n".to_string(), // a host before #968
        output(2, "after"),
    ]
    .concat();
    assert_eq!(reader.pump(&mut lines.as_bytes()), Ended::Exit(Some(7)));
    assert_eq!(data(&rx), ["x"]);
    let replaced = String::from_utf8(to_line(&HostMessage::Replaced)).unwrap();
    assert_eq!(reader.pump(&mut replaced.as_bytes()), Ended::Replaced);
}
