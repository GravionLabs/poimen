use super::*;
use crate::tui::sidebar::{Model, Row};
use poimen_client::protocol::{Space, Terminal};
use poimen_core::settings::{RowToken, SidebarRows, SidebarSort};

fn repo_space(id: &str, name: &str, cwd: &str) -> Space {
    Space {
        id: id.into(),
        name: name.into(),
        cwd: cwd.into(),
        tabs: vec![],
        active_tab: None,
    }
}

fn term(cwd: &str) -> Terminal {
    serde_json::from_value(serde_json::json!({
        "id": "t1", "space_id": "s1", "name": "sh", "cwd": cwd,
        "status": "running", "exit_code": null, "cols": 80, "rows": 24
    }))
    .unwrap()
}

fn texts(row: &Row) -> String {
    row.lines
        .iter()
        .map(|l| {
            l.iter()
                .map(|s| s.text.as_str())
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect::<Vec<_>>()
        .join(" / ")
}

fn repo_ref(common: &str, worktree: bool) -> RepoRef {
    RepoRef {
        common_dir: common.into(),
        worktree,
        ..RepoRef::default()
    }
}

fn read_repo(dir: &str) -> Option<RepoRef> {
    (dir == "/w/a").then(|| RepoRef {
        branch: Some("feat/x".into()),
        dirty: Some(3),
        ahead: Some(2),
        behind: Some(1),
        ..repo_ref("/a/.git", false)
    })
}

fn read_pr(_dir: &str, branch: &str) -> Option<String> {
    (branch == "feat/x").then(|| "#7 ✓".to_owned())
}

fn readers() -> Readers {
    Readers {
        repo: read_repo,
        pr: read_pr,
    }
}

#[test]
fn the_cache_keeps_a_failed_directory_flat_and_serves_what_it_read() {
    let cache = RepoCache::with_readers(readers());
    let dirs = ["/w/a", "/remote/only"];
    let first = cache.resolve_now(dirs.iter().copied());
    assert_eq!(first.len(), 1);
    assert_eq!(first["/w/a"].pr.as_deref(), Some("#7 ✓"));
    assert_eq!(cache.resolve(dirs.iter().copied()), first);
}

fn git_rows(terminal_dir: Option<&str>, show_base: bool, on_base: bool) -> Vec<String> {
    let cache = RepoCache::with_readers(readers());
    let mut m = Model {
        spaces: vec![repo_space("s1", "w", "/w/a")],
        terminals: terminal_dir.map(|d| vec![term(d)]).unwrap_or_default(),
        show_base_branch: show_base,
        ..Model::default()
    };
    m.repos = m.resolve_repos_now(&cache);
    if on_base && let Some(r) = m.repos.get_mut("/w/a") {
        r.on_base = true;
    }
    let line = || {
        vec![
            ["name", "branch", "dirty", "sync", "pr"]
                .map(|n| RowToken {
                    token: n.to_owned(),
                    bold: false,
                    dim: false,
                })
                .to_vec(),
        ]
    };
    let config = SidebarRows {
        workspace: Some(line()),
        terminal: Some(line()),
        ..SidebarRows::default()
    };
    m.rows(&config, SidebarSort::Manual, None)
        .iter()
        .map(texts)
        .collect()
}

#[test]
fn a_space_row_shows_branch_dirty_sync_and_pr() {
    assert_eq!(git_rows(None, false, false), ["w feat/x ●3 ↑2 ↓1 #7 ✓"]);
}

#[test]
fn the_base_branch_is_hidden_unless_shown() {
    assert_eq!(git_rows(None, false, true), ["w ●3 ↑2 ↓1 #7 ✓"]);
    assert_eq!(git_rows(None, true, true), ["w feat/x ●3 ↑2 ↓1 #7 ✓"]);
}

#[test]
fn a_terminal_shows_git_tokens_only_where_it_differs_from_its_space() {
    // The same directory repeats nothing; a directory without repository (remote) shows nothing.
    assert_eq!(git_rows(Some("/w/a"), false, false)[1], "sh");
    assert_eq!(git_rows(Some("/remote/only"), false, false)[1], "sh");
}

#[test]
fn pull_requests_read_from_gh_json() {
    use serde_json::json;
    let open = |checks: serde_json::Value| json!([{"number": 9, "state": "OPEN", "statusCheckRollup": checks}]);
    let run = |status: &str, conclusion: &str| json!({"__typename": "CheckRun", "status": status, "conclusion": conclusion});
    let text = |v: serde_json::Value| pr_text(&v);
    assert_eq!(text(open(json!([]))).as_deref(), Some("#9"));
    let ok = run("COMPLETED", "SUCCESS");
    assert_eq!(text(open(json!([ok]))).as_deref(), Some("#9 ✓"));
    let pending = run("IN_PROGRESS", "");
    assert_eq!(text(open(json!([ok, pending]))).as_deref(), Some("#9 …"));
    let failed = run("COMPLETED", "FAILURE");
    assert_eq!(
        text(open(json!([pending, failed]))).as_deref(),
        Some("#9 ✗")
    );
    assert_eq!(
        text(json!([{"number": 3, "state": "MERGED"}])).as_deref(),
        Some("merged")
    );
    assert_eq!(text(json!([{"number": 3, "state": "CLOSED"}])), None);
    assert_eq!(text(json!([])), None);
}
