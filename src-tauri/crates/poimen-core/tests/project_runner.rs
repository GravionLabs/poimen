//! `SystemRunner` against a real shell.
#![cfg(unix)]

use std::path::Path;
use std::time::Duration;

use poimen_core::project::{ProcessRunner, RunResult, SystemRunner, shell_argv};

fn sh(script: &str, timeout_ms: u64, cap: usize) -> RunResult {
    let argv = shell_argv("linux", Some("/bin/sh"), false, script);
    SystemRunner
        .run(
            &argv,
            Path::new("/tmp"),
            &Default::default(),
            Duration::from_millis(timeout_ms),
            cap,
        )
        .unwrap()
}

#[test]
fn captures_stdout_stderr_and_the_exit_code() {
    let r = sh("echo out; echo err >&2; exit 3", 10_000, 1024);
    assert_eq!(r.exit_code, Some(3));
    assert!(!r.timed_out && !r.success());
    assert!(r.output.contains("out") && r.output.contains("err"));
}

#[test]
fn runs_in_the_given_directory() {
    let r = sh("pwd", 10_000, 1024);
    assert!(r.success());
    assert!(r.output.trim().ends_with("tmp"));
}

#[test]
fn a_command_over_the_timeout_is_killed() {
    let r = sh("sleep 30", 200, 1024);
    assert!(r.timed_out && !r.success());
    assert_eq!(r.exit_code, None);
}

#[test]
fn output_is_capped() {
    let r = sh("yes x | head -c 5000", 10_000, 100);
    assert!(r.success());
    assert_eq!(r.output.len(), 100);
    assert!(r.truncated);
}

#[test]
fn a_missing_program_is_an_error() {
    let err = SystemRunner
        .run(
            &["/no/such/shell".into()],
            Path::new("/tmp"),
            &Default::default(),
            Duration::from_secs(1),
            10,
        )
        .unwrap_err();
    assert!(err.contains("could not be started"));
}
