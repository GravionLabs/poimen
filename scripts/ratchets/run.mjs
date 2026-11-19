// `pnpm ratchets` runs every ratchet against its baseline; `pnpm ratchets --prune` also removes or lowers baseline
// entries that are no longer needed (it never adds an entry or raises a count).
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { RATCHETS } from "./checks.mjs";
import { compare, formatBaseline, parseBaseline, prune } from "./core.mjs";

const here = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(here, "..", "..");

export function runRatchet(
  ratchet,
  { prune: doPrune = false, dir = path.join(here, "baselines"), repo = root } = {},
) {
  const file = path.join(dir, `${ratchet.name}.txt`);
  const baseline =
    ratchet.fixed || !fs.existsSync(file)
      ? new Map()
      : parseBaseline(fs.readFileSync(file, "utf8"));
  const current = ratchet.measure(repo);
  let result = compare(baseline, current);
  if (doPrune && !ratchet.fixed) {
    const next = prune(baseline, current);
    fs.writeFileSync(file, formatBaseline(next));
    result = compare(next, current);
  }
  return result;
}

function main() {
  const doPrune = process.argv.includes("--prune");
  let failed = false;
  for (const ratchet of RATCHETS) {
    const r = runRatchet(ratchet, { prune: doPrune });
    const problems = [
      ...r.added.map((e) => `new: ${e.key} (${e.count})`),
      ...r.higher.map((e) => `higher: ${e.key} (${e.was} -> ${e.count})`),
      ...r.stale.map((e) => `no longer needed: ${e.key} (run pnpm ratchets --prune)`),
    ];
    if (problems.length === 0) {
      const hint =
        r.lower.length > 0 ? ` (${r.lower.length} lower, run pnpm ratchets --prune)` : "";
      console.log(`ok   ${ratchet.name}${hint}`);
      continue;
    }
    failed = true;
    console.log(`FAIL ${ratchet.name}: ${ratchet.fix}`);
    for (const p of problems) console.log(`  ${p}`);
  }
  if (failed) process.exit(1);
}

if (process.argv[1] === fileURLToPath(import.meta.url)) main();
