use super::*;
use serde_json::json;

fn env<'a>(vars: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
    move |name| {
        vars.iter()
            .find(|(k, _)| *k == name)
            .map(|(_, v)| v.to_string())
    }
}

#[test]
fn the_override_wins_on_every_platform_unless_empty() {
    let set = [(DIR_ENV, "/x/events"), ("HOME", "/home/u")];
    for windows in [false, true] {
        assert_eq!(
            shared_dir(env(&set), windows),
            Some(PathBuf::from("/x/events"))
        );
    }
    let empty = [(DIR_ENV, ""), ("HOME", "/home/u")];
    assert_eq!(
        shared_dir(env(&empty), false),
        Some(
            Path::new("/home/u")
                .join(".local/state")
                .join("agent-events")
        )
    );
}

#[test]
fn xdg_state_home_is_used_when_set_to_an_absolute_path() {
    let vars = [("XDG_STATE_HOME", "/state"), ("HOME", "/home/u")];
    assert_eq!(
        shared_dir(env(&vars), false),
        Some(Path::new("/state").join("agent-events"))
    );
}

#[test]
fn without_xdg_state_home_linux_and_macos_use_dot_local_state() {
    // macOS has the rule of Linux (owner decision on GravionLabs/agent-events#89).
    let want = Some(Path::new("/Users/u").join(".local/state/agent-events"));
    for xdg in [None, Some(""), Some("relative")] {
        let mut vars = vec![("HOME", "/Users/u"), ("LOCALAPPDATA", "C:\\l")];
        vars.extend(xdg.map(|v| ("XDG_STATE_HOME", v)));
        assert_eq!(shared_dir(env(&vars), false), want, "{xdg:?}");
    }
    assert_eq!(shared_dir(env(&[]), false), None);
}

#[test]
fn windows_uses_local_app_data() {
    let vars = [
        ("LOCALAPPDATA", r"C:\Users\u\AppData\Local"),
        ("XDG_STATE_HOME", "/state"),
    ];
    assert_eq!(
        shared_dir(env(&vars), true),
        Some(Path::new(r"C:\Users\u\AppData\Local").join("agent-events"))
    );
    assert_eq!(shared_dir(env(&[("HOME", "/h")]), true), None);
}

#[test]
fn a_linked_worktree_shares_the_legacy_directory_of_its_main_checkout() {
    let guard = tempfile::tempdir().unwrap();
    let main = guard.path().join("main");
    let linked = guard.path().join("linked");
    std::fs::create_dir(&main).unwrap();
    let git = |args: &[&str]| {
        crate::git::run_bytes(&main.to_string_lossy(), args).unwrap();
    };
    git(&["init", "-q"]);
    git(&[
        "-c",
        "user.name=t",
        "-c",
        "user.email=t@example.com",
        "-c",
        "commit.gpgsign=false",
        "commit",
        "-q",
        "--allow-empty",
        "-m",
        "x",
    ]);
    git(&["worktree", "add", "-q", &linked.to_string_lossy()]);
    let a = RepoRef::of(&main.to_string_lossy()).unwrap();
    let b = RepoRef::of(&linked.to_string_lossy()).unwrap();
    assert_eq!(legacy_dir(&a.common), legacy_dir(&b.common));
    assert_eq!(
        legacy_dir(&a.common),
        main.canonicalize().unwrap().join(".git").join(EVENTS_PATH)
    );
    assert_ne!(a.top, b.top);
    assert_eq!(a.name, b.name);
    assert_eq!(
        RepoRef::of(&guard.path().join("gone").to_string_lossy()),
        None
    );
}

#[test]
fn the_repository_name_is_the_path_of_the_remote_or_the_directory() {
    let common = Path::new("/src/api/.git");
    for url in [
        "https://github.com/o/r.git",
        "https://github.com/o/r/",
        "git@github.com:o/r.git",
        "ssh://git@example.com:22/o/r.git",
        "host:o/r",
    ] {
        assert_eq!(repo_name(Some(url), common), "o/r", "{url}");
    }
    for url in [
        None,
        Some("/srv/r.git"),
        Some("file:///srv/r.git"),
        Some(r"C:\srv\r.git"),
        Some("../r"),
    ] {
        assert_eq!(repo_name(url, common), "/src/api", "{url:?}");
    }
    assert_eq!(repo_name(None, Path::new("/srv/r.git")), "/srv/r.git");
}

#[test]
fn the_head_is_the_subject_and_origin_of_the_first_line() {
    let guard = tempfile::tempdir().unwrap();
    let file = guard.path().join("r.jsonl");
    let line = r#"{"v":1,"seq":1,"type":"run.started","subject":{"kind":"k","id":"1"},"origin":{"repo":"o/r","branch":"","worktree":"/w"}}"#;
    std::fs::write(&file, line).unwrap();
    let mut heads = Heads::default();
    let len = line.len() as u64;
    assert_eq!(heads.get(&file, len), None, "incomplete");
    std::fs::write(&file, format!("{line}\n")).unwrap();
    let head = heads.get(&file, len + 1).unwrap();
    assert_eq!(head.subject, Some(("k".into(), "1".into())));
    assert_eq!(
        head.origin,
        Some(Origin {
            repo: Some("o/r".into()),
            worktree: Some("/w".into()),
            ..Origin::default()
        })
    );
    std::fs::write(&file, "{\"type\":\"progress\"}\n").unwrap();
    assert_eq!(first_head(&file), Some(Head::default()));
}

fn repo(common: &str, name: &str) -> RepoRef {
    RepoRef {
        top: common.into(),
        common: common.into(),
        name: name.into(),
    }
}

#[test]
fn a_run_belongs_to_the_worktree_then_the_name_and_without_origin_to_its_terminal() {
    let repos = [repo("/a", "o/r"), repo("/b", "o/r"), repo("/c", "o/c")];
    let of = |dir: &str| match dir {
        "/b/wt" | "/b/term" => Some(repo("/b", "o/r")),
        "/x" => Some(repo("/x", "o/x")),
        _ => None,
    };
    let origin = |v: Value| Origin::of(&json!({ "origin": v }));
    let at = |o: &Option<Origin>, terminal: Option<&str>| {
        attribute(o.as_ref(), &repos, of, || terminal.map(str::to_owned))
    };
    assert_eq!(
        at(&origin(json!({"worktree":"/b/wt","repo":"o/r"})), None),
        Some(1)
    );
    // A removed worktree: by the name.
    assert_eq!(
        at(&origin(json!({"worktree":"/gone","repo":"o/c"})), None),
        Some(2)
    );
    assert_eq!(at(&origin(json!({"repo":"o/r"})), None), Some(0));
    // Another repository, whatever its terminal.
    assert_eq!(
        at(
            &origin(json!({"worktree":"/x","repo":"o/x"})),
            Some("/b/term")
        ),
        None
    );
    assert_eq!(at(&origin(json!({"host":"h"})), Some("/b/term")), None);
    assert_eq!(at(&None, Some("/b/term")), Some(1));
    assert_eq!(at(&None, Some("/x")), None);
    assert_eq!(at(&None, None), None);
}

#[test]
fn a_repository_is_asked_again_only_after_the_ttl() {
    let mut cache = RepoCache::default();
    let t0 = Instant::now();
    let mut calls = 0;
    for at in [t0, t0 + Duration::from_secs(1), t0 + RESOLVE_TTL] {
        cache.get("/a", at, |_| {
            calls += 1;
            None
        });
    }
    assert_eq!(calls, 2);
}
