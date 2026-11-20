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
