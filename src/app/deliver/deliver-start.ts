import type { Terminal } from "../core/snapshot";
import type { TrustRequest } from "../core/space-actions";
import { runState, startedAt, type DeliverCard } from "./deliver-status";

/** What a loop runs on (the wire shape of `Target` in `deliver_start.rs`). */
export type DeliverTarget =
  | { kind: "milestone"; name: string }
  | { kind: "issues"; numbers: number[] };

/** The values of the dialog (the wire shape of `Request` in `deliver_start.rs`). */
export interface DeliverRequest {
  target: DeliverTarget;
  harness?: string;
  model?: string;
  max?: number;
}

/** What `deliver_plan` answers. */
export interface DeliverPlan {
  available: boolean;
  base: string | null;
  command: string;
  error: string | null;
  tab: string;
}

/** What `deliver_start` answers (`Outcome` of `deliver_start.rs`). */
export type DeliverOutcome =
  | { kind: "started"; terminal_id: string }
  | { kind: "ask"; request: TrustRequest }
  | { kind: "declined" }
  | { kind: "missing" };

/** The harnesses the loop knows; an empty choice leaves the loop's default. */
export const HARNESSES = ["claude", "copilot"] as const;

/** The text the dialog shows when `deliver-loop` is not installed. */
export const MISSING_TEXT =
  "deliver-loop is not on the PATH. Install it with /skills-manage delivery.";

const MILESTONE = /^M\d{1,6}$/;

/** The `M<nnn>` a milestone title or id starts with (`M016 (0.34.0)` gives `M016`), else null. */
export function milestoneName(text: string): string | null {
  const name = /^M\d{1,6}(?![\w.])/.exec(text.trim())?.[0];
  return name ?? null;
}

/** Why `target` cannot start a loop, else null. */
export function targetError(target: DeliverTarget): string | null {
  if (target.kind === "milestone") {
    return MILESTONE.test(target.name) ? null : "Pick a milestone named M<nnn>.";
  }
  const ok =
    target.numbers.length > 0 && target.numbers.every((n) => Number.isSafeInteger(n) && n > 0);
  return ok ? null : "Issue numbers are positive whole numbers.";
}

/** The `--max` text as a number: empty leaves the default (`undefined`), else a positive integer. */
export function parseMax(text: string): { ok: true; value?: number } | { ok: false } {
  const t = text.trim();
  if (t === "") return { ok: true };
  if (!/^\d{1,6}$/.test(t) || Number(t) < 1) return { ok: false };
  return { ok: true, value: Number(t) };
}

/** "M016" or "#12, #15". */
export function targetText(target: DeliverTarget): string {
  return target.kind === "milestone" ? target.name : target.numbers.map((n) => `#${n}`).join(", ");
}

/** The request of the dialog: empty harness, model and max stay out. */
export function requestOf(
  target: DeliverTarget,
  form: { harness: string; model: string; max: string },
): DeliverRequest | null {
  const max = parseMax(form.max);
  if (!max.ok || targetError(target)) return null;
  const harness = (HARNESSES as readonly string[]).includes(form.harness) ? form.harness : "";
  const model = form.model.trim();
  return {
    target,
    ...(harness ? { harness } : {}),
    ...(model ? { model } : {}),
    ...(max.value === undefined ? {} : { max: max.value }),
  };
}

/** A start by this action whose terminal may not have written its run file yet. */
export interface Launched {
  terminalId: string;
  /** `Date.now()` of the start. */
  at: number;
}

/** What stands in the way of a second loop for a repository. */
export type ActiveLoop = { kind: "running" } | { kind: "pending"; terminalId: string };

/**
 * The loop of the repository at `root` on this Mac that a second start must not duplicate: a
 * running, not stale run, else a start by this action whose terminal still exists and whose run
 * file has not appeared yet (no run of the repository started since). `undefined` when none.
 */
export function activeLoop(
  cards: readonly DeliverCard[],
  launched: Launched | undefined,
  terminals: readonly Pick<Terminal, "terminal_id">[],
  root: string,
  now: number,
): ActiveLoop | undefined {
  const mine = cards.filter((c) => c.root === root && c.machine === undefined);
  const running = mine.some(
    (c) => c.state.kind === "ok" && runState(c.state.status, now) === "running",
  );
  if (running) return { kind: "running" };
  if (!launched || !terminals.some((t) => t.terminal_id === launched.terminalId)) return undefined;
  const appeared = mine.some((c) => {
    if (c.state.kind !== "ok") return false;
    const start = Date.parse(startedAt(c.state.status) ?? c.state.status.updated);
    // Allows a few seconds of clock skew between the app and the loop.
    return Number.isFinite(start) && start >= launched.at - 5_000;
  });
  return appeared ? undefined : { kind: "pending", terminalId: launched.terminalId };
}

/** The refusal text of a second start. */
export function activeText(active: ActiveLoop): string {
  return active.kind === "running"
    ? "A loop is already running for this repository."
    : "A loop was just started for this repository; its run has not appeared yet.";
}
