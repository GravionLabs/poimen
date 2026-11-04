//! `poimen-server`: one per user, started by the app (or by hand), never stopped by the app.
//!
//! Subcommands: `hook claude` (run by Claude Code, see `claude.rs`) and
//! `integration install|uninstall|status claude [--settings <path>]`.

use std::path::PathBuf;
use std::process::ExitCode;

use poimen_server::{Config, Server, StartError, claude};

const USAGE: &str =
    "usage: poimen-server integration install|uninstall|status claude [--settings <path>]";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("--version") => {
            println!("poimen-server {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Some("hook") => {
            // Never fails and never blocks Claude Code, whatever happens.
            if args.get(1).map(String::as_str) == Some("claude") {
                claude::run_hook(
                    std::io::stdin(),
                    claude::terminal_id_from_env(),
                    poimen_protocol::socket_path(),
                );
            }
            ExitCode::SUCCESS
        }
        Some("integration") => integration(&args[1..]),
        _ => serve(),
    }
}

fn integration(args: &[String]) -> ExitCode {
    let (Some(action), Some("claude")) = (args.first(), args.get(1).map(String::as_str)) else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    let path = match args.get(2).map(String::as_str) {
        Some("--settings") => args.get(3).map(PathBuf::from),
        None => std::env::var_os("HOME").map(|h| claude::default_settings_path(&PathBuf::from(h))),
        _ => None,
    };
    let Some(path) = path else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    let result = match action.as_str() {
        "install" => std::env::current_exe()
            .map_err(|e| e.to_string())
            .and_then(|exe| claude::install(&path, &exe))
            .map(|()| format!("installed in {}", path.display())),
        "uninstall" => claude::uninstall(&path).map(|removed| {
            if removed {
                format!("removed from {}", path.display())
            } else {
                "nothing to remove".to_owned()
            }
        }),
        "status" => claude::status(&path).map(|events| {
            if events.is_empty() {
                "not installed".to_owned()
            } else if events.len() == claude::EVENTS.len() {
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
