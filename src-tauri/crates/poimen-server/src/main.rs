//! `poimen-server`: one per user, started by the app (or by hand), never stopped by the app.
//!
//! Subcommands: `report tokens|clear|agent` (see `report.rs`), `hook claude` / `hook copilot <event>` (run by Claude Code and Copilot CLI, see
//! `claude.rs` and `copilot.rs`) and `integration install|uninstall|status claude|copilot
//! [--settings <path>]`.

use std::path::PathBuf;
use std::process::ExitCode;

use poimen_server::{Config, Server, StartError, claude, copilot, report};

const USAGE: &str =
    "usage: poimen-server integration install|uninstall|status claude|copilot [--settings <path>]";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("--version") => {
            println!("poimen-server {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Some("hook") => {
            // Never fails and never blocks Claude Code, whatever happens.
            match args.get(1).map(String::as_str) {
                Some("claude") => claude::run_hook(
                    std::io::stdin(),
                    claude::terminal_id_from_env(),
                    poimen_protocol::socket_path(),
                ),
                Some("copilot") => copilot::run_hook(
                    std::io::stdin(),
                    args.get(2).cloned(),
                    claude::terminal_id_from_env(),
                    poimen_protocol::socket_path(),
                ),
                _ => {}
            }
            ExitCode::SUCCESS
        }
        Some("report") => ExitCode::from(report::run(
            &args[1..],
            claude::terminal_id_from_env(),
            poimen_protocol::socket_path(),
        )),
        Some("integration") => integration(&args[1..]),
        _ => serve(),
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
        _ => copilot::default_hook_path(home),
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
                copilot::install(&path, &exe)
            } else {
                claude::install(&path, &exe)
            }
        })
        .map(|()| format!("installed in {}", path.display())),
        "uninstall" => if copilot {
            copilot::uninstall(&path)
        } else {
            claude::uninstall(&path)
        }
        .map(|removed| {
            if removed {
                format!("removed from {}", path.display())
            } else {
                "nothing to remove".to_owned()
            }
        }),
        "status" => if copilot {
            copilot::status(&path).map(|e| (e, copilot::EVENTS.len()))
        } else {
            claude::status(&path).map(|e| (e, claude::EVENTS.len()))
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
