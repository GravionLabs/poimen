//! `poimen-server`: one per user, started by the app (or by hand), stopped by the app only on a confirmed restart or quit (`server.shutdown`).
//!
//! Subcommands: `report tokens|clear|agent` (see `report.rs`), `hook claude` / `hook copilot <event>` (run by Claude Code and Copilot CLI, see
//! `claude.rs` and `copilot.rs`) and `integration install|uninstall|status claude|copilot
//! [--settings <path>]`.

use std::path::PathBuf;
use std::process::ExitCode;

use poimen_protocol::CHANNEL;
use poimen_server::{Config, Server, StartError, claude, copilot, hook, report, stdio};

const USAGE: &str =
    "usage: poimen-server integration install|uninstall|status claude|copilot [--settings <path>]";

const HELP: &str = "\
usage:
  poimen-server                      start the server (one per user; no argument)
  poimen-server --version            print the version and the protocol version
  poimen-server stdio                bridge stdin/stdout to the running server (started if absent)
  poimen-server --help, -h           print this usage
  poimen-server report tokens|clear|agent
  poimen-server hook claude
  poimen-server hook copilot <event>
  poimen-server integration install|uninstall|status claude|copilot [--settings <path>]";

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
            match args.get(1).map(String::as_str) {
                Some("claude") => {
                    claude::run_hook(std::io::stdin(), &env);
                }
                Some("copilot") => {
                    copilot::run_hook(std::io::stdin(), args.get(2).cloned(), &env);
                }
                _ => {}
            }
            ExitCode::SUCCESS
        }
        Some("report") => ExitCode::from(report::run(
            &args[1..],
            claude::terminal_id_from_env(),
            poimen_protocol::terminal_socket_path(),
        )),
        Some("integration") => integration(&args[1..]),
        _ => ExitCode::from(2),
    }
}

fn integration(args: &[String]) -> ExitCode {
    let (Some(action), Some(agent @ ("claude" | "copilot"))) =
        (args.first(), args.get(1).map(String::as_str))
    else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    let default = |home: &PathBuf| match agent {
        "claude" => claude::default_settings_path(home),
        _ => copilot::default_hook_path(home, CHANNEL),
    };
    let path = match args.get(2).map(String::as_str) {
        Some("--settings") => args.get(3).map(PathBuf::from),
        None => std::env::home_dir().map(|h| default(&h)),
        _ => None,
    };
    let Some(path) = path else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    let copilot = agent == "copilot";
    let result = match action.as_str() {
        "install" => claude::hook_exe(
            std::env::var_os(poimen_protocol::ENV_HOOK_EXE),
            std::env::current_exe,
        )
        .and_then(|exe| {
            if copilot {
                copilot::install(&path, &exe, CHANNEL)
            } else {
                claude::install(&path, &exe, CHANNEL)
            }
        })
        .map(|()| format!("installed in {}", path.display())),
        "uninstall" => if copilot {
            copilot::uninstall(&path, CHANNEL)
        } else {
            claude::uninstall(&path, CHANNEL)
        }
        .map(|removed| {
            if removed {
                format!("removed from {}", path.display())
            } else {
                "nothing to remove".to_owned()
            }
        }),
        "status" => if copilot {
            copilot::status(&path, CHANNEL).map(|e| (e, copilot::EVENTS.len()))
        } else {
            claude::status(&path, CHANNEL).map(|e| (e, claude::EVENTS.len()))
        }
        .map(|(events, all)| {
            if events.is_empty() {
                "not installed".to_owned()
            } else if events.len() == all {
                "installed".to_owned()
            } else {
                format!("partially installed: {}", events.join(", "))
            }
        }),
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    match result {
        Ok(message) => {
            println!("{message}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("poimen-server: {e}");
            ExitCode::FAILURE
        }
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
        assert!(
            HELP.contains(
                "integration install|uninstall|status claude|copilot [--settings <path>]"
            )
        );
        assert!(HELP.contains("hook claude"));
        assert!(HELP.contains("report tokens|clear|agent"));
    }
}
