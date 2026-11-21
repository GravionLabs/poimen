// The ratchets: each measures the repository and returns a Map of key to count. `fixed` ratchets have no baseline:
// any entry fails.
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";

export const MAX_LINES = 800;
export const AGENTS_MD_LIMIT = 12 * 1024;

function tracked(root) {
  const out = execFileSync("git", ["ls-files", "-z"], { cwd: root, maxBuffer: 1 << 28 }).toString(
    "utf8",
  );
  return out.split("\0").filter((f) => f !== "" && fs.existsSync(path.join(root, f)));
}

const read = (root, file) => fs.readFileSync(path.join(root, file), "utf8");
const lineCount = (text) =>
  text === "" ? 0 : text.split("\n").length - (text.endsWith("\n") ? 1 : 0);

const SOURCE = /^(src|src-tauri)\/.*\.(rs|ts|html|css)$/;
const EXCLUDED = /(^|\/)(vendor|gen|node_modules|target)\//;
const GENERATED = /\.(generated|gen)\.[a-z]+$|\.d\.ts$/;

export function maxLines(root, files = tracked(root)) {
  const map = new Map();
  for (const file of files) {
    if (!SOURCE.test(file) || EXCLUDED.test(file) || GENERATED.test(file)) continue;
    const n = lineCount(read(root, file));
    if (n > MAX_LINES) map.set(file, n);
  }
  return map;
}

const isTestFile = (file, text) =>
  /^(src|src-tauri)\//.test(file) &&
  !EXCLUDED.test(file) &&
  (/\.spec\.ts$/.test(file) ||
    (file.endsWith(".rs") && (/\/tests\//.test(file) || /#\[(cfg\(test\)|test)\]/.test(text))));

const WAIT = /\bthread::sleep\(|\bsetTimeout\(/;
const SKIP = /#\[ignore\b|\b(it|describe|test)\.skip\(|\bxit\(|\bxdescribe\(/;
const ISSUE = /#\d+/;

/** Counts of real-time waits and of skipped tests without an issue number (on the line or the line before). */
export function countTestPatterns(text) {
  const lines = text.split("\n");
  let waits = 0;
  let skips = 0;
  lines.forEach((line, i) => {
    if (WAIT.test(line)) waits += 1;
    if (SKIP.test(line) && !ISSUE.test(line) && !(i > 0 && ISSUE.test(lines[i - 1]))) skips += 1;
  });
  return { waits, skips };
}

export function testPatterns(root, files = tracked(root)) {
  const map = new Map();
  for (const file of files) {
    if (!/\.(rs|ts)$/.test(file)) continue;
    const text = read(root, file);
    if (!isTestFile(file, text)) continue;
    const { waits, skips } = countTestPatterns(text);
    if (waits > 0) map.set(`${file} waits`, waits);
    if (skips > 0) map.set(`${file} skips`, skips);
  }
  return map;
}

/** Repository paths named in a Markdown text: backticked tokens and link targets that start in a top-level entry. */
export function pathsIn(text, tops) {
  const found = new Set();
  const add = (raw) => {
    let p = raw.trim().replace(/^\.\//, "");
    p = p.replace(/[#:?].*$/, "").replace(/[.,;]+$/, "");
    if (p === "" || /[\s<>{}*$~|()[\]"'=\\]/.test(p) || p.includes("::")) return;
    const top = p.split("/")[0];
    if (!p.includes("/") || !tops.has(top)) return;
    found.add(p);
  };
  for (const m of text.matchAll(/`([^`\n]+)`/g)) add(m[1]);
  for (const m of text.matchAll(/\]\(([^)\s]+)\)/g)) if (!/^[a-z]+:/.test(m[1])) add(m[1]);
  return found;
}

export function docPaths(root, files = tracked(root)) {
  const tops = new Set(
    fs.readdirSync(root).filter((e) => e !== "node_modules" && e !== "target" && e !== ".git"),
  );
  const map = new Map();
  for (const file of files) {
    if (file !== "AGENTS.md" && !/^docs\/agents\/[^/]+\.md$/.test(file)) continue;
    const text = read(root, file);
    for (const p of pathsIn(text, tops)) {
      const direct = fs.existsSync(path.join(root, p));
      // A link target is relative to its file (`../adr/x.md` in docs/agents/).
      const relative = fs.existsSync(path.resolve(root, path.dirname(file), p));
      if (!direct && !relative) map.set(`${file} ${p}`, 1);
    }
  }
  return map;
}

export function agentsMdSize(root) {
  const size = fs.statSync(path.join(root, "AGENTS.md")).size;
  return new Map(size > AGENTS_MD_LIMIT ? [["AGENTS.md bytes (limit 12288)", size]] : []);
}

// Every exception in src-tauri/deny.toml (an advisory `ignore` entry or a `[[licenses.exceptions]]` entry) needs a
// `reason` and an `expires YYYY-MM-DD` in it, and the date must not be past. cargo-deny rejects unknown keys, so the
// date lives in the reason text.
export function denyExceptionProblems(text, today = new Date().toISOString().slice(0, 10)) {
  const map = new Map();
  const lines = text.split("\n");
  let inList = false;
  let inException = false;
  const check = (line, n) => {
    const reason = /reason\s*=\s*"([^"]*)"/.exec(line)?.[1] ?? "";
    const date = /expires (\d{4}-\d{2}-\d{2})/.exec(reason)?.[1];
    const key = `deny.toml line ${n}`;
    if (reason.trim() === "") map.set(`${key} has no reason`, 1);
    else if (!date) map.set(`${key} has no "expires YYYY-MM-DD" in its reason`, 1);
    else if (date < today) map.set(`${key} expired ${date}`, 1);
  };
  lines.forEach((raw, i) => {
    const line = raw.replace(/#.*$/, "").trim();
    if (/^ignore\s*=\s*\[/.test(line)) inList = !line.includes("]");
    else if (inList && line.startsWith("]")) inList = false;
    else if (inList && line !== "") check(line, i + 1);
    if (/^\[\[licenses\.exceptions\]\]/.test(line)) inException = true;
    else if (line.startsWith("[")) inException = false;
    else if (inException && /^(crate|name)\s*=/.test(line))
      check(lines.slice(i, i + 6).join(" "), i + 1);
  });
  return map;
}

export function denyExceptions(root) {
  const file = "src-tauri/deny.toml";
  return fs.existsSync(path.join(root, file)) ? denyExceptionProblems(read(root, file)) : new Map();
}

export const RATCHETS = [
  { name: "max-lines", measure: maxLines, fix: "split the file; do not raise the baseline" },
  {
    name: "test-patterns",
    measure: testPatterns,
    fix: "wait on a condition, not on time; give a skipped test an issue number (#123)",
  },
  { name: "doc-paths", measure: docPaths, fix: "correct the path in AGENTS.md or docs/agents/" },
  {
    name: "agents-md-size",
    measure: agentsMdSize,
    fixed: true,
    fix: "move text to docs/agents/<area>.md",
  },
  {
    name: "deny-exceptions",
    measure: denyExceptions,
    fixed: true,
    fix: 'give the exception in src-tauri/deny.toml a reason with "expires YYYY-MM-DD" in the future, or remove it',
  },
];
