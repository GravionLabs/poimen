import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { test } from "node:test";
import { countTestPatterns, denyExceptionProblems, isTrackedPath, pathsIn } from "./checks.mjs";
import { compare, formatBaseline, parseBaseline, prune } from "./core.mjs";
import { runRatchet } from "./run.mjs";

const map = (o) => new Map(Object.entries(o));

test("baseline round trip, sorted", () => {
  const text = formatBaseline(map({ "b file.rs": 2, "a.rs": 900 }));
  assert.equal(text, "a.rs\t900\nb file.rs\t2\n");
  assert.deepEqual(parseBaseline(text), map({ "a.rs": 900, "b file.rs": 2 }));
});

test("a new entry fails", () => {
  const r = compare(map({ "a.rs": 900 }), map({ "a.rs": 900, "b.rs": 801 }));
  assert.equal(r.ok, false);
  assert.deepEqual(r.added, [{ key: "b.rs", count: 801 }]);
});

test("a higher count fails", () => {
  const r = compare(map({ "a.rs": 900 }), map({ "a.rs": 901 }));
  assert.equal(r.ok, false);
  assert.deepEqual(r.higher, [{ key: "a.rs", count: 901, was: 900 }]);
});

test("a stale entry fails", () => {
  const r = compare(map({ "a.rs": 900 }), map({}));
  assert.equal(r.ok, false);
  assert.deepEqual(r.stale, [{ key: "a.rs", was: 900 }]);
});

test("equal and lower counts pass", () => {
  assert.equal(compare(map({ "a.rs": 900 }), map({ "a.rs": 900 })).ok, true);
  const r = compare(map({ "a.rs": 900 }), map({ "a.rs": 850 }));
  assert.equal(r.ok, true);
  assert.equal(r.lower.length, 1);
});

test("prune removes and lowers, never adds or raises", () => {
  const baseline = map({ gone: 5, lower: 9, same: 3, higher: 4 });
  const current = map({ lower: 7, same: 3, higher: 6, fresh: 2 });
  assert.deepEqual(prune(baseline, current), map({ lower: 7, same: 3, higher: 4 }));
});

test("prune on disk leaves a new entry failing", () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "ratchets-"));
  fs.writeFileSync(path.join(dir, "demo.txt"), "gone\t5\nkept\t3\n");
  const ratchet = { name: "demo", measure: () => map({ kept: 2, fresh: 1 }) };
  const r = runRatchet(ratchet, { prune: true, dir });
  assert.equal(fs.readFileSync(path.join(dir, "demo.txt"), "utf8"), "kept\t2\n");
  assert.equal(r.ok, false);
  assert.deepEqual(r.added, [{ key: "fresh", count: 1 }]);
});

test("a fixed ratchet ignores a baseline", () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "ratchets-"));
  fs.writeFileSync(path.join(dir, "size.txt"), "AGENTS.md\t99999\n");
  const r = runRatchet(
    { name: "size", fixed: true, measure: () => map({ "AGENTS.md": 99999 }) },
    { dir },
  );
  assert.equal(r.ok, false);
});

test("test patterns count waits and unnumbered skips", () => {
  const text = [
    "std::thread::sleep(d);",
    "setTimeout(() => {}, 5);",
    "#[ignore]",
    '#[ignore = "live"]',
    "// #12 needs a server",
    "#[ignore]",
    "it.skip('x', () => {});",
    "it.skip('y'); // #99",
  ].join("\n");
  assert.deepEqual(countTestPatterns(text), { waits: 2, skips: 3 });
});

test("doc paths: backticked paths and link targets in repository directories only", () => {
  const tops = new Set(["src", "docs"]);
  const text =
    "`src/app/a.ts` `src/x/{a,b}.ts` `core/b.ts` `src/app/c.ts:12` [adr](../adr/x.md) `docs/p.md`";
  assert.deepEqual([...pathsIn(text, tops)].sort(), ["docs/p.md", "src/app/a.ts", "src/app/c.ts"]);
});

test("doc paths: only tracked files and their directories exist, not ignored local ones", () => {
  const files = ["src/app/a.ts", ".claude/settings.json", "docs/agents/x.md"];
  assert.equal(isTrackedPath("src/app/a.ts", files), true);
  assert.equal(isTrackedPath("src/app", files), true);
  assert.equal(isTrackedPath("src/app/", files), true);
  assert.equal(isTrackedPath("./src/app", files), true);
  assert.equal(isTrackedPath("src/ap", files), false);
  assert.equal(isTrackedPath(".claude/worktrees", files), false);
});

test("deny exceptions need a reason and a future expiry", () => {
  const ok = 'ignore = [\n  { id = "RUSTSEC-1", reason = "no fix, expires 2027-01-01" },\n]\n';
  assert.equal(denyExceptionProblems(ok, "2026-11-21").size, 0);
  assert.equal(denyExceptionProblems(ok, "2027-01-02").size, 1);
  const noDate = 'ignore = [\n  { id = "RUSTSEC-1", reason = "no fix" },\n]\n';
  assert.equal(denyExceptionProblems(noDate, "2026-11-21").size, 1);
  const noReason = 'ignore = [\n  "RUSTSEC-1",\n]\n';
  assert.equal(denyExceptionProblems(noReason, "2026-11-21").size, 1);
  const lic = '[[licenses.exceptions]]\ncrate = "x"\nallow = ["GPL-3.0"]\n';
  assert.equal(denyExceptionProblems(lic, "2026-11-21").size, 1);
  assert.equal(denyExceptionProblems('ignore = [\n  # { id = "x" },\n]\n').size, 0);
});
