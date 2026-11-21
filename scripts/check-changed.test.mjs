import assert from "node:assert/strict";
import { test } from "node:test";
import { crateDeps, loadRules, plan, select } from "./check-changed.mjs";

const rules = loadRules();
const deps = crateDeps();
const sel = (...files) => select(files, rules, deps);

test("a frontend file selects the frontend only", () => {
  assert.deepEqual(sel("src/app/app.ts"), { frontend: true, rust: false, crates: [] });
});

test("docs select nothing", () => {
  assert.deepEqual(sel("docs/agents/tests.md", "AGENTS.md"), {
    frontend: false,
    rust: false,
    crates: [],
  });
});

test("a leaf crate selects itself and its dependents", () => {
  const s = sel("src-tauri/crates/poimen-client/src/lib.rs");
  assert.equal(s.frontend, false);
  assert.deepEqual(s.crates, [
    "poimen-app",
    "poimen-client",
    "poimen-console",
    "poimen-pty-host",
    "poimen-server",
  ]);
});

test("the app crate selects only itself", () => {
  assert.deepEqual(sel("src-tauri/src/lib.rs").crates, ["poimen-app"]);
});

test("Cargo.lock, the toolchain and the workflow select every crate", () => {
  for (const f of ["src-tauri/Cargo.lock", "rust-toolchain.toml", ".github/workflows/ci.yml"]) {
    assert.equal(sel(f).crates.length, 7, f);
  }
  assert.equal(sel(".github/workflows/ci.yml").frontend, true);
});

test("the root package.json selects both areas", () => {
  const s = sel("package.json");
  assert.equal(s.frontend, true);
  assert.equal(s.rust, true);
});

test("the plan names what it ran and skipped", () => {
  const p = plan(sel("src/app/app.ts"));
  assert.deepEqual(
    p.ran.map((s) => s.name),
    ["ratchets", "pnpm check", "pnpm lint", "pnpm format:check", "pnpm test", "pnpm build"],
  );
  assert.equal(p.skipped.length, 1);
  assert.match(p.skipped[0].name, /^rust/);
});
