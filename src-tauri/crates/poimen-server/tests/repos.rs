//! `repo.ensure` and `worktree.create` (#282) end to end: a real server on a temp socket, a temp
//! home and a bare local `origin`. `gh repo clone` is faked by a `Gh` that clones the bare origin
//! and points the clone's `origin` at the GitHub URL (with an `insteadOf` back to the bare one),
//! so no test needs the network. Never the user's server, home or config.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::{io, thread};

use poimen_client::protocol::{ErrorCode, TaskIssue};
use poimen_client::{Client, ClientError};
use poimen_core::github::Output;
use poimen_core::repos::Gh;
use poimen_core::settings;
use poimen_server::{Config, PtyHost, Server};

static N: AtomicU32 = AtomicU32::new(0);

fn git(dir: &Path, args: &[&str]) -> String {
    let out = poimen_core::gui_path::command("git")
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
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

/// `gh repo clone owner/repo <path>` as a clone of the bare origin; records every call.
struct FakeGh {
    origin: PathBuf,
    calls: Mutex<Vec<Vec<String>>>,
}

impl Gh for FakeGh {
    fn run(&self, args: &[String]) -> io::Result<Output> {
        self.calls.lock().unwrap().push(args.to_vec());
        let [verb, sub, name, path] = args else {
            return Err(io::Error::other(format!("unexpected gh {args:?}")));
        };
        assert_eq!((verb.as_str(), sub.as_str()), ("repo", "clone"));
        let parent = Path::new(path).parent().unwrap();
        git(parent, &["clone", self.origin.to_str().unwrap(), path]);
        let url = format!("https://github.com/{name}.git");
        let path = Path::new(path);
        git(path, &["remote", "set-url", "origin", &url]);
        let key = format!("url.{}.insteadOf", self.origin.display());
        git(path, &["config", &key, &url]);
        Ok(Output {
            code: Some(0),
            stdout: String::new(),
            stderr: String::new(),
        })
    }
}

struct Env {
    dir: PathBuf,
    origin: PathBuf,
    seed: PathBuf,
    gh: Arc<FakeGh>,
}

impl Env {
    /// A bare `origin.git` with one commit on `main` (`a.txt`) and a `seed` clone that pushes.
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!(
            "poimen-repos-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // Windows canonicalizes to `\\?\C:\...`, which git reads as a host name.
        let dir = PathBuf::from(
            dir.canonicalize()
                .unwrap()
                .to_string_lossy()
                .trim_start_matches(r"\\?\"),
        );
        let origin = dir.join("origin.git");
        let seed = dir.join("seed");
        std::fs::create_dir_all(&seed).unwrap();
        git(&dir, &["init", "--bare", "-b", "main", "origin.git"]);
        git(&seed, &["init", "-b", "main"]);
        std::fs::write(seed.join("a.txt"), "a").unwrap();
        git(&seed, &["add", "."]);
        git(&seed, &["commit", "-m", "init"]);
        git(
            &seed,
            &["remote", "add", "origin", origin.to_str().unwrap()],
        );
        git(&seed, &["push", "origin", "main"]);
        git(
            &dir,
            &[
                "--git-dir",
                origin.to_str().unwrap(),
                "symbolic-ref",
                "HEAD",
                "refs/heads/main",
            ],
        );
        std::fs::create_dir_all(settings::config_dir(&dir.join("home"))).unwrap();
        let gh = Arc::new(FakeGh {
            origin: origin.clone(),
            calls: Mutex::new(Vec::new()),
        });
        Self {
            dir,
            origin,
            seed,
            gh,
        }
    }

    fn home(&self) -> PathBuf {
        self.dir.join("home")
    }

    fn start(&self) -> Client {
        let config = Config {
            socket: self.dir.join("run").join("server.sock"),
            state_file: self.dir.join("state").join("server.json"),
            scrollback: 1000,
            home: Some(self.home()),
            pty_host: PtyHost::launch(poimen_pty_host::spawn_in_thread),
        };
        let server = Server::bind(config.clone()).unwrap();
        server.set_gh(self.gh.clone());
        thread::spawn(move || server.serve());
        Client::new(&config.socket)
    }

    fn calls(&self) -> usize {
        self.gh.calls.lock().unwrap().len()
    }

    /// One more commit on `origin/main`, whose id is returned.
    fn push_commit(&self, file: &str) -> String {
        std::fs::write(self.seed.join(file), file).unwrap();
        git(&self.seed, &["add", "."]);
        git(&self.seed, &["commit", "-m", file]);
        git(&self.seed, &["push", "origin", "main"]);
        git(&self.seed, &["rev-parse", "HEAD"])
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn code(e: ClientError) -> (ErrorCode, String) {
    match e {
        ClientError::Server { code, message } => (code, message),
        other => panic!("not a server error: {other:?}"),
    }
}

fn listing(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn absent_repository_is_cloned_under_the_default_repos_dir() {
    let env = Env::new();
    let client = env.start();
    let r = client.ensure_repo("GravionLabs", "demo").unwrap();
    let want = env.home().join("Source").join("GravionLabs").join("demo");
    assert_eq!(PathBuf::from(&r.path), want);
    assert!(r.cloned);
    assert!(want.join("a.txt").exists());
    assert_eq!(
        env.gh.calls.lock().unwrap()[0],
        ["repo", "clone", "GravionLabs/demo", r.path.as_str()]
    );
}

#[test]
fn existing_clone_is_fetched_and_not_cloned_again() {
    let env = Env::new();
    let client = env.start();
    let first = client.ensure_repo("GravionLabs", "demo").unwrap();
    let later = env.push_commit("b.txt");
    let r = client.ensure_repo("GravionLabs", "demo").unwrap();
    assert_eq!(r.path, first.path);
    assert!(!r.cloned);
    assert_eq!(env.calls(), 1, "gh ran only for the first clone");
    let path = PathBuf::from(&r.path);
    assert_eq!(git(&path, &["rev-parse", "origin/main"]), later);
    // A fetch only: the checked-out files stay as they were.
    assert!(!path.join("b.txt").exists());
}

#[test]
fn repos_dir_of_the_user_config_is_used() {
    let env = Env::new();
    let repos = env.dir.join("repos");
    let template = format!("{}/{{owner}}-{{repo}}", repos.display()).replace('\\', "/");
    std::fs::write(
        settings::config_path(&env.home()),
        serde_json::json!({ "reposDir": template }).to_string(),
    )
    .unwrap();
    let r = env.start().ensure_repo("o", "r").unwrap();
    assert_eq!(PathBuf::from(&r.path), repos.join("o-r"));
    assert!(repos.join("o-r/a.txt").exists());
}

#[test]
fn foreign_directory_is_refused_and_left_untouched() {
    let env = Env::new();
    let client = env.start();
    let base = env.home().join("Source").join("o");

    // A plain directory with a file.
    let plain = base.join("plain");
    std::fs::create_dir_all(&plain).unwrap();
    std::fs::write(plain.join("notes.txt"), "mine").unwrap();
    let (c, m) = code(client.ensure_repo("o", "plain").unwrap_err());
    assert_eq!(c, ErrorCode::Conflict);
    assert!(m.contains(&plain.display().to_string()), "{m}");
    assert_eq!(listing(&plain), ["notes.txt"]);
    assert_eq!(
        std::fs::read_to_string(plain.join("notes.txt")).unwrap(),
        "mine"
    );

    // An empty directory: a clone never goes into an existing one.
    let empty = base.join("empty");
    std::fs::create_dir_all(&empty).unwrap();
    let (c, _) = code(client.ensure_repo("o", "empty").unwrap_err());
    assert_eq!(c, ErrorCode::Conflict);
    assert!(listing(&empty).is_empty());

    // A clone of another repository.
    let other = base.join("other");
    git(&base, &["clone", env.origin.to_str().unwrap(), "other"]);
    git(
        &other,
        &[
            "remote",
            "set-url",
            "origin",
            "https://github.com/o/else.git",
        ],
    );
    let head = git(&other, &["rev-parse", "HEAD"]);
    env.push_commit("c.txt");
    let (c, m) = code(client.ensure_repo("o", "other").unwrap_err());
    assert_eq!(c, ErrorCode::Conflict);
    assert!(m.contains("o/else"), "{m}");
    // Not fetched: its remote-tracking branch did not move.
    assert_eq!(git(&other, &["rev-parse", "origin/main"]), head);

    // A directory inside a clone.
    let inner = other.join("sub");
    std::fs::create_dir_all(&inner).unwrap();
    std::fs::write(
        settings::config_path(&env.home()),
        serde_json::json!({ "reposDir": format!("{}/{{repo}}", other.display()).replace('\\', "/") })
            .to_string(),
    )
    .unwrap();
    let (c, _) = code(client.ensure_repo("o", "sub").unwrap_err());
    assert_eq!(c, ErrorCode::Conflict);

    assert_eq!(env.calls(), 0, "gh never ran");
}

#[test]
fn invalid_names_are_invalid_params() {
    let env = Env::new();
    let client = env.start();
    for (owner, repo) in [("..", "x"), ("o", "a/b"), ("", "r")] {
        let (c, _) = code(client.ensure_repo(owner, repo).unwrap_err());
        assert_eq!(c, ErrorCode::InvalidParams, "{owner}/{repo}");
    }
    assert_eq!(env.calls(), 0);
}

#[test]
fn worktree_create_matches_task_worktree() {
    let env = Env::new();
    let client = env.start();
    let repo = client.ensure_repo("o", "app").unwrap();
    let later = env.push_commit("d.txt");
    let issue = || TaskIssue {
        nr: 7,
        title: "feat(x): Add thing".into(),
        url: "https://github.com/o/app/issues/7".into(),
    };
    let wt = client
        .create_worktree(&repo.path, issue(), "agent")
        .unwrap();
    assert_eq!(wt.branch, "feat/7-add-thing");
    let path = PathBuf::from(&wt.path);
    assert_eq!(
        path,
        PathBuf::from(&repo.path).parent().unwrap().join("app-7")
    );
    assert!(wt.created);
    // Branched from the base fetched in the same call.
    assert_eq!(git(&path, &["rev-parse", "HEAD"]), later);

    // Again: the existing worktree, untouched.
    std::fs::write(path.join("work.txt"), "w").unwrap();
    let again = client
        .create_worktree(&repo.path, issue(), "agent")
        .unwrap();
    assert_eq!(again.path, wt.path);
    assert!(!again.created);
    assert_eq!(git(&path, &["rev-parse", "HEAD"]), later);
    assert!(path.join("work.txt").exists());

    let (c, _) = code(
        client
            .create_worktree(&repo.path, issue(), "nope")
            .unwrap_err(),
    );
    assert_eq!(c, ErrorCode::InvalidParams);
    let gone = env.dir.join("gone").display().to_string();
    let (c, _) = code(client.create_worktree(&gone, issue(), "agent").unwrap_err());
    assert_eq!(c, ErrorCode::InvalidParams);
}
