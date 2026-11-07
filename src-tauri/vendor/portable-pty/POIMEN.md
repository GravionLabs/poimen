# portable-pty 0.9.0 for poimen (vendored, #455)

Copy of `portable-pty` 0.9.0 (MIT, wezterm) used through `[patch.crates-io]` in `src-tauri/Cargo.toml`.
Differences from upstream, nothing else:

- `src/win/psuedocon.rs`: `PsuedoCon::new` creates the pseudo console with
  `PSEUDOCONSOLE_PASSTHROUGH_MODE` added to the upstream flags and retries with the upstream
  flags when the OS rejects it. Why: without pass-through conhost re-encodes VT input, and SGR
  mouse reports never reach Copilot CLI on Windows.
- `src/conpty_flags.rs` (new, all platforms): the flag sets, so they can be tested on any host.
- Examples, `Cargo.lock` and package metadata files removed.

Drop this copy when upstream passes the flag (or offers an option) and update `portable-pty` in
`crates/poimen-server/Cargo.toml`.
