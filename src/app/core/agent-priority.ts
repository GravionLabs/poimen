import type { AgentStatus } from "./snapshot";

/** The sections of the Agents list in `priority` order (#722). */
export type AgentSection = "needs" | "done" | "working" | "idle";

export const AGENT_SECTIONS: readonly { id: AgentSection; title: string }[] = [
  { id: "needs", title: "Needs you" },
  { id: "done", title: "Done" },
  { id: "working", title: "Working" },
  { id: "idle", title: "Idle" },
];

/** The section of an agent: blocked, done and unread, working, else idle (also done and read, unknown). */
export function agentSection(status: AgentStatus, unread: boolean): AgentSection {
  if (status === "blocked" || status === "error") return "needs";
  if (status === "done" && unread) return "done";
  if (status === "working") return "working";
  return "idle";
}

const MINUTE = 60_000;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;

/** Time since a status change as `now` (under a minute), `5m`, `2h` or `3d`; a future time is `now`. */
export function formatElapsed(ms: number): string {
  if (!(ms >= MINUTE)) return "now";
  if (ms < HOUR) return `${Math.floor(ms / MINUTE)}m`;
  if (ms < DAY) return `${Math.floor(ms / HOUR)}h`;
  return `${Math.floor(ms / DAY)}d`;
}

/**
 * Orders agents by section, then the one whose status changed first, then by name (and id, so the
 * order is total). An agent without a known section counts as idle, one without a time as `now`.
 */
export function orderAgents<T>(
  items: readonly T[],
  idOf: (item: T) => string,
  nameOf: (item: T) => string,
  sections: ReadonlyMap<string, AgentSection>,
  changedAt: ReadonlyMap<string, number>,
  now: number,
): T[] {
  const rank = (item: T) =>
    AGENT_SECTIONS.findIndex((s) => s.id === (sections.get(idOf(item)) ?? "idle"));
  return [...items].sort(
    (a, b) =>
      rank(a) - rank(b) ||
      (changedAt.get(idOf(a)) ?? now) - (changedAt.get(idOf(b)) ?? now) ||
      nameOf(a).localeCompare(nameOf(b)) ||
      idOf(a).localeCompare(idOf(b)),
  );
}
