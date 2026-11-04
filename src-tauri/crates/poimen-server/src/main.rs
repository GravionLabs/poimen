//! `poimen-server`: one per user, started by the app (or by hand), never stopped by the app.

use std::process::ExitCode;

use poimen_server::{Config, Server, StartError};

fn main() -> ExitCode {
    if std::env::args().nth(1).as_deref() == Some("--version") {
        println!("poimen-server {}", env!("CARGO_PKG_VERSION"));
        return ExitCode::SUCCESS;
    }
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
