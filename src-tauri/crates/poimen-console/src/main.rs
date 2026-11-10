use std::io::{Read, Write};
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc;
use std::thread;

use poimen_client::Client;
use poimen_console::cli::{self, Command, USAGE};
use poimen_console::commands;
use poimen_console::session::{self, Options, Outcome};
use poimen_console::tty::{LocalTerminal, RawGuard};
use poimen_console::tui::{self, screen::LocalScreen, screen::ScreenGuard};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match cli::parse(&args) {
        Ok(Command::Help) => {
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        Ok(Command::Tui) => tui(),
        Ok(Command::Attach(a)) => attach(a),
        Ok(Command::Request(r)) => {
            let client = Client::new(poimen_protocol::terminal_socket_path());
            match commands::run(&client, &r, commands::PROMPT_WAIT, &mut std::io::stdout()) {
                Ok(()) => ExitCode::SUCCESS,
                Err(commands::Failure(m)) => {
                    eprintln!("poimen: {m}");
                    ExitCode::FAILURE
                }
            }
        }
        Err(e) => {
            eprintln!("poimen: {e}\n{USAGE}");
            ExitCode::from(2)
        }
    }
}

fn attach(a: cli::AttachArgs) -> ExitCode {
    let client = Client::new(poimen_protocol::terminal_socket_path());
    let stop = Arc::new(AtomicBool::new(false));
    // Raw mode turns Ctrl+C into a byte for the terminal; these arrive from outside (kill, a
    // closing window) and make the session end through its normal path, which restores the mode.
    #[cfg(unix)]
    for sig in [
        signal_hook::consts::SIGTERM,
        signal_hook::consts::SIGHUP,
        signal_hook::consts::SIGINT,
    ] {
        let _ = signal_hook::flag::register(sig, Arc::clone(&stop));
    }
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let mut stdin = std::io::stdin().lock();
        let mut buf = [0u8; 1024];
        while let Ok(n) = stdin.read(&mut buf) {
            if n == 0 || tx.send(buf[..n].to_vec()).is_err() {
                return;
            }
        }
    });
    let opts = Options {
        terminal_id: a.terminal_id,
        mode: a.mode,
        take_over: a.take_over,
        // `console.prefix` when it is a Ctrl+letter key (a byte), else the default Ctrl+B.
        prefix: tui::config::load(std::env::home_dir().as_deref())
            .prefix
            .ctrl_byte()
            .unwrap_or(poimen_console::keys::DEFAULT_PREFIX),
    };
    let mut tty = LocalTerminal;
    let result = {
        let guard = match RawGuard::new(&mut tty) {
            Ok(g) => g,
            Err(e) => {
                eprintln!("poimen: this is not a terminal: {e}");
                return ExitCode::FAILURE;
            }
        };
        let size = || guard.size();
        let mut out = std::io::stdout();
        session::run(&client, &opts, rx, &mut out, &size, &stop)
    };
    let _ = std::io::stdout().flush();
    match result {
        Ok(Outcome::Detached) => {
            eprintln!("[detached; the terminal keeps running]");
            ExitCode::SUCCESS
        }
        Ok(Outcome::Exited(code)) => {
            let with = code.map(|c| format!(" with {c}")).unwrap_or_default();
            eprintln!("[terminal exited{with}]");
            ExitCode::SUCCESS
        }
        Ok(Outcome::ServerDetached(reason)) => {
            eprintln!("[detached by the server: {reason:?}]");
            ExitCode::FAILURE
        }
        Ok(Outcome::Interrupted) => {
            eprintln!("[interrupted]");
            ExitCode::FAILURE
        }
        Ok(Outcome::Closed(e)) => {
            eprintln!("[connection ended: {e}]");
            ExitCode::FAILURE
        }
        Err(e) => {
            eprintln!("poimen: {e}");
            ExitCode::FAILURE
        }
    }
}

/// The TUI on the local terminal, until detach, a signal or the server going away.
fn tui() -> ExitCode {
    let client = Client::new(poimen_protocol::terminal_socket_path());
    let home = std::env::home_dir().filter(|h| !h.as_os_str().is_empty());
    let opts = tui::Options {
        config: tui::config::load(home.as_deref()),
        home: home.map(|h| h.display().to_string()),
        own_terminal: std::env::var(poimen_protocol::ENV_TERMINAL_ID)
            .ok()
            .filter(|id| !id.is_empty()),
        mirror: None,
    };
    let stop = Arc::new(AtomicBool::new(false));
    // Raw mode turns Ctrl+C into a key for the terminal; these arrive from outside (kill, a
    // closing window) and end the TUI through its normal path, which restores the local terminal.
    #[cfg(unix)]
    for sig in [
        signal_hook::consts::SIGTERM,
        signal_hook::consts::SIGHUP,
        signal_hook::consts::SIGINT,
    ] {
        let _ = signal_hook::flag::register(sig, Arc::clone(&stop));
    }
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        while let Ok(ev) = crossterm::event::read() {
            if tx.send(ev).is_err() {
                return;
            }
        }
    });
    tui::screen::install_panic_hook();
    let mut screen = LocalScreen;
    let result = {
        let _guard = match ScreenGuard::new(&mut screen) {
            Ok(g) => g,
            Err(e) => {
                eprintln!("poimen: this is not a terminal: {e}");
                return ExitCode::FAILURE;
            }
        };
        match ratatui::Terminal::new(ratatui::backend::CrosstermBackend::new(std::io::stdout())) {
            Ok(mut terminal) => tui::run(&client, &opts, rx, &mut terminal, &stop),
            Err(e) => Err(session::Error::Io(e)),
        }
    };
    match result {
        Ok(tui::Outcome::Detached) => {
            eprintln!("[detached; the terminals keep running]");
            ExitCode::SUCCESS
        }
        Ok(tui::Outcome::Interrupted) => {
            eprintln!("[interrupted; the terminals keep running]");
            ExitCode::FAILURE
        }
        Ok(tui::Outcome::ServerGone(e)) => {
            eprintln!("[connection to poimen-server ended: {e}]");
            ExitCode::FAILURE
        }
        Err(e) => {
            eprintln!("poimen: {e}");
            ExitCode::FAILURE
        }
    }
}
