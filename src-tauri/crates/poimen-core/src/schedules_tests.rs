use super::*;
use serde_json::json;

fn ms(text: &str) -> u64 {
    u64::try_from(text.parse::<Timestamp>().unwrap().as_millisecond()).unwrap()
}

/// An absolute directory on every platform.
fn repo() -> String {
    std::env::temp_dir().join("repo").display().to_string()
}

fn config(schedules: Value) -> Value {
    json!({
        "tasks": {"modes": [
            {"name": "loop", "command": ["deliver-loop", "{repo}"], "worktree": false},
            {"name": "agent"},
            {"name": "tree", "command": "make"},
            {"name": "nr", "command": ["x", "--issue={nr}"], "worktree": false},
            {"name": "title", "command": "x {title}", "worktree": false},
            {"name": "url", "command": ["x", "{url}"], "worktree": false},
            {"name": "branch", "command": ["x", "{branch}"], "worktree": false},
        ]},
        "schedules": schedules,
    })
}

fn one(mode: &str) -> Value {
    json!({"cron": "0 9 * * *", "timezone": "Europe/Zurich", "dir": repo(), "mode": mode})
}

fn parsed(root: &Value) -> (Schedules, Vec<String>) {
    let mut warnings = Vec::new();
    let out = parse(root, None, &mut warnings);
    (out, warnings)
}

#[test]
fn a_command_mode_without_issue_and_worktree_is_scheduled_with_the_defaults() {
    let mut item = one("loop");
    item["precheck"] = json!(["test", "-f", "x"]);
    item["cron"] = json!(" 0  9 * *  * ");
    let (s, warnings) = parsed(&config(json!([item])));
    assert!(warnings.is_empty(), "{warnings:?}");
    assert!(!s.paused);
    assert_eq!(s.max_runs_per_day, 24);
    let s = &s.list[0];
    assert_eq!(s.cron, "0 9 * * *");
    assert!(s.enabled);
    assert_eq!(s.precheck.as_deref().unwrap(), ["test", "-f", "x"]);
    assert_eq!(s.key, format!("{}|loop|0 9 * * *|Europe/Zurich", repo()));
    assert_eq!(
        s.command,
        CommandSpec::Argv(vec!["deliver-loop".into(), "{repo}".into()])
    );
}

#[test]
fn modes_that_cannot_be_scheduled_warn_naming_the_schedule() {
    for (mode, why) in [
        ("agent", "has no command"),
        ("tree", "worktree"),
        ("nr", "{nr}"),
        ("title", "{title}"),
        ("url", "{url}"),
        ("branch", "{branch}"),
        ("missing", "no mode \"missing\""),
    ] {
        let (s, warnings) = parsed(&config(json!([one(mode)])));
        assert!(s.list.is_empty(), "{mode}");
        assert_eq!(warnings.len(), 1, "{mode}: {warnings:?}");
        assert!(
            warnings[0].starts_with(&format!("schedules[0] (mode \"{mode}\" in {})", repo())),
            "{}",
            warnings[0]
        );
        assert!(warnings[0].contains(why), "{}", warnings[0]);
    }
}

#[test]
fn invalid_entries_warn_and_the_valid_ones_stay() {
    let with = |key: &str, value: Value| {
        let mut item = one("loop");
        item[key] = value;
        item
    };
    let bad = [
        json!(1),
        with("cron", json!("0 9 * *")),
        with("cron", json!("0 0 9 * * *")),
        with("cron", json!("61 9 * * *")),
        with("timezone", json!("Mars/Base")),
        with("timezone", json!("")),
        with("dir", json!("relative/dir")),
        with("precheck", json!("make")),
        with("precheck", json!([])),
        with("precheck", json!([1])),
        with("enabled", json!("no")),
    ];
    for item in bad {
        let (s, warnings) = parsed(&config(json!([item.clone(), one("loop")])));
        assert_eq!(s.list.len(), 1, "{item}");
        assert_eq!(warnings.len(), 1, "{item}: {warnings:?}");
        assert!(warnings[0].starts_with("schedules[0]"), "{}", warnings[0]);
    }
    let (s, warnings) = parsed(&config(json!([one("loop"), one("loop")])));
    assert_eq!(s.list.len(), 1);
    assert!(warnings[0].contains("twice"));
    let (s, warnings) = parsed(&config(json!({})));
    assert!(s.list.is_empty());
    assert_eq!(warnings, ["schedules must be an array"]);
}

