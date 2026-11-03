import { Injectable, signal } from "@angular/core";
import type { AgentStatus, SpaceNode } from "./herdr";

const same = (a: ReadonlySet<string>, b: ReadonlySet<string>) =>
  a.size === b.size && [...a].every((id) => b.has(id));

/**
 * Unread and attention marks of agent panes. UI bookkeeping only (herdr owns pane state): a pane
 * that turns blocked while not viewed rings until viewed, one that finishes (done) while not
 * viewed counts as unread until viewed.
 */
@Injectable({ providedIn: "root" })
export class Attention {
  private previous: ReadonlyMap<string, AgentStatus> | null = null;
  /** Panes that turned blocked while no tab showed them. */
  readonly attention = signal<ReadonlySet<string>>(new Set());
  /** Panes that finished while no tab showed them. */
  readonly unread = signal<ReadonlySet<string>>(new Set());

  /** Folds a snapshot of pane statuses in; `viewed` are the panes whose tab is on screen. */
  observe(statuses: ReadonlyMap<string, AgentStatus>, viewed: ReadonlySet<string>): void {
    const attention = new Set(this.attention());
    const unread = new Set(this.unread());
    for (const [id, status] of statuses) {
      const before = this.previous?.get(id);
      if (viewed.has(id)) {
        attention.delete(id);
        unread.delete(id);
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
      }
    }
    this.previous = new Map(statuses);
    if (!same(attention, this.attention())) this.attention.set(attention);
    if (!same(unread, this.unread())) this.unread.set(unread);
  }
}

/** Unread panes of a workspace: what its row shows as the sum of its panes. */
export function workspaceUnread(space: SpaceNode, unread: ReadonlySet<string>): number {
  return space.rows.filter((row) => unread.has(row.pane.pane_id)).length;
}
