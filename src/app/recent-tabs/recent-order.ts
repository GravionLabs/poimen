/** Moves `id` to the front of the most-recently-used order. */
export function touch(order: readonly string[], id: string): string[] {
  return [id, ...order.filter((x) => x !== id)];
}

/** The tabs in the order the switcher lists them: used most recently first, never-used last. */
export function switchList(order: readonly string[], open: readonly string[]): string[] {
  const live = new Set(open);
  const known = order.filter((id) => live.has(id));
  const seen = new Set(known);
  return [...known, ...open.filter((id) => !seen.has(id))];
}

/** Next index, wrapping around. */
export function stepIndex(index: number, delta: number, length: number): number {
  return length === 0 ? 0 : (((index + delta) % length) + length) % length;
}

/** `KeyboardEvent.key` of the modifier whose release ends the switch; `null` without one. */
export function holdKey(chord: { ctrl: boolean; alt: boolean; meta: boolean }): string | null {
  if (chord.ctrl) return "Control";
  if (chord.alt) return "Alt";
  if (chord.meta) return "Meta";
  return null;
}
