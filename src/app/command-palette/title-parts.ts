/** Splits `title` into runs of matched and unmatched characters at the matched `positions`. */
export function titleParts(
  title: string,
  positions: readonly number[],
): { text: string; hit: boolean }[] {
  const hits = new Set(positions);
  const parts: { text: string; hit: boolean }[] = [];
  for (let i = 0; i < title.length; i++) {
    const hit = hits.has(i);
    const last = parts[parts.length - 1];
    if (last && last.hit === hit) last.text += title[i];
    else parts.push({ text: title[i], hit });
  }
  return parts;
}
