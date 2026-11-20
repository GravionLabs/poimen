// Pure part of the ratchets: baseline text, comparison and pruning. No file system access.

/** A baseline is one `key<TAB>count` per line; a key may contain spaces but no tab. */
export function parseBaseline(text) {
  const map = new Map();
  for (const line of text.split("\n")) {
    if (line.trim() === "") continue;
    const tab = line.lastIndexOf("\t");
    if (tab < 0) throw new Error(`baseline line without a tab: ${line}`);
    const count = Number(line.slice(tab + 1));
    if (!Number.isInteger(count) || count < 0)
      throw new Error(`baseline line without a count: ${line}`);
    map.set(line.slice(0, tab), count);
  }
  return map;
}

export function formatBaseline(map) {
  const keys = [...map.keys()].sort();
  return keys.map((k) => `${k}\t${map.get(k)}\n`).join("");
}

/**
 * Compares what is measured now with the baseline.
 * - added: an entry that is not in the baseline
 * - higher: an entry whose count is above the baseline
 * - stale: a baseline entry that is no longer needed (nothing measured for it)
 * - lower: an entry whose count is below the baseline (not a failure, `--prune` lowers it)
 */
export function compare(baseline, current) {
  const added = [];
  const higher = [];
  const stale = [];
  const lower = [];
  for (const [key, count] of current) {
    if (count <= 0) continue;
    if (!baseline.has(key)) added.push({ key, count });
    else if (count > baseline.get(key)) higher.push({ key, count, was: baseline.get(key) });
    else if (count < baseline.get(key)) lower.push({ key, count, was: baseline.get(key) });
  }
  for (const [key, was] of baseline) {
    if (!((current.get(key) ?? 0) > 0)) stale.push({ key, was });
  }
  return { added, higher, stale, lower, ok: added.length + higher.length + stale.length === 0 };
}

/** Removes entries that are no longer needed and lowers counts. Never adds an entry and never raises a count. */
export function prune(baseline, current) {
  const next = new Map();
  for (const [key, was] of baseline) {
    const now = current.get(key) ?? 0;
    if (now > 0) next.set(key, Math.min(was, now));
  }
  return next;
}
