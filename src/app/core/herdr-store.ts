import { DestroyRef, Injectable, computed, inject, signal } from "@angular/core";
import {
  applyStatus,
  buildSpaces,
  buildTree,
  paneDir,
  waitingCount,
  type HerdrEvent,
  type Pane,
  type RepoInfo,
  type Snapshot,
} from "./herdr";
import { TauriBridge, type Unlisten } from "./tauri";

/** Delay that collapses a burst of herdr events into one re-read. */
export const REFRESH_DEBOUNCE_MS = 80;

const EMPTY: Snapshot = { workspaces: [], tabs: [], panes: [] };

/**
 * The herdr state as poimen shows it. herdr owns the state: the snapshot is re-read from herdr,
 * events only invalidate it (see the herdr socket API docs). `pane.agent_status_changed` is applied
 * right away so the status dots react without waiting for the re-read.
 */
@Injectable({ providedIn: "root" })
export class HerdrStore {
  private readonly bridge = inject(TauriBridge);

  private readonly _snapshot = signal<Snapshot>(EMPTY);
  private readonly _connectionError = signal<string | null>(null);
  private readonly _repos = signal<ReadonlyMap<string, RepoInfo>>(new Map());

  readonly snapshot = this._snapshot.asReadonly();
  /** Why herdr is not reachable; `null` while connected. */
  readonly connectionError = this._connectionError.asReadonly();
  /** Branch and worktree flag per pane directory; poimen asks git, herdr does not know them. */
  readonly repos = this._repos.asReadonly();
  readonly tree = computed(() => buildTree(this._snapshot()));
  readonly spaces = computed(() => buildSpaces(this._snapshot(), this._repos()));
  readonly waiting = computed(() => waitingCount(this._snapshot().panes));
  readonly panes = computed(() => new Map(this._snapshot().panes.map((p) => [p.pane_id, p])));

  private refreshTimer: ReturnType<typeof setTimeout> | undefined;
  private refreshing = false;
  private refreshAgain = false;
  private unlisten: Promise<Unlisten> | undefined;

  constructor() {
    inject(DestroyRef).onDestroy(() => {
      clearTimeout(this.refreshTimer);
      void this.unlisten?.then((stop) => stop());
    });
  }

  /** Subscribes to herdr events and loads the first snapshot. Calling it again does nothing. */
  async start(): Promise<void> {
    if (this.unlisten) return;
    this.unlisten = this.bridge.listen<HerdrEvent>("herdr", (event) => this.handle(event));
    await this.unlisten;
    await this.refresh();
  }

  pane(paneId: string): Pane | undefined {
    return this.panes().get(paneId);
  }

  /** Re-reads the snapshot. A call during a running re-read queues exactly one more. */
  async refresh(): Promise<void> {
    if (this.refreshing) {
      this.refreshAgain = true;
      return;
    }
    this.refreshing = true;
    try {
      const snapshot = await this.bridge.invoke<Snapshot>("snapshot");
      this._snapshot.set(snapshot);
      await this.readRepos(snapshot);
    } catch (err) {
      this._connectionError.set(String(err));
    } finally {
      this.refreshing = false;
      if (this.refreshAgain) {
        this.refreshAgain = false;
        void this.refresh();
      }
    }
  }

  /** Looks up branch and worktree of the pane directories. A failure only leaves them blank. */
  private async readRepos(snapshot: Snapshot): Promise<void> {
    const dirs = [...new Set(snapshot.panes.map(paneDir).filter((d): d is string => !!d))];
    try {
      const found = await this.bridge.invoke<Record<string, RepoInfo> | undefined>("repo_info", {
        dirs,
      });
      this._repos.set(new Map(Object.entries(found ?? {})));
    } catch {
      // keep the previous values
    }
  }

  private handle(event: HerdrEvent): void {
    switch (event.kind) {
      case "poimen.connected":
        this._connectionError.set(null);
        break;
      case "poimen.disconnected":
        this._connectionError.set(event.data?.reason ?? "");
        return;
      case "pane.agent_status_changed":
        if (event.data?.pane_id && event.data.agent_status) {
          this._snapshot.update((s) =>
            applyStatus(s, event.data!.pane_id!, event.data!.agent_status!),
          );
        }
        break;
    }
    this.scheduleRefresh();
  }

  private scheduleRefresh(): void {
    clearTimeout(this.refreshTimer);
    this.refreshTimer = setTimeout(() => void this.refresh(), REFRESH_DEBOUNCE_MS);
  }
}
