// `pnpm check:changed [base]` runs the checks the changed paths need. The path rules are in scripts/changed-paths.json,
// which the `changes` job of .github/workflows/ci.yml reads too (`--github-output`).
import { execFileSync, spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const CRATES_DIR = "src-tauri/crates/";
const APP_CRATE = "poimen-app";

export function loadRules(file = path.join(root, "scripts", "changed-paths.json")) {
  const json = JSON.parse(fs.readFileSync(file, "utf8"));
  const compile = (list) => list.map((source) => new RegExp(source));
  return {
    frontend: compile(json.frontend),
    rust: compile(json.rust),
    rustAll: compile(json.rustAll),
  };
}

const matches = (patterns, files) => files.some((f) => patterns.some((re) => re.test(f)));

/** Crate name -> names of the workspace crates it depends on (path dependencies). */
export function crateDeps(cratesRoot = path.join(root, "src-tauri", "crates")) {
  const deps = new Map();
  for (const name of fs.readdirSync(cratesRoot)) {
    const manifest = path.join(cratesRoot, name, "Cargo.toml");
    if (!fs.existsSync(manifest)) continue;
    const text = fs.readFileSync(manifest, "utf8");
    deps.set(
      name,
      [...text.matchAll(/^(poimen-[\w-]+)\s*=\s*\{[^}]*path\s*=/gm)].map((m) => m[1]),
    );
  }
  deps.set(APP_CRATE, [...deps.keys()]);
  return deps;
}

/** The changed crates and every crate that depends on one of them, in name order. */
export function selectCrates(files, rules, deps) {
  if (!matches(rules.rust, files)) return [];
  if (matches(rules.rustAll, files)) return [...deps.keys()].sort();
  const changed = new Set();
  for (const f of files) {
    if (f.startsWith(CRATES_DIR)) {
      const name = f.slice(CRATES_DIR.length).split("/")[0];
      if (deps.has(name)) changed.add(name);
    } else if (f.startsWith("src-tauri/")) changed.add(APP_CRATE);
  }
  let grew = true;
  while (grew) {
    grew = false;
    for (const [name, list] of deps) {
      if (!changed.has(name) && list.some((d) => changed.has(d))) {
        changed.add(name);
        grew = true;
      }
    }
  }
  return [...changed].sort();
}

export function select(files, rules, deps) {
  const crates = selectCrates(files, rules, deps);
  return { frontend: matches(rules.frontend, files), rust: crates.length > 0, crates };
}

/** The steps to run and the ones to skip, as { name, cmd, args } and { name, reason }. */
export function plan(selection) {
  const ran = [{ name: "ratchets", cmd: "pnpm", args: ["ratchets"] }];
  const skipped = [];
  if (selection.frontend) {
    for (const s of ["check", "lint", "format:check", "test", "build"]) {
      ran.push({ name: `pnpm ${s}`, cmd: "pnpm", args: [s] });
    }
  } else {
    skipped.push({
      name: "frontend (check, lint, format:check, test, build)",
      reason: "no frontend paths changed",
    });
  }
  if (selection.rust) {
    const pkgs = selection.crates.flatMap((c) => ["-p", c]);
    const cwd = "src-tauri";
    ran.push({ name: "cargo fmt --check", cmd: "cargo", args: ["fmt", "--check"], cwd });
    ran.push({
      name: `cargo clippy (${selection.crates.join(", ")})`,
      cmd: "cargo",
      args: ["clippy", ...pkgs, "--all-targets", "--locked", "--", "-D", "warnings"],
      cwd,
    });
    ran.push({
      name: `cargo test (${selection.crates.join(", ")})`,
      cmd: "cargo",
      args: ["test", ...pkgs, "--locked"],
      cwd,
    });
  } else {
    skipped.push({ name: "rust (fmt, clippy, test)", reason: "no rust paths changed" });
  }
  return { ran, skipped };
}

function git(...args) {
  return execFileSync("git", args, { cwd: root, encoding: "utf8" }).trim();
}

/** Files changed since the merge base with `base`: committed, uncommitted and untracked. */
export function changedFiles(base) {
  const mergeBase = git("merge-base", base, "HEAD");
  const lines = [
    ...git("diff", "--name-only", mergeBase).split("\n"),
    ...git("ls-files", "--others", "--exclude-standard").split("\n"),
  ];
  return [...new Set(lines.filter(Boolean))].sort();
}

function main(argv) {
  const args = argv.filter((a) => !a.startsWith("--"));
  const rules = loadRules();
  const deps = crateDeps();
  if (argv.includes("--github-output")) {
    // CI: `changes` job. Not a pull request or no base: everything runs (decided in the workflow).
    const files = execFileSync("git", ["diff", "--name-only", args[0], "HEAD"], {
      cwd: root,
      encoding: "utf8",
    })
      .split("\n")
      .filter(Boolean);
    console.log(files.join("\n"));
    const s = select(files, rules, deps);
    const out = [`frontend=${s.frontend}`, `rust=${s.rust}`].join("\n") + "\n";
    if (process.env.GITHUB_OUTPUT) fs.appendFileSync(process.env.GITHUB_OUTPUT, out);
    else process.stdout.write(out);
    return 0;
  }
  const base = args[0] ?? "origin/release/next";
  const files = changedFiles(base);
  const { ran, skipped } = plan(select(files, rules, deps));
  console.log(`check:changed against ${base}: ${files.length} changed file(s)`);
  for (const s of skipped) console.log(`skip  ${s.name}: ${s.reason}`);
  for (const s of ran) console.log(`run   ${s.name}`);
  if (argv.includes("--dry-run")) return 0;
  const done = [];
  for (const step of ran) {
    console.log(`\n> ${step.name}`);
    const r = spawnSync(step.cmd, step.args, {
      cwd: step.cwd ? path.join(root, step.cwd) : root,
      stdio: "inherit",
      shell: process.platform === "win32",
    });
    if (r.status !== 0) {
      console.log(`\nFAILED ${step.name}`);
      console.log(`ran: ${done.join(", ") || "-"}`);
      console.log(`skipped: ${skipped.map((s) => s.name).join(", ") || "-"}`);
      return r.status ?? 1;
    }
    done.push(step.name);
  }
  console.log(`\nran: ${done.join(", ")}`);
  console.log(`skipped: ${skipped.map((s) => `${s.name} (${s.reason})`).join(", ") || "-"}`);
  return 0;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  process.exit(main(process.argv.slice(2)));
}
