import { Injectable, computed, signal } from "@angular/core";
import { agentKind, type AgentStatus, type SpaceNode, type Terminal } from "./snapshot";

const same = (a: ReadonlySet<string>, b: ReadonlySet<string>) =>
  a.size === b.size && [...a].every((id) => b.has(id));

/** A pane whose agent turned blocked or finished since the previous snapshot. */
export interface StatusChange {
  paneId: string;
  status: "blocked" | "done";
}

/**
 * Unread and attention marks of agent panes. UI bookkeeping only (herdr owns pane state): a pane
 * that turns blocked while not viewed rings until viewed, one that finishes (done) while not
 * viewed counts as unread until viewed.
 */
@Injectable({ providedIn: "root" })
export class Attention {
  private previous: ReadonlyMap<string, AgentStatus> | null = null;
  private previousViewed: ReadonlySet<string> = new Set();
  /** Panes the user marked unread; they stay marked while viewed (#725). */
  private readonly manual = new Set<string>();
  /** Panes that turned blocked while no tab showed them. */
  readonly attention = signal<ReadonlySet<string>>(new Set());
  /** Panes that finished while no tab showed them. */
  readonly unread = signal<ReadonlySet<string>>(new Set());
  /** Panes with a mark (ringing or unread): what the dock badge counts. */
  readonly count = computed(() => new Set([...this.attention(), ...this.unread()]).size);
  /** The panes that turned blocked or done in the latest snapshot that had any (a new array each time). */
  readonly lastChanges = signal<readonly StatusChange[]>([]);

  /** Puts the unread mark back on a pane (not persisted); it follows the clear rules of `observe`. */
  markUnread(paneId: string): void {
    this.manual.add(paneId);
    if (this.unread().has(paneId)) return;
    this.unread.set(new Set([...this.unread(), paneId]));
  }

  /**
   * Folds a snapshot of pane statuses in; `viewed` are the panes whose tab is on screen. Returns
   * the panes that turned blocked or done (panes seen for the first time do not count).
   */
  observe(statuses: ReadonlyMap<string, AgentStatus>, viewed: ReadonlySet<string>): StatusChange[] {
    const changes: StatusChange[] = [];
    const attention = new Set(this.attention());
    const unread = new Set(this.unread());
    for (const [id, status] of statuses) {
      const before = this.previous?.get(id);
      if (before !== undefined && status !== before && (status === "blocked" || status === "done"))
        changes.push({ paneId: id, status });
      if (this.manual.has(id) && (status === "working" || status === "blocked")) {
        this.manual.delete(id);
        unread.delete(id);
      }
      if (viewed.has(id)) {
        attention.delete(id);
        // A manual mark survives while the pane stays viewed.
        if (this.manual.has(id) && this.previousViewed.has(id)) unread.add(id);
        else {
          this.manual.delete(id);
          unread.delete(id);
        }
      } else if (status !== before) {
        if (status === "blocked") attention.add(id);
        else if (status === "done" && before !== undefined) unread.add(id);
      }
      if (status !== "blocked") attention.delete(id);
    }
    for (const id of [...attention, ...unread]) {
      if (!statuses.has(id)) {
        attention.delete(id);
        unread.delete(id);
        this.manual.delete(id);
      }
    }
    this.previousViewed = new Set(viewed);
    this.previous = new Map(statuses);
    if (changes.length) this.lastChanges.set(changes);
    if (!same(attention, this.attention())) this.attention.set(attention);
    if (!same(unread, this.unread())) this.unread.set(unread);
    return changes;
  }
}

/** Unread panes of a workspace: what its row shows as the sum of its panes. */
export function workspaceUnread(space: SpaceNode, unread: ReadonlySet<string>): number {
  return space.rows.filter((row) => unread.has(row.pane.terminal_id)).length;
}

/** "Mark as unread" is offered for an agent that is done or idle and not unread already. */
export function canMarkUnread(
  pane: Terminal | undefined,
  unread: ReadonlySet<string>,
): pane is Terminal {
  return (
    !!pane &&
    agentKind(pane) !== undefined &&
    (pane.agent_status === "done" || pane.agent_status === "idle") &&
    !unread.has(pane.terminal_id)
  );
}
