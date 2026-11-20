//! The real `Remote`: one `ssh` process per step, sharing the machine's connection (#1012).

use std::io::{self, Write};
use std::process::Stdio;
use std::thread;

use poimen_core::gui_path;

use super::{Out, Remote, join_output};
use crate::machines::{check_target, ssh_mux};

/// ssh in batch mode: never a prompt, bounded connect.
pub struct Ssh;

impl Remote for Ssh {
    fn run(&self, target: &str, command: &str, input: Option<&[u8]>) -> io::Result<Out> {
        check_target(target).map_err(io::Error::other)?;
        let program = gui_path::resolve_program("ssh", &gui_path::child_path())
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "ssh is not on the PATH"))?;
        let mux = ssh_mux::for_target(&program, target);
        let out = run_once(&program, &mux.options, target, command, input)?;
        // A stale control socket (the master died): remove it and try once more.
        if !out.ok
            && ssh_mux::is_stale(&out.text)
            && mux.socket.as_deref().is_some_and(ssh_mux::remove_stale)
        {
            return run_once(&program, &mux.options, target, command, input);
        }
        Ok(out)
    }
}

fn run_once(
    program: &std::path::Path,
    extra: &[String],
    target: &str,
    command: &str,
    input: Option<&[u8]>,
) -> io::Result<Out> {
    let mut child = gui_path::command(program)
        .args([
            "-T",
            "-o",
            "BatchMode=yes",
            "-o",
            "ConnectTimeout=10",
            "-o",
            "ServerAliveInterval=15",
        ])
        .args(extra)
        .args(["--", target, command])
        .env("PATH", gui_path::child_path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let mut stdin = child.stdin.take();
    let data = input.map(<[u8]>::to_vec);
    // Writing and reading run apart, so a full pipe in either direction cannot stall.
    let writer = thread::spawn(move || {
        if let (Some(stdin), Some(data)) = (stdin.as_mut(), data) {
            let _ = stdin.write_all(&data);
        }
        drop(stdin);
    });
    let output = child.wait_with_output()?;
    let _ = writer.join();
    Ok(Out {
        ok: output.status.success(),
        text: join_output(&output.stdout, &output.stderr),
    })
}
