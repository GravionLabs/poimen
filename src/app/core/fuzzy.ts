/**
 * Fuzzy match of `query` against `text`: every query character must occur in order (case
 * insensitive). Returns a score (higher is better) or `null` for no match. Consecutive characters
 * and characters at the start of a word score higher, and so do earlier matches.
 */
export function fuzzyScore(query: string, text: string): number | null {
  const q = query.toLowerCase().replace(/\s+/g, "");
  if (q === "") return 0;
  const t = text.toLowerCase();
  let score = 0;
  let from = 0;
  let previous = -2;
  for (const ch of q) {
    const at = t.indexOf(ch, from);
    if (at === -1) return null;
    const wordStart = at === 0 || /[^a-z0-9]/.test(t[at - 1]);
    score += 1 + (at === previous + 1 ? 5 : 0) + (wordStart ? 3 : 0) - Math.min(at, 20) / 20;
    previous = at;
    from = at + 1;
  }
  return score;
}

/** The items that match `query`, best first; the input order breaks ties and rules an empty query. */
export function fuzzyFilter<T>(items: readonly T[], query: string, text: (item: T) => string): T[] {
  return items
    .map((item, index) => ({ item, index, score: fuzzyScore(query, text(item)) }))
    .filter((r): r is { item: T; index: number; score: number } => r.score !== null)
    .sort((a, b) => b.score - a.score || a.index - b.index)
    .map((r) => r.item);
}
