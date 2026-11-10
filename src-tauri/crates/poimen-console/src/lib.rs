//! The console client of `poimen-server` (ADR 0006): `poimen attach` for one terminal and the single-request commands (`commands`: ls, status, new, send, prompt).
//!
//! `cli` parses the arguments, `keys` filters the keyboard input (detach key, UTF-8 joining),
//! `session` runs one attachment against any `Write` and input channel, `tty` is the real local
//! terminal (raw mode, size).

pub mod cli;
pub mod commands;
pub mod keys;
pub mod session;
pub mod tty;