#[test]
fn the_limits_come_from_triggers_and_an_unclear_pause_pauses() {
    let of = |triggers: Value| {
        let (s, w) = parsed(&json!({ "triggers": triggers }));
        (s.paused, s.max_runs_per_day, w.len())
    };
    assert_eq!(
        of(json!({"paused": true, "maxRunsPerDay": 96})),
        (true, 96, 0)
    );
    assert_eq!(of(json!({"maxRunsPerDay": 1})), (false, 1, 0));
    assert_eq!(of(json!({"maxRunsPerDay": 0})), (false, 24, 1));
    assert_eq!(of(json!({"maxRunsPerDay": 97})), (false, 24, 1));
    assert_eq!(of(json!({"paused": "true"})), (true, 24, 1));
    assert_eq!(of(json!([])), (true, 24, 1));
}

#[test]
fn a_leading_tilde_is_the_home() {
    let mut item = one("loop");
    item["dir"] = json!("~/work");
    let home = std::env::temp_dir();
    let mut warnings = Vec::new();
    let s = parse(&config(json!([item])), Some(&home), &mut warnings);
    assert_eq!(s.list[0].dir, home.join("work").to_string_lossy());
}

#[test]
fn due_times_follow_the_time_zone() {
    let t = Timing::parse("0 9 * * *", "Europe/Zurich").unwrap();
    assert_eq!(
        t.next_after(ms("2026-01-10T00:00:00Z")),
        Some(ms("2026-01-10T08:00:00Z"))
    );
    // A due time is not "after" itself.
    assert_eq!(
        t.next_after(ms("2026-01-10T08:00:00Z")),
        Some(ms("2026-01-11T08:00:00Z"))
    );
    assert_eq!(
        t.latest_between(ms("2026-01-08T00:00:00Z"), ms("2026-01-10T08:00:00Z")),
        Some(ms("2026-01-10T08:00:00Z"))
    );
    assert_eq!(
        t.latest_between(ms("2026-01-10T08:00:00Z"), ms("2026-01-11T07:59:59Z")),
        None
    );
}

#[test]
fn a_skipped_local_time_fires_after_the_gap_and_a_repeated_one_once() {
    // Zurich 2026: 02:00 jumps to 03:00 on March 29, 03:00 falls back to 02:00 on October 25.
    let t = Timing::parse("30 2 * * *", "Europe/Zurich").unwrap();
    let gap = ms("2026-03-29T01:00:00Z");
    assert_eq!(t.next_after(ms("2026-03-28T12:00:00Z")), Some(gap));
    assert_eq!(t.next_after(gap), Some(ms("2026-03-30T00:30:00Z")));
    let first = ms("2026-10-25T00:30:00Z");
    assert_eq!(t.next_after(ms("2026-10-24T12:00:00Z")), Some(first));
    assert_eq!(t.next_after(first), Some(ms("2026-10-26T01:30:00Z")));
    // Also when asked during the second pass of the repeated hour.
    assert_eq!(
        t.next_after(ms("2026-10-25T01:15:00Z")),
        Some(ms("2026-10-26T01:30:00Z"))
    );
}

#[test]
fn an_expression_that_never_matches_has_no_due_time() {
    let t = Timing::parse("0 0 30 2 *", "UTC").unwrap();
    assert_eq!(t.next_after(ms("2026-01-01T00:00:00Z")), None);
}

#[test]
fn the_switches_are_written_into_the_config() {
    let mut other = one("loop");
    other["cron"] = json!("0 10 * * *");
    let text = config(json!([one("loop"), other])).to_string();
    let key = format!("{}|loop|0 10 * * *|Europe/Zurich", repo());
    let off = set_enabled_text(&text, None, &key, false).unwrap();
    let (s, warnings) = parsed(&serde_json::from_str(&off).unwrap());
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(
        s.list.iter().map(|s| s.enabled).collect::<Vec<_>>(),
        [true, false]
    );
    assert!(set_enabled_text(&text, None, "nope", false).is_err());

    let paused = set_paused_text(&text, true).unwrap();
    assert!(parsed(&serde_json::from_str(&paused).unwrap()).0.paused);
    let again = set_paused_text(&paused, false).unwrap();
    assert!(!parsed(&serde_json::from_str(&again).unwrap()).0.paused);
}
