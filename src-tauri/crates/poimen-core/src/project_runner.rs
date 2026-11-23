//! The real [`ProcessRunner`] of the project's commands (`run_teardown`).

use std::path::Path;
use std::time::Duration;

use crate::project::{ProcessRunner, RunResult};

/// The real [`ProcessRunner`]: stdin closed, the login-shell `PATH`, stdout and stderr read on
/// their own threads into one capped buffer, the child killed after `timeout`.
pub struct SystemRunner;

impl ProcessRunner for SystemRunner {
    fn run(
        &self,
        argv: &[String],
        cwd: &Path,
        env: &std::collections::BTreeMap<String, String>,
        timeout: Duration,
        cap: usize,
    ) -> Result<RunResult, String> {
        use std::io::Read;
        use std::process::Stdio;
        use std::sync::{Arc, Mutex};
        use std::time::Instant;

        let (program, args) = argv.split_first().ok_or("no command to run")?;
        let mut child = crate::gui_path::command(program)
            .args(args)
            .current_dir(cwd)
            .envs(env)
            .env("PATH", crate::gui_path::child_path())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("{program} could not be started: {e}"))?;
        let buffer = Arc::new(Mutex::new((Vec::<u8>::new(), false)));
        let readers: Vec<_> = [
            child
                .stdout
                .take()
                .map(|s| Box::new(s) as Box<dyn Read + Send>),
            child
                .stderr
                .take()
                .map(|s| Box::new(s) as Box<dyn Read + Send>),
        ]
        .into_iter()
        .flatten()
        .map(|mut stream| {
            let buffer = Arc::clone(&buffer);
            std::thread::spawn(move || {
                let mut chunk = [0u8; 8192];
                while let Ok(n) = stream.read(&mut chunk) {
                    if n == 0 {
                        break;
                    }
                    let mut b = buffer.lock().unwrap();
                    let room = cap.saturating_sub(b.0.len());
                    b.0.extend_from_slice(&chunk[..n.min(room)]);
                    if n > room {
                        b.1 = true;
                    }
                }
            })
        })
        .collect();
        let start = Instant::now();
        let mut timed_out = false;
        let status = loop {
            match child.try_wait().map_err(|e| e.to_string())? {
                Some(status) => break status,
                None if start.elapsed() >= timeout => {
                    timed_out = true;
                    let _ = child.kill();
                    break child.wait().map_err(|e| e.to_string())?;
                }
                None => std::thread::sleep(Duration::from_millis(10)),
            }
        };
        // A killed shell can leave grandchildren holding the pipes; do not wait for them.
        if !timed_out {
            for r in readers {
                let _ = r.join();
            }
        }
        let b = buffer.lock().unwrap();
        Ok(RunResult {
            exit_code: status.code(),
            timed_out,
            output: String::from_utf8_lossy(&b.0).into_owned(),
            truncated: b.1,
        })
    }
}
