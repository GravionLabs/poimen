//! Drafts a Conventional Commit message from the staged diff through a headless call of an agent
//! harness (`printArgs`, for example `claude -p`). The result only fills the commit box: nothing is
//! committed. The diff is shortened to the file list and the largest hunks over `DIFF_LIMIT`
//! (the prompt is one argv element), the call has a deadline and can be cancelled by `id`.

use std::collections::HashMap;
use std::io::Read;
use std::path::Path;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use poimen_core::harness::{self, Harness};

use crate::changes::validate;
use crate::git::run_bytes;

/// Longest the harness may take.
pub(crate) const TIMEOUT: Duration = Duration::from_secs(120);
/// Bytes of diff sent as is; more is shortened (Windows caps a command line near 32 000).
pub(crate) const DIFF_LIMIT: usize = if cfg!(windows) { 20_000 } else { 60_000 };
/// Bytes of one hunk kept when the diff is shortened.
const HUNK_LIMIT: usize = 4_000;
/// Fewest bytes of a hunk worth keeping.
const MIN_HUNK: usize = 200;

fn running() -> &'static Mutex<HashMap<String, Arc<AtomicBool>>> {
    static RUNNING: OnceLock<Mutex<HashMap<String, Arc<AtomicBool>>>> = OnceLock::new();
    RUNNING.get_or_init(Default::default)
}

/// Asks the generation `id` to stop; false when none runs under that id.
pub fn cancel(id: &str) -> bool {
    match running().lock().unwrap().get(id) {
        Some(flag) => {
            flag.store(true, Ordering::SeqCst);
            true
        }
        None => false,
    }
}

pub(crate) fn cut(text: &str, max: usize) -> &str {
    let mut end = text.len().min(max);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

struct Hunk<'a> {
    file: &'a str,
    text: &'a str,
}

/// `diff` as is when it fits `limit`, else the file list followed by the largest hunks that fit
/// (each cut to `HUNK_LIMIT`), in the order of the diff.
pub(crate) fn shorten(diff: &str, limit: usize) -> String {
    if diff.len() <= limit {
        return diff.to_owned();
    }
    let mut files: Vec<&str> = Vec::new();
    let mut hunks: Vec<Hunk> = Vec::new();
    let mut starts: Vec<usize> = diff
        .match_indices("\ndiff --git ")
        .map(|(i, _)| i + 1)
        .collect();
    if diff.starts_with("diff --git ") {
        starts.insert(0, 0);
    }
    starts.push(diff.len());
    for w in starts.windows(2) {
        let section = &diff[w[0]..w[1]];
        let head = section.lines().next().unwrap_or("");
        let file = head.rsplit_once(" b/").map_or(head, |(_, b)| b);
        files.push(file);
        let mut at = section.find("\n@@").map(|i| i + 1);
        while let Some(h) = at {
            let next = section[h + 1..].find("\n@@").map(|i| h + 1 + i + 1);
            let text = &section[h..next.unwrap_or(section.len())];
            hunks.push(Hunk { file, text });
            at = next;
        }
    }

    let mut out = format!(
        "The diff is too large ({} bytes) and shortened. Changed files ({}):\n",
        diff.len(),
        files.len()
    );
    for f in &files {
        out.push_str(&format!("- {f}\n"));
        if out.len() > limit / 2 {
            out.push_str("- (more files not listed)\n");
            break;
        }
    }
    out.push_str("\nLargest hunks:\n");
    let mut order: Vec<usize> = (0..hunks.len()).collect();
    order.sort_by_key(|&i| std::cmp::Reverse(hunks[i].text.len()));
    let mut chosen: Vec<(usize, usize)> = Vec::new();
    let mut used = out.len();
    for i in order {
        let overhead = hunks[i].file.len() + 32;
        let room = limit.saturating_sub(used + overhead).min(HUNK_LIMIT);
        if room < MIN_HUNK {
            continue;
        }
        let len = cut(hunks[i].text, room).len();
        used += len + overhead;
        chosen.push((i, len));
    }
    chosen.sort_unstable();
    for (i, len) in chosen {
        let h = &hunks[i];
        out.push_str(&format!("\n--- {}\n{}", h.file, &h.text[..len]));
        if len < h.text.len() {
            out.push_str("\n(hunk cut)\n");
        }
    }
    out
}

fn prompt(diff: &str, branch: &str) -> String {
    format!(
        "Write a Conventional Commit message for the staged changes below.\n\
         Rules: first line `type(scope): summary` in the imperative, at most 72 characters, \
         type one of feat, fix, docs, style, refactor, perf, test, build, ci, chore; \
         optionally a blank line and a short body that says why. Reply with the message only: \
         no code fence, no explanation.\nBranch: {branch}\n\nStaged diff:\n{diff}"
    )
}

