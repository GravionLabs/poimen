import { DestroyRef, Injectable, inject, signal } from "@angular/core";
import { TauriBridge } from "./tauri";
import { formatTokens } from "./transcripts";

/** Token totals of an agent session (`poimen_core::transcripts::Totals`, the `tokens` event). */
export interface TokenTotals {
  input: number;
  output: number;
  cache_read: number;
  cache_creation: number;
}

const isObject = (v: unknown): v is Record<string, unknown> => typeof v === "object" && v !== null;
const count = (v: unknown): number | null =>
  typeof v === "number" && Number.isFinite(v) && v >= 0 ? v : null;

/** Four non-negative counts, else `null`. */
export function parseTotals(raw: unknown): TokenTotals | null {
  if (!isObject(raw)) return null;
  const [input, output, cache_read, cache_creation] = [
    count(raw["input"]),
    count(raw["output"]),
    count(raw["cache_read"]),
    count(raw["cache_creation"]),
  ];
  if (input === null || output === null || cache_read === null || cache_creation === null) {
    return null;
  }
  return { input, output, cache_read, cache_creation };
}

/** Reads a `tokens` event payload `{ terminal_id, totals }`; anything else is `null`. */
export function parseTokensEvent(raw: unknown): { id: string; totals: TokenTotals } | null {
  if (!isObject(raw) || typeof raw["terminal_id"] !== "string") return null;
  const totals = parseTotals(raw["totals"]);
  return totals ? { id: raw["terminal_id"], totals } : null;
}

/** Sum of the totals of several panes; `null` when none has any (a space of plain shells). */
export function sumTotals(list: readonly (TokenTotals | undefined)[]): TokenTotals | null {
  const known = list.filter((t): t is TokenTotals => t !== undefined);
  if (known.length === 0) return null;
  return known.reduce((a, t) => ({
    input: a.input + t.input,
    output: a.output + t.output,
    cache_read: a.cache_read + t.cache_read,
    cache_creation: a.cache_creation + t.cache_creation,
  }));
}

/** Everything the session has used, the four counts together. */
export function totalCount(t: TokenTotals): number {
  return t.input + t.output + t.cache_read + t.cache_creation;
}

/** Compact total of a space row: `1.2M`. */
export function compactTotal(t: TokenTotals): string {
  return formatTokens(totalCount(t));
}

/** Tooltip text: the four counts. */
export function totalsTip(t: TokenTotals): string {
  return (
    `Tokens ${formatTokens(totalCount(t))}: input ${formatTokens(t.input)}, ` +
    `output ${formatTokens(t.output)}, cache read ${formatTokens(t.cache_read)}, ` +
    `cache write ${formatTokens(t.cache_creation)}`
  );
}

/**
 * Follows the token usage of agent panes (#895): one backend watch per pane (a file watch for a
 * local pane, `agent.transcript` for a machine's), the totals by terminal id. A pane that stops
 * being followed, or whose watch fails (offline machine, no transcript), has no total.
 */
@Injectable({ providedIn: "root" })
export class TokenTotalsService {
  private readonly bridge = inject(TauriBridge);
  private readonly _totals = signal<ReadonlyMap<string, TokenTotals>>(new Map());
  readonly totals = this._totals.asReadonly();
  /** Terminal id to the session it is followed for. */
  private readonly followed = new Map<string, string>();
  private listening: Promise<() => void> | null = null;

  constructor() {
    inject(DestroyRef).onDestroy(() => {
      void this.listening?.then((stop) => stop());
      for (const id of this.followed.keys()) this.stopWatch(id);
    });
  }

  /** Follows exactly `panes` (terminal id to session id); the others are dropped. */
  async follow(panes: ReadonlyMap<string, string>): Promise<void> {
    this.listening ??= this.bridge.listen<unknown>("tokens", (raw) => {
      const event = parseTokensEvent(raw);
      if (event && this.followed.has(event.id)) this.set(event.id, event.totals);
    });
    for (const [id, session] of [...this.followed]) {
      if (panes.get(id) === session) continue;
      this.followed.delete(id);
      this.stopWatch(id);
      this.set(id, undefined);
    }
    const starts: Promise<void>[] = [];
    for (const [id, session] of panes) {
      if (this.followed.has(id)) continue;
      this.followed.set(id, session);
      starts.push(this.start(id, session));
    }
    await Promise.all(starts);
  }

  private async start(id: string, session: string): Promise<void> {
    let following: boolean;
    try {
      following = await this.bridge.invoke<boolean>("tokens_watch", { terminalId: id });
    } catch {
      following = false;
    }
    // Dropped or replaced while the request ran: the backend watch of the new one wins.
    if (this.followed.get(id) !== session) return;
    if (!following) {
      // Asked again on the next change of the panes (the transcript may appear later).
      this.followed.delete(id);
      this.set(id, undefined);
    }
  }

  private stopWatch(id: string): void {
    void this.bridge.invoke("tokens_unwatch", { terminalId: id }).catch(() => undefined);
  }

  private set(id: string, totals: TokenTotals | undefined): void {
    this._totals.update((map) => {
      if (totals === undefined && !map.has(id)) return map;
      const next = new Map(map);
      if (totals) next.set(id, totals);
      else next.delete(id);
      return next;
    });
  }
}
