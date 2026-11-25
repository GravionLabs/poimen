//! `poimen-server`: one per user, started by the app (or by hand), stopped by the app only on a confirmed restart or quit (`server.shutdown`).
//!
//! Subcommands: `report tokens|clear|agent` (see `report.rs`), `hook <kind> [event]` (run by the agents, `claude`, `copilot` or `gemini`; the kinds are in
//! `integration.rs`) and `integration install|uninstall|status <kind> [--settings <path>]`.

use std::process::ExitCode;

use poimen_server::{Config, Server, StartError, claude, hook, integration, report, stdio};

const HELP: &str = "\
usage:
  poimen-server                      start the server (one per user; no argument)
  poimen-server --version            print the version and the protocol version
  poimen-server stdio                bridge stdin/stdout to the running server (started if absent)
  poimen-server --help, -h           print this usage
  poimen-server report tokens|clear|agent
  poimen-server hook claude
  poimen-server hook copilot <event>
  poimen-server hook gemini
  poimen-server integration ...      deprecated: use `poimen integration install|uninstall|status claude|copilot|gemini [--settings <path>]`";

/// What the first argument asks for. Pure: no process is started by deciding it.
#[derive(Debug, PartialEq, Eq)]
enum Mode {
    Serve,
    Version,
    Help,
    Stdio,
    Subcommand,
    Unknown,
}

fn mode(args: &[String]) -> Mode {
    match args.first().map(String::as_str) {
        None => Mode::Serve,
        Some("--version") => Mode::Version,
        Some("--help" | "-h") => Mode::Help,
        Some("stdio") if args.len() == 1 => Mode::Stdio,
        Some("hook" | "report" | "integration") => Mode::Subcommand,
        Some(_) => Mode::Unknown,
    }
}

/// `poimen-server <version> (protocol <n>)`, read by the machine check of a remote host.
fn version_line() -> String {
    format!(
        "poimen-server {} (protocol {})",
        env!("CARGO_PKG_VERSION"),
        poimen_protocol::PROTOCOL_VERSION
    )
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match mode(&args) {
        Mode::Serve => serve(),
        Mode::Version => {
            println!("{}", version_line());
            ExitCode::SUCCESS
        }
        Mode::Stdio => match stdio::run(&poimen_protocol::socket_path()) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("poimen-server: {e}");
                ExitCode::FAILURE
            }
        },
        Mode::Help => {
            println!("{HELP}");
            ExitCode::SUCCESS
        }
        Mode::Unknown => {
            eprintln!("poimen-server: unknown argument\n{HELP}");
            ExitCode::from(2)
        }
        Mode::Subcommand => subcommand(&args),
    }
}

fn subcommand(args: &[String]) -> ExitCode {
    match args.first().map(String::as_str) {
        Some("hook") => {
            // Never fails and never blocks the agent, whatever happens; a report that does not
            // get through is a line in the hook log (`hook.rs`).
            let env = hook::HookEnv::from_env();
            if let Some(kind) = args.get(1).and_then(|n| integration::kind(n)) {
                (kind.run_hook)(args.get(2).cloned(), &env);
            }
            ExitCode::SUCCESS
        }
        Some("report") => ExitCode::from(report::run(
            &args[1..],
            claude::terminal_id_from_env(),
            poimen_protocol::terminal_socket_path(),
        )),
        Some("integration") => {
            eprintln!(
                "poimen-server: `integration` is deprecated and goes away in a later release; use `poimen integration {}`",
                args[1..].join(" ")
            );
            integration::run(
                &args[1..],
                "poimen-server",
                claude::hook_exe(
                    std::env::var_os(poimen_protocol::ENV_HOOK_EXE),
                    std::env::current_exe,
                ),
            )
        }
        _ => ExitCode::from(2),
    }
}

fn serve() -> ExitCode {
    match Server::bind(Config::from_env()) {
        Ok(server) => {
            eprintln!("poimen-server: listening on {}", server.socket().display());
            // `server.shutdown`: the app asked after the user confirmed; the terminals are gone.
            server.on_shutdown(|| std::process::exit(0));
            server.serve();
            ExitCode::SUCCESS
        }
        // A second start while one runs is not an error for whoever raced to start it.
        Err(e @ StartError::AlreadyRunning(_)) => {
            eprintln!("poimen-server: {e}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("poimen-server: {e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn no_argument_serves() {
        assert_eq!(mode(&args(&[])), Mode::Serve);
    }

    #[test]
    fn help_flags_print_help() {
        assert_eq!(mode(&args(&["--help"])), Mode::Help);
        assert_eq!(mode(&args(&["-h"])), Mode::Help);
    }

    #[test]
    fn version_flag_prints_version() {
        assert_eq!(mode(&args(&["--version"])), Mode::Version);
    }

    #[test]
    fn stdio_takes_no_further_argument() {
        assert_eq!(mode(&args(&["stdio"])), Mode::Stdio);
        assert_eq!(mode(&args(&["stdio", "x"])), Mode::Unknown);
    }

    #[test]
    fn version_line_has_version_and_protocol() {
        let line = version_line();
        assert!(line.starts_with(concat!("poimen-server ", env!("CARGO_PKG_VERSION"))));
        assert!(line.ends_with(&format!("(protocol {})", poimen_protocol::PROTOCOL_VERSION)));
    }

    #[test]
    fn known_subcommands_are_dispatched() {
        for sub in ["hook", "report", "integration"] {
            assert_eq!(mode(&args(&[sub, "x"])), Mode::Subcommand, "{sub}");
        }
    }

    #[test]
    fn unknown_argument_is_unknown_and_never_serves() {
        assert_eq!(mode(&args(&["--bogus"])), Mode::Unknown);
        assert_eq!(mode(&args(&["serve"])), Mode::Unknown);
        assert_eq!(mode(&args(&["-"])), Mode::Unknown);
    }

    #[test]
    fn help_lists_the_integration_usage() {
        assert!(HELP.contains("poimen integration install|uninstall|status"));
        for kind in integration::KINDS {
            assert!(
                HELP.contains(&format!("hook {}", kind.name)),
                "{}",
                kind.name
            );
        }
        assert!(HELP.contains("hook copilot <event>"));
        assert!(HELP.contains("hook claude"));
        assert!(HELP.contains("report tokens|clear|agent"));
    }
}
