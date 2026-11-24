# Tests

What a useful test is. Read before you add or review a test. Notes on a change go here, not in `AGENTS.md`. The mechanics (Vitest, `cargo test`, read-only live tests) are in the `ng-test`, `ts-test` and `rust-test` skills and in `AGENTS.md`.

Every change ships with tests, and every test earns its place with three rules.

## Rules

1. **Revert criterion.** A test must fail when the change is reverted. Before you push, undo the production change (or break the line you wrote) and run the test: if it stays green, it tests nothing, so rewrite or delete it. A test that passes both with and without the change is noise that costs maintenance and hides the tests that matter.
2. **Written-out expectations.** The expected value is a literal in the test (`"v1:abc"`, `3`, a JSON string), not computed by the code under test or by a copy of its logic. `expect(f(x)).toBe(f(x))` and a test that rebuilds the result with the same formula can never fail.
3. **One owning layer.** Test a behavior where it lives, once. A parsing rule is tested in the parser's unit test, not again through the server, the Tauri command and the component. Higher layers get one test that the pieces are wired together (an integration or component test), not a copy of the lower cases. Wire shapes belong to `poimen-protocol` (a parsing test per shape, see `AGENTS.md`).

## What is not tested

| Not tested | Rust example | Angular example |
| --- | --- | --- |
| Constants and static data | `assert_eq!(DEFAULT_TIMEOUT_MS, 5000)` repeats the declaration. Test the behavior that uses the timeout (a request that exceeds it fails). | `expect(ACTIVITY_ENTRIES.length).toBe(6)`. Test that a view is reachable from the activity bar, not how many entries exist. |
| CSS classes and markup | A snapshot of the full rendered text of a `--help` page. Assert the one line a script depends on. | `expect(el.classList).toContain("card")` or a `querySelector(".title")` check. Assert what the user sees or can do (role, accessible name, text, a click effect). Contrast and token rules already have `*-contrast.spec.ts`. |
| Third-party code | A test that `serde_json` round-trips a `HashMap`, or that `tokio::time::timeout` times out. Test that your type serializes to the documented shape. | A test that xterm.js renders a character, or that `hx-button` emits `click`. Test your handler. |
| Private state and internals | Reaching into a private field with `#[cfg(test)]` accessors to check a cache. Observe it through the public function: the second call returns the cached value without a second request. | `component["internal"]`, or reading a private signal. Drive the input or event and assert the output or the rendered result. |
| Timing | `thread::sleep(100ms)` and then asserting. Wait on a condition (a channel message, the event) with a timeout as the safety net. `pnpm ratchets` counts waits. | `setTimeout`/real timers in a spec. Use fake timers and advance them, or `await` the condition. |

An exception is not a loophole: a constant that is a wire value or a user-visible contract (a protocol version, a shortcut default) is tested as a contract, with the literal written out, and the test says why it is frozen.

## Test directories

A test that needs a directory under `temp_dir()` creates it with `tempfile::TempDir` (a dev-dependency) and keeps the guard in a variable for as long as it uses the path: the directory is removed when the guard drops, also when the test panics. A helper returns the guard with the path (`(TempDir, PathBuf)`) or a small wrapper (`TestDir` in `src-tauri/crates/poimen-core/src/test_dir.rs` and `src-tauri/src/test_dir.rs`). Do not take the guard as a temporary (`Tracker::new(&helper())` followed by more calls): the directory is gone at the end of the statement.

A directory made by hand (`temp_dir().join(..)` and `create_dir_all`) and never removed stays on every run. On Linux `/tmp` is often a tmpfs with a fixed number of inodes: it ran out at about a million leftover files and the desktop stopped (icons gone, apps frozen). The `test-temp-dirs` ratchet (`pnpm ratchets`, no baseline) fails for a Rust test file that creates files under `temp_dir()` and has no `TempDir`, `TestDir`, `tempdir(` or `remove_dir_all`.

## Review checklist

Ask these of every diff, in this order. A "no" on the first one is blocking.

- [ ] **Revert question:** if the production change were reverted, would a test fail? Name the test. (Run the revert when it is not obvious.)
- [ ] Are the expected values literals, not computed by the code under test?
- [ ] Is each behavior tested at the one layer that owns it, without repeating lower cases higher up?
- [ ] Does any test only check a constant, a CSS class, third-party code, private state or timing?
- [ ] Is a bug fix covered by a test that failed before the fix?
- [ ] Do tests stay read-only against the user's session (`AGENTS.md` rules)?
- [ ] Does a test directory remove itself (a `TempDir` guard), also when the test fails?
