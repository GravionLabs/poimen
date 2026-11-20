import { splitId } from "./snapshot";

/**
 * The space ids whose collapsed marks to keep (#938): the ones that exist, plus the marks of a
 * machine that lists no space yet (not read, or offline without a last snapshot), so a restart
 * does not forget them.
 */
export function keptCardIds(existing: readonly string[], marks: readonly string[]): string[] {
  const machines = new Set(existing.map((id) => splitId(id).machine));
  const unread = marks.filter((id) => {
    const machine = splitId(id).machine;
    return machine !== undefined && !machines.has(machine);
  });
  return [...existing, ...unread];
}

/** The non-empty strings of a stored list, once each; `undefined` when there are none. */
export function idList(raw: unknown): string[] | undefined {
  if (!Array.isArray(raw)) return undefined;
  const ids = raw.filter((k): k is string => typeof k === "string" && k !== "");
  return ids.length ? [...new Set(ids)] : undefined;
}

/** `idList` of every value; the keys without ids are left out. */
export function idLists(raw: Record<string, unknown>): Record<string, string[]> {
  const out: Record<string, string[]> = {};
  for (const [key, value] of Object.entries(raw)) {
    const ids = idList(value);
    if (ids) out[key] = ids;
  }
  return out;
}

/** `marks` with `id` added or removed. */
export function withMark(marks: readonly string[], id: string, on: boolean): string[] {
  return on ? [...marks, id] : marks.filter((m) => m !== id);
}
