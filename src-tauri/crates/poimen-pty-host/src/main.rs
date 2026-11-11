//! `poimen-pty-host`: started by `poimen-server` with one [`HostStart`] line on stdin; answers
//! with `hello` (or `error`) on stdout and serves its endpoint until it ends (ADR 0009,
//! `docs/protocol.md`, "Host protocol").
//!
//! [`HostStart`]: poimen_protocol::host::HostStart

use poimen_protocol::host::HostMessage;
use poimen_pty_host::{Host, read_start, version_line, write_message};

const HELP: &str = "\
usage:
  poimen-pty-host               read the start line from stdin and run the host
  poimen-pty-host --version     print the version and the host protocol version
  poimen-pty-host --help, -h    print this usage";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        None => {}
        Some("--version") => {
            println!("{}", version_line());
            return;
        }
        Some("--help" | "-h") => {
            println!("{HELP}");
            return;
        }
        Some(other) => {
            eprintln!("poimen-pty-host: unknown argument {other}\n{HELP}");
            std::process::exit(2);
        }
    }
    let mut stdout = std::io::stdout();
    let started = read_start(std::io::stdin()).and_then(|start| Host::start(&start));
    let host = match started {
        Ok(host) => host,
        Err(message) => {
            eprintln!("poimen-pty-host: {message}");
            let _ = write_message(&mut stdout, &HostMessage::Error { message });
            std::process::exit(1);
        }
    };
    let _ = write_message(&mut stdout, &host.hello());
    host.serve();
    // Threads blocked on the PTY or on a client stay behind; ending the process ends them.
    std::process::exit(0);
}