/// The message without a surrounding code fence or blank lines around it.
pub(crate) fn clean(output: &str) -> String {
    let mut text = output.trim();
    if let Some(rest) = text.strip_prefix("```") {
        text = rest.split_once('\n').map_or("", |(_, r)| r);
        text = text.trim_end().strip_suffix("```").unwrap_or(text);
    }
    text.trim().to_owned()
}

/// Kills the process `pid` and what it started (its own process group on Unix, `taskkill /T` on
/// Windows).
fn kill_tree(pid: u32) {
    #[cfg(unix)]
    let _ = poimen_core::gui_path::command("kill")
        .args(["-KILL", "--", &format!("-{pid}")])
        .stderr(Stdio::null())
        .status();
    #[cfg(windows)]
    let _ = poimen_core::gui_path::command("taskkill")
        .args(["/T", "/F", "/PID", &pid.to_string()])
        .status();
}

fn drain(mut pipe: impl Read + Send + 'static) -> std::thread::JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = pipe.read_to_end(&mut buf);
        buf
    })
}

pub(crate) fn run(
    argv: &[String],
    dir: &str,
    cancelled: &AtomicBool,
    timeout: Duration,
) -> Result<String, String> {
    let (program, args) = argv.split_first().ok_or("The harness has no command")?;
    let resolved =
        poimen_core::gui_path::resolve_program(program, &poimen_core::gui_path::child_path())
            .ok_or_else(|| format!("{program} was not found on the PATH"))?;
    let mut command = poimen_core::gui_path::command(resolved);
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut command, 0);
    let mut child = command
        .args(args)
        .current_dir(dir)
        .env("PATH", poimen_core::gui_path::child_path())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("{program} could not be started: {e}"))?;
    let out = drain(child.stdout.take().expect("piped"));
    let err = drain(child.stderr.take().expect("piped"));
    let pid = child.id();
    let (done, exited) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = done.send(child.wait());
    });
    let started = Instant::now();
    let status = loop {
        if cancelled.load(Ordering::SeqCst) {
            kill_tree(pid);
            return Err("Cancelled".into());
        }
        if started.elapsed() >= timeout {
            kill_tree(pid);
            return Err(format!("{program} timed out after {} s", timeout.as_secs()));
        }
        match exited.recv_timeout(Duration::from_millis(50)) {
            Ok(Ok(status)) => break status,
            Ok(Err(e)) => return Err(format!("{program} failed: {e}")),
            Err(_) => {}
        }
    };
    let out = String::from_utf8_lossy(&out.join().unwrap_or_default()).into_owned();
    let err = String::from_utf8_lossy(&err.join().unwrap_or_default()).into_owned();
    if !status.success() {
        let text = if err.trim().is_empty() {
            out.trim()
        } else {
            err.trim()
        };
        return Err(format!("{program} failed: {}", cut(text, 2_000)));
    }
    Ok(out)
}

/// Drafts the message for the staged changes of `dir` with `harness`; `id` names the call for
/// `cancel`. Never commits and never changes the repository.
pub fn generate(
    dir: &str,
    harness: &Harness,
    id: &str,
    home: Option<&Path>,
) -> Result<String, String> {
    tracked(id, |flag| draft(dir, harness, flag, home))
}

/// Runs `work` with a cancel flag that `cancel(id)` sets; the id is free again afterwards.
pub(crate) fn tracked<T>(id: &str, work: impl FnOnce(&AtomicBool) -> T) -> T {
    let flag = Arc::new(AtomicBool::new(false));
    running()
        .lock()
        .unwrap()
        .insert(id.to_owned(), flag.clone());
    let result = work(&flag);
    running().lock().unwrap().remove(id);
    result
}

fn draft(
    dir: &str,
    harness: &Harness,
    flag: &AtomicBool,
    home: Option<&Path>,
) -> Result<String, String> {
    let top = validate(dir, home)?;
    let diff = run_bytes(
        &top,
        &[
            "diff",
            "--cached",
            "--no-color",
            "--no-ext-diff",
            "--no-textconv",
        ],
    )?;
    if diff.is_empty() {
        return Err("Nothing is staged".into());
    }
    let diff = String::from_utf8_lossy(&diff);
    let branch = String::from_utf8_lossy(
        &run_bytes(&top, &["rev-parse", "--abbrev-ref", "HEAD"]).unwrap_or_default(),
    )
    .trim()
    .to_owned();
    let text = prompt(&shorten(&diff, DIFF_LIMIT), &branch);
    let argv = harness::build_print_argv(harness, &text)
        .ok_or_else(|| format!("{} has no printArgs", harness.id))?;
    if flag.load(Ordering::SeqCst) {
        return Err("Cancelled".into());
    }
    let message = clean(&run(&argv, &top, flag, TIMEOUT)?);
    if message.is_empty() {
        return Err(format!("{} returned no message", harness.id));
    }
    Ok(message)
}

