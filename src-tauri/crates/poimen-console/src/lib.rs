//! The console client of `poimen-server` (ADR 0006): the TUI, `poimen attach` for one terminal and the single-request commands (`commands`: ls, status, new, send, prompt) and `mcp`, the stdio MCP server (read-only tools).
//!
//! `cli` parses the arguments, `keys` filters the keyboard input (detach key, UTF-8 joining),
//! `session` runs one attachment against any `Write` and input channel, `tty` is the real local
//! terminal (raw mode, size), `tui` the TUI of `poimen` without arguments.

pub mod cli;
pub mod commands;
pub mod doctor;
pub mod keys;
pub mod mcp;
pub mod remote;
pub mod server_start;
pub mod session;
pub mod session_run;
pub mod space_tab;
pub mod tty;
pub mod tui;
