import { Injectable, inject, signal } from "@angular/core";
import { emptySpaces, type Snapshot } from "./snapshot";
import { SpacesStore } from "./spaces-store";
import { SpaceActions, type TrustDecision, type TrustRequest } from "./space-actions";
import { TauriBridge } from "./tauri";

/** Answer of the `rebuild_space` command (`RebuildOutcome` of `lib.rs`). */
export type RebuildOutcome =
  | { state: "rebuilt"; warnings: string[]; setup_started: boolean }
  | { state: "consent"; reasons: string[] }
  | { state: "trust"; request: TrustRequest }
  | { state: "missing"; reason: string }
  | { state: "not_empty" };

/** What an empty space shows: a Rebuild action, a running rebuild or why it stays empty. */
export type RebuildMark =
  | { kind: "consent"; reasons: string[] }
  | { kind: "busy" }
  | { kind: "missing"; reason: string }
  | { kind: "failed"; reason: string };

/** Why a space waits for the user while its project file is not trusted. */
const UNTRUSTED = "the project file is not trusted yet";

/**
 * Rebuilds spaces that come back without terminals after a server restart (spaces persist,
 * terminals do not). The backend resolves the layout as for a new space (`rebuild_space`, the
 * path of `create_space`); without consent it builds only when no agent, setup or untrusted
 * project command would start, otherwise the space gets a Rebuild action. One check per backend
 * session, so a failure or a missing directory never loops.
 */
@Injectable({ providedIn: "root" })
export class SpaceRebuild {
  private readonly bridge = inject(TauriBridge);
  private readonly store = inject(SpacesStore);
  private readonly spaces = inject(SpaceActions);

  private readonly _marks = signal<ReadonlyMap<string, RebuildMark>>(new Map());
  /** The mark per space id; a space without one needs nothing. */
  readonly marks = this._marks.asReadonly();
  private stop: (() => void) | undefined;

  /** Checks the empty spaces of every backend session from now on; returns the stop. */
  start(): () => void {
    this.stop ??= this.store.onSessionStart((snapshot) => void this.check(snapshot));
    return () => {
      this.stop?.();
      this.stop = undefined;
    };
  }

  /** Tries an automatic rebuild of each space of `snapshot` that has a directory but no terminal. */
  async check(snapshot: Snapshot): Promise<void> {
    const empty = new Set(emptySpaces(snapshot).map((s) => s.space_id));
    // Marks of an earlier session are stale: the spaces are checked again.
    this._marks.update((marks) => {
      const next = new Map([...marks].filter(([id, m]) => m.kind === "busy" && empty.has(id)));
      return next.size === marks.size ? marks : next;
    });
    await Promise.all([...empty].map((id) => this.run(id, null, false)));
  }

  /** The user's "Rebuild" on an empty space: agents and setup start, the project file is gated. */
  rebuild(workspaceId: string, decision: TrustDecision | null = null): Promise<void> {
    return this.run(workspaceId, decision, true);
  }

  private async run(id: string, decision: TrustDecision | null, confirmed: boolean): Promise<void> {
    if (this._marks().get(id)?.kind === "busy") return;
    const previous = this._marks().get(id);
    this.mark(id, { kind: "busy" });
    let outcome: RebuildOutcome;
    try {
      outcome = await this.bridge.invoke<RebuildOutcome>("rebuild_space", {
        spaceId: id,
        decision,
        confirmed,
      });
    } catch (err) {
      this.mark(id, { kind: "failed", reason: String(err) });
      return;
    }
    switch (outcome.state) {
      case "rebuilt": {
        this.mark(id, null);
        const warnings = outcome.warnings ?? [];
        if (warnings.length > 0) this.spaces.notify(warnings.join("; "));
        await this.store.refresh();
        return;
      }
      case "consent":
        this.mark(id, { kind: "consent", reasons: outcome.reasons ?? [] });
        return;
      case "trust":
        // Asked once more on the next click when the dialog is cancelled.
        this.mark(
          id,
          previous?.kind === "consent" ? previous : { kind: "consent", reasons: [UNTRUSTED] },
        );
        this.spaces.askTrust(outcome.request, (answer) => this.run(id, answer, true), true);
        return;
      case "missing":
        this.mark(id, { kind: "missing", reason: outcome.reason });
        return;
      default:
        this.mark(id, null);
    }
  }

  private mark(id: string, mark: RebuildMark | null): void {
    this._marks.update((marks) => {
      const next = new Map(marks);
      if (mark) next.set(id, mark);
      else next.delete(id);
      return next;
    });
  }
}
