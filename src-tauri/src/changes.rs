//! Read-only changes of a worktree for the Changes view: the changed files against the base
//! branch (or `HEAD`) and, per file, the old and new content for the merge view. Only `git`
//! reads (argv, no shell, no fetch); works on every backend.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

use crate::git::{base_ref, run_bytes};

/// Largest file content sent to the merge view.
pub const MAX_CONTENT: usize = 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    /// Working tree (uncommitted included) against `git merge-base HEAD <base>`.
    Base,
    /// Working tree against `HEAD`.
    Uncommitted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Added,
    Modified,
    Deleted,
    Renamed,
    Untracked,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ChangedFile {
    pub path: String,
    /// The path before a rename.
    pub old_path: Option<String>,
    pub status: Status,
    pub additions: u32,
    pub deletions: u32,
    pub binary: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ChangeList {
    /// The base ref used (`HEAD` when the repository has no `origin` base).
    pub base: String,
    pub scope: Scope,
    /// The commit the old content comes from.
    pub from: String,
    pub files: Vec<ChangedFile>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum FileContent {
    Text { old: String, new: String },
    Binary,
    TooLarge,
}

/// The ref the scope compares against and the revision the old content is read from.
fn resolve(dir: &str, scope: Scope) -> (String, String) {
    if scope == Scope::Uncommitted {
        return ("HEAD".into(), "HEAD".into());
    }
    let Some(base) = base_ref(dir) else {
        return ("HEAD".into(), "HEAD".into());
    };
    // Without a merge base the comparison is against `HEAD`, and `base` says so.
    match run_bytes(dir, &["merge-base", "HEAD", &base])
        .ok()
        .map(|b| String::from_utf8_lossy(&b).trim().to_owned())
        .filter(|s| !s.is_empty())
    {
        Some(from) => (base, from),
        None => ("HEAD".into(), "HEAD".into()),
    }
}

fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

type NameStatus = (Status, String, Option<String>);

/// Parses `git diff --name-status -z -M`: (status, path, old path) per entry.
pub fn parse_name_status(out: &[u8]) -> Vec<NameStatus> {
    let mut tokens = out.split(|b| *b == 0).filter(|t| !t.is_empty());
    let mut list = Vec::new();
    while let Some(code) = tokens.next() {
        match code[0] {
            b'R' | b'C' => {
                let (Some(old), Some(new)) = (tokens.next(), tokens.next()) else {
                    break;
                };
                if code[0] == b'R' {
                    list.push((Status::Renamed, text(new), Some(text(old))));
                } else {
                    list.push((Status::Added, text(new), None));
                }
            }
            c => {
                let Some(path) = tokens.next() else { break };
                let status = match c {
                    b'A' => Status::Added,
                    b'D' => Status::Deleted,
                    _ => Status::Modified,
                };
                list.push((status, text(path), None));
            }
        }
    }
    list
}

/// Parses `git diff --numstat -z -M` into (additions, deletions, binary) by new path.
pub fn parse_numstat(out: &[u8]) -> HashMap<String, (u32, u32, bool)> {
    let mut tokens = out.split(|b| *b == 0).filter(|t| !t.is_empty());
    let mut map = HashMap::new();
    while let Some(head) = tokens.next() {
        let head = text(head);
        let mut parts = head.splitn(3, '\t');
        let (Some(a), Some(d), Some(path)) = (parts.next(), parts.next(), parts.next()) else {
            continue;
        };
        let path = if path.is_empty() {
            // Rename or copy: the old and the new path follow as separate entries.
            let _old = tokens.next();
            let Some(new) = tokens.next() else { break };
            text(new)
        } else {
            path.to_owned()
        };
        let binary = a == "-" && d == "-";
        map.insert(
            path,
            (a.parse().unwrap_or(0), d.parse().unwrap_or(0), binary),
        );
    }
    map
}

/// A file of the working tree, read without following symlinks (a link yields its target
/// path) and only up to `MAX_CONTENT` bytes: `Err(())` means too large, `Ok(None)` unreadable.
fn read_worktree_file(root: &str, path: &str) -> Result<Option<Vec<u8>>, ()> {
    use std::os::unix::ffi::OsStringExt;
    let full = Path::new(root).join(path);
    let Ok(meta) = std::fs::symlink_metadata(&full) else {
        return Ok(None);
    };
    if meta.file_type().is_symlink() {
        return Ok(std::fs::read_link(&full)
            .ok()
            .map(|t| t.into_os_string().into_vec()));
    }
    if !meta.is_file() {
        return Ok(None);
    }
    if meta.len() > MAX_CONTENT as u64 {
        return Err(());
    }
    Ok(std::fs::read(&full).ok())
}

fn count_lines(b: &[u8]) -> u32 {
    let newlines = b.iter().filter(|c| **c == b'\n').count();
    let partial = usize::from(b.last().is_some_and(|c| *c != b'\n'));
    u32::try_from(newlines + partial).unwrap_or(u32::MAX)
}

fn list_in(dir: &str, scope: Scope) -> Result<ChangeList, String> {
    let (base, from) = resolve(dir, scope);
    let name_status = run_bytes(dir, &["diff", "--name-status", "-z", "-M", &from, "--"])?;
    let numstat = parse_numstat(&run_bytes(
        dir,
        &["diff", "--numstat", "-z", "-M", &from, "--"],
    )?);
    let mut files: Vec<ChangedFile> = parse_name_status(&name_status)
        .into_iter()
        .map(|(status, path, old_path)| {
            let (additions, deletions, binary) = numstat.get(&path).copied().unwrap_or_default();
            ChangedFile {
                path,
                old_path,
                status,
                additions,
                deletions,
                binary,
            }
        })
        .collect();
    let untracked = run_bytes(dir, &["ls-files", "--others", "--exclude-standard", "-z"])?;
    for path in untracked.split(|b| *b == 0).filter(|t| !t.is_empty()) {
        let path = text(path);
        let content = read_worktree_file(dir, &path)
            .ok()
            .flatten()
            .unwrap_or_default();
        let binary = content.contains(&0);
        files.push(ChangedFile {
            additions: if binary { 0 } else { count_lines(&content) },
            path,
            old_path: None,
            status: Status::Untracked,
            deletions: 0,
            binary,
        });
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(ChangeList {
        base,
        scope,
        from,
        files,
    })
}

fn file_in(dir: &str, scope: Scope, path: &str) -> Result<FileContent, String> {
    let list = list_in(dir, scope)?;
    let file = list
        .files
        .iter()
        .find(|f| f.path == path)
        .ok_or_else(|| format!("{path} is not a changed file"))?;
    if file.binary {
        return Ok(FileContent::Binary);
    }
    let old = match file.status {
        Status::Added | Status::Untracked => Vec::new(),
        _ => {
            let old = file.old_path.as_deref().unwrap_or(&file.path);
            run_bytes(dir, &["show", &format!("{}:{old}", list.from)])?
        }
    };
    let new = if file.status == Status::Deleted {
        Vec::new()
    } else {
        match read_worktree_file(dir, &file.path) {
            Ok(Some(b)) => b,
            Ok(None) => return Err(format!("{} could not be read", file.path)),
            Err(()) => return Ok(FileContent::TooLarge),
        }
    };
    if old.len() > MAX_CONTENT || new.len() > MAX_CONTENT {
        return Ok(FileContent::TooLarge);
    }
    if old.contains(&0) || new.contains(&0) {
        return Ok(FileContent::Binary);
    }
    match (String::from_utf8(old), String::from_utf8(new)) {
        (Ok(old), Ok(new)) => Ok(FileContent::Text { old, new }),
        _ => Ok(FileContent::Binary),
    }
}

/// `dir` as in `create_space` (an existing absolute directory) and inside a git repository;
/// returns the repository's top level, the base of every path and git call here (git prints
/// root-relative paths for `diff` and cwd-relative ones for `ls-files`).
fn validate(dir: &str, home: Option<&Path>) -> Result<String, String> {
    let dir = crate::spaces::resolve_dir(dir, home)?;
    crate::git::toplevel(&dir).ok_or_else(|| format!("{dir} is not in a git repository"))
}

pub fn list(dir: &str, scope: Scope, home: Option<&Path>) -> Result<ChangeList, String> {
    list_in(&validate(dir, home)?, scope)
}

pub fn file(
    dir: &str,
    scope: Scope,
    path: &str,
    home: Option<&Path>,
) -> Result<FileContent, String> {
    file_in(&validate(dir, home)?, scope, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU32, Ordering};

    static N: AtomicU32 = AtomicU32::new(0);

    fn run(dir: &Path, args: &[&str]) {
        let out = crate::gui_path::command("git")
            .arg("-C")
            .arg(dir)
            .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    /// A bare `origin` with `main` (and `release/next` when asked) and a clone `app` on a
    /// feature branch.
    struct Fixture {
        base: PathBuf,
        app: PathBuf,
    }

    impl Fixture {
        fn new(release_next: bool) -> Self {
            let base = std::env::temp_dir().join(format!(
                "poimen-changes-{}-{}",
                std::process::id(),
                N.fetch_add(1, Ordering::SeqCst)
            ));
            let _ = std::fs::remove_dir_all(&base);
            std::fs::create_dir_all(base.join("seed")).unwrap();
            let base = base.canonicalize().unwrap();
            let seed = base.join("seed");
            let origin = base.join("origin.git");
            run(&base, &["init", "--bare", "-b", "main", "origin.git"]);
            run(&seed, &["init", "-b", "main"]);
            std::fs::write(seed.join("keep.txt"), "one\ntwo\nthree\n").unwrap();
            std::fs::write(seed.join("old.txt"), "rename me\nline 2\nline 3\nline 4\n").unwrap();
            std::fs::write(seed.join("gone.txt"), "bye\n").unwrap();
            run(&seed, &["add", "."]);
            run(&seed, &["commit", "-m", "init"]);
            run(
                &seed,
                &["remote", "add", "origin", origin.to_str().unwrap()],
            );
            run(&seed, &["push", "origin", "main"]);
            if release_next {
                run(&seed, &["checkout", "-b", "release/next"]);
                std::fs::write(seed.join("next.txt"), "next\n").unwrap();
                run(&seed, &["add", "."]);
                run(&seed, &["commit", "-m", "next"]);
                run(&seed, &["push", "origin", "release/next"]);
            }
            run(&base, &["clone", origin.to_str().unwrap(), "app"]);
            let app = base.join("app");
            run(&app, &["checkout", "-b", "feat/x"]);
            Self { base, app }
        }

        fn dir(&self) -> String {
            self.app.to_string_lossy().into_owned()
        }

        fn files(&self, scope: Scope) -> HashMap<String, ChangedFile> {
            list_in(&self.dir(), scope)
                .unwrap()
                .files
                .into_iter()
                .map(|f| (f.path.clone(), f))
                .collect()
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.base);
        }
    }

    #[test]
    fn committed_and_uncommitted_changes_against_the_base() {
        let f = Fixture::new(true);
        // The feature branch starts at origin/main, behind release/next.
        std::fs::write(f.app.join("keep.txt"), "one\nTWO\nthree\nfour\n").unwrap();
        run(&f.app, &["commit", "-am", "edit"]);
        std::fs::write(f.app.join("keep.txt"), "one\nTWO\nthree\nfour\nfive\n").unwrap();
        std::fs::write(f.app.join("new.txt"), "n\n").unwrap();
        run(&f.app, &["add", "new.txt"]);

        let list = list_in(&f.dir(), Scope::Base).unwrap();
        assert_eq!(list.base, "origin/release/next");
        let by: HashMap<_, _> = list.files.iter().map(|f| (f.path.as_str(), f)).collect();
        // next.txt lives on the base only and is not a change of this branch.
        assert_eq!(by.len(), 2, "{by:?}");
        assert_eq!(by["keep.txt"].status, Status::Modified);
        assert_eq!((by["keep.txt"].additions, by["keep.txt"].deletions), (3, 1));
        assert_eq!(by["new.txt"].status, Status::Added);
    }

    #[test]
    fn uncommitted_scope_ignores_commits() {
        let f = Fixture::new(true);
        std::fs::write(f.app.join("keep.txt"), "changed\n").unwrap();
        run(&f.app, &["commit", "-am", "edit"]);
        std::fs::write(f.app.join("old.txt"), "dirty\n").unwrap();
        let files = f.files(Scope::Uncommitted);
        assert_eq!(files.keys().collect::<Vec<_>>(), ["old.txt"]);
        let list = list_in(&f.dir(), Scope::Uncommitted).unwrap();
        assert_eq!((list.base.as_str(), list.from.as_str()), ("HEAD", "HEAD"));
    }

    #[test]
    fn rename_delete_and_untracked() {
        let f = Fixture::new(true);
        run(&f.app, &["mv", "old.txt", "renamed.txt"]);
        std::fs::remove_file(f.app.join("gone.txt")).unwrap();
        std::fs::write(f.app.join("loose.txt"), "a\nb").unwrap();
        let files = f.files(Scope::Base);
        let r = &files["renamed.txt"];
        assert_eq!(r.status, Status::Renamed);
        assert_eq!(r.old_path.as_deref(), Some("old.txt"));
        assert_eq!(files["gone.txt"].status, Status::Deleted);
        assert_eq!(files["gone.txt"].deletions, 1);
        let u = &files["loose.txt"];
        assert_eq!(
            (u.status, u.additions, u.binary),
            (Status::Untracked, 2, false)
        );

        let same = "rename me\nline 2\nline 3\nline 4\n".to_owned();
        assert_eq!(
            file_in(&f.dir(), Scope::Base, "renamed.txt").unwrap(),
            FileContent::Text {
                old: same.clone(),
                new: same
            }
        );
        assert_eq!(
            file_in(&f.dir(), Scope::Base, "gone.txt").unwrap(),
            FileContent::Text {
                old: "bye\n".into(),
                new: String::new()
            }
        );
        assert_eq!(
            file_in(&f.dir(), Scope::Base, "loose.txt").unwrap(),
            FileContent::Text {
                old: String::new(),
                new: "a\nb".into()
            }
        );
    }

    #[test]
    fn binary_and_large_files_come_back_without_content() {
        let f = Fixture::new(true);
        std::fs::write(f.app.join("img.bin"), [0u8, 1, 2, 3]).unwrap();
        std::fs::write(f.app.join("big.txt"), vec![b'x'; MAX_CONTENT + 1]).unwrap();
        run(&f.app, &["add", "img.bin"]);
        let files = f.files(Scope::Base);
        assert!(files["img.bin"].binary);
        assert_eq!(files["img.bin"].status, Status::Added);
        assert!(!files["big.txt"].binary);
        assert_eq!(
            file_in(&f.dir(), Scope::Base, "img.bin").unwrap(),
            FileContent::Binary
        );
        assert_eq!(
            file_in(&f.dir(), Scope::Base, "big.txt").unwrap(),
            FileContent::TooLarge
        );
        assert!(file_in(&f.dir(), Scope::Base, "nope.txt").is_err());
    }

    #[test]
    fn base_falls_back_to_the_default_branch_of_origin() {
        let f = Fixture::new(false);
        assert_eq!(list_in(&f.dir(), Scope::Base).unwrap().base, "origin/main");
    }

    #[test]
    fn base_falls_back_to_head_without_origin() {
        let f = Fixture::new(false);
        run(&f.app, &["remote", "remove", "origin"]);
        std::fs::write(f.app.join("keep.txt"), "z\n").unwrap();
        let list = list_in(&f.dir(), Scope::Base).unwrap();
        assert_eq!(list.base, "HEAD");
        assert_eq!(list.files.len(), 1);
    }

    #[test]
    fn subdirectory_dir_uses_the_repository_root_for_paths() {
        let f = Fixture::new(true);
        std::fs::create_dir_all(f.app.join("sub")).unwrap();
        std::fs::write(f.app.join("sub/in.txt"), "i\n").unwrap();
        run(&f.app, &["add", "sub/in.txt"]);
        std::fs::write(f.app.join("keep.txt"), "one\nTWO\nthree\n").unwrap();
        std::fs::write(f.app.join("outside.txt"), "o\n").unwrap();
        std::fs::write(f.app.join("sub/loose.txt"), "l\n").unwrap();
        let sub = f.app.join("sub").to_string_lossy().into_owned();
        let list = list(&sub, Scope::Base, None).unwrap();
        let paths: Vec<_> = list.files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(
            paths,
            ["keep.txt", "outside.txt", "sub/in.txt", "sub/loose.txt"]
        );
        for p in ["keep.txt", "outside.txt", "sub/loose.txt"] {
            assert!(matches!(
                file(&sub, Scope::Base, p, None).unwrap(),
                FileContent::Text { .. }
            ));
        }
    }

    #[test]
    fn no_merge_base_reports_head_as_base() {
        let f = Fixture::new(false);
        run(&f.app, &["checkout", "--orphan", "lone"]);
        run(&f.app, &["commit", "-m", "unrelated", "--allow-empty"]);
        let list = list_in(&f.dir(), Scope::Base).unwrap();
        assert_eq!((list.base.as_str(), list.from.as_str()), ("HEAD", "HEAD"));
    }

    #[test]
    fn symlinks_are_not_followed() {
        let f = Fixture::new(true);
        let secret = f.base.join("secret.txt");
        std::fs::write(&secret, "secret\n").unwrap();
        std::os::unix::fs::symlink(&secret, f.app.join("link")).unwrap();
        assert_eq!(
            file_in(&f.dir(), Scope::Base, "link").unwrap(),
            FileContent::Text {
                old: String::new(),
                new: secret.to_string_lossy().into_owned()
            }
        );
    }

    #[test]
    fn rejects_unusable_directories() {
        assert!(list("relative/dir", Scope::Base, None).is_err());
        assert!(list("/definitely/not/here", Scope::Base, None).is_err());
    }

    #[test]
    fn parses_name_status_and_numstat() {
        let ns = b"M\0a.txt\0R087\0old.txt\0new.txt\0D\0d.txt\0A\0n.txt\0";
        assert_eq!(
            parse_name_status(ns),
            vec![
                (Status::Modified, "a.txt".into(), None),
                (Status::Renamed, "new.txt".into(), Some("old.txt".into())),
                (Status::Deleted, "d.txt".into(), None),
                (Status::Added, "n.txt".into(), None),
            ]
        );
        let num = b"3\t1\ta.txt\0-\t-\timg.bin\x000\t0\t\0old.txt\0new.txt\0";
        let m = parse_numstat(num);
        assert_eq!(m["a.txt"], (3, 1, false));
        assert_eq!(m["img.bin"], (0, 0, true));
        assert_eq!(m["new.txt"], (0, 0, false));
    }

    #[test]
    fn counts_lines_without_a_final_newline() {
        let f = Fixture::new(true);
        // "three\n" -> "three" (`\ No newline at end of file`): one line removed, one added.
        std::fs::write(f.app.join("keep.txt"), "one\ntwo\nthree").unwrap();
        let files = f.files(Scope::Base);
        assert_eq!(
            (files["keep.txt"].additions, files["keep.txt"].deletions),
            (1, 1)
        );
        assert_eq!(count_lines(b"a\nb"), 2);
        assert_eq!(count_lines(b"a\nb\n"), 2);
        assert_eq!(count_lines(b""), 0);
    }
}