/// Drafts a commit message for the staged changes of `dir` with the harness `harness` (its
/// `printArgs`); `id` names the call for `changes_generate_cancel`. Never commits.
#[tauri::command]
pub async fn changes_generate_message(
    dir: String,
    harness: String,
    id: String,
) -> Result<String, String> {
    let found = crate::get_settings()
        .harnesses
        .into_iter()
        .find(|h| h.id == harness)
        .ok_or_else(|| format!("unknown harness {harness:?}"))?;
    tauri::async_runtime::spawn_blocking(move || {
        generate(&dir, &found, &id, crate::home_dir().as_deref())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Stops the commit message generation `id`.
#[tauri::command]
pub fn changes_generate_cancel(id: String) -> bool {
    cancel(&id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn big_diff() -> String {
        let mut d = String::new();
        for (name, lines) in [("a.rs", 3), ("b.rs", 300), ("c.rs", 40)] {
            d.push_str(&format!(
                "diff --git a/{name} b/{name}\n--- a/{name}\n+++ b/{name}\n@@ -1 +1 @@\n"
            ));
            for i in 0..lines {
                d.push_str(&format!("+line {i} of {name}\n"));
            }
        }
        d
    }

    #[test]
    fn a_small_diff_is_left_alone() {
        let d = big_diff();
        assert_eq!(shorten(&d, d.len()), d);
    }

    #[test]
    fn a_large_diff_keeps_the_file_list_and_the_largest_hunks_within_the_limit() {
        let d = big_diff();
        let out = shorten(&d, 1_500);
        assert!(out.len() <= 1_500, "{}", out.len());
        for f in ["- a.rs", "- b.rs", "- c.rs"] {
            assert!(out.contains(f), "{f}");
        }
        assert!(out.contains("--- b.rs"));
        assert!(out.contains("(hunk cut)") || out.contains("--- c.rs"));
        assert!(out.contains("too large"));
    }

    #[test]
    fn cutting_respects_character_boundaries() {
        assert_eq!(cut("aé", 2), "a");
    }

    #[test]
    fn cleans_a_fenced_reply() {
        assert_eq!(
            clean("```\nfeat(x): y\n\nbody\n```\n"),
            "feat(x): y\n\nbody"
        );
        assert_eq!(clean("```text\nfix: z\n```"), "fix: z");
        assert_eq!(clean("  fix: z \n"), "fix: z");
    }

    #[cfg(unix)]
    fn script(body: &str) -> (crate::test_dir::TestDir, Vec<String>) {
        let dir = crate::test_dir::TestDir::new("poimen-cm-");
        (dir, vec!["sh".into(), "-c".into(), body.into()])
    }

    #[cfg(unix)]
    #[test]
    fn runs_the_command_and_reports_a_failure() {
        let (dir, argv) = script("echo feat: x");
        let flag = AtomicBool::new(false);
        let out = run(&argv, dir.to_str().unwrap(), &flag, Duration::from_secs(5)).unwrap();
        assert_eq!(out.trim(), "feat: x");
        let (_, argv) = script("echo nope >&2; exit 3");
        let err = run(&argv, dir.to_str().unwrap(), &flag, Duration::from_secs(5)).unwrap_err();
        assert!(err.contains("nope"), "{err}");
    }

    #[cfg(unix)]
    #[test]
    fn cancel_and_timeout_stop_the_command() {
        let (dir, argv) = script("sleep 30");
        let flag = AtomicBool::new(true);
        let err = run(&argv, dir.to_str().unwrap(), &flag, Duration::from_secs(5)).unwrap_err();
        assert_eq!(err, "Cancelled");
        let flag = AtomicBool::new(false);
        let err = run(
            &argv,
            dir.to_str().unwrap(),
            &flag,
            Duration::from_millis(100),
        )
        .unwrap_err();
        assert!(err.contains("timed out"), "{err}");
    }

    #[test]
    fn cancel_of_an_unknown_id_is_false() {
        assert!(!cancel("nobody"));
    }
}
