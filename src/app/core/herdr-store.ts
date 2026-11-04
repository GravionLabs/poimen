import { DestroyRef, Injectable, computed, inject, signal } from "@angular/core";
import {
  applyStatus,
  buildSpaces,
  buildTree,
  paneDir,
  nextWaiting,
  waitingCount,
  type HerdrEvent,
  type HerdrInfo,
  type Terminal,
  type RepoInfo,
  type Snapshot,
  type WireSnapshot,
  toSnapshot,
  type BranchPull,
} from "./herdr";
import type { BackendKind } from "./settings";
import { TauriBridge, type Unlisten } from "./tauri";

/** Delay that collapses a burst of herdr events into one re-read. */
export const REFRESH_DEBOUNCE_MS = 80;

/** Interval of the repo refresh (dirty state, ahead/behind) while the window is focused. */
export const REPOS_REFRESH_MS = 60_000;

/** `reconnecting`: the event watcher lost herdr and retries; `unreachable`: a snapshot read failed. */
export type ConnectionState =
  { state: "connected" } | { state: "reconnecting" | "unreachable"; reason: string };

/** What `ensure_backend` found: `running` (already), `started`, `disabled` (`herdr.autoStart` off) or `failed`. */
export interface BackendStatus {
  state: "running" | "started" | "disabled" | "failed";
  reason: string | null;
}

/** Whether the window has focus; false outside a browser. */
const windowFocused = (): boolean => typeof document !== "undefined" && document.hasFocus();

const EMPTY: Snapshot = { spaces: [], tabs: [], terminals: [] };

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
  private readonly _watchLost = signal(false);
  private readonly _repos = signal<ReadonlyMap<string, RepoInfo>>(new Map());
  private readonly _pulls = signal<ReadonlyMap<string, BranchPull>>(new Map());

  private readonly _loaded = signal(false);
  private readonly _info = signal<HerdrInfo | null>(null);
  private readonly _appVersion = signal<string | null>(null);
  private readonly _backend = signal<BackendStatus | null>(null);
  private readonly _backendKind = signal<BackendKind>("herdr");

  readonly snapshot = this._snapshot.asReadonly();
  /** Whether a snapshot was read from herdr at least once (before that the snapshot is empty). */
  readonly loaded = this._loaded.asReadonly();
  /** Version and protocol of the running herdr; `null` when unreachable or too old. Read once per connection. */
  readonly info = this._info.asReadonly();
  /** The backend of this run (`backend` setting at start); herdr until the app has said otherwise. */
  readonly backendKind = this._backendKind.asReadonly();
  /** The last result of starting herdr; `null` before the first one. */
  readonly backend = this._backend.asReadonly();
  /** Why herdr could not be started; `null` when it ran, started or auto start is off. */
  readonly startFailure = computed(() => {
    const b = this._backend();
    return b?.state === "failed" ? (b.reason ?? "herdr could not be started") : null;
  });
  /**
   * The one message for a disconnected herdr (start failed, reconnecting, unreachable, auto start
   * off); `null` while connected, even after a failed start. A failed start wins over the connection error it causes.
   */
  readonly disconnected = computed<string | null>(() => {
    const conn = this.connection();
    if (conn.state === "connected") return null;
    const failure = this.startFailure();
    if (failure !== null) return `herdr was not started: ${failure}`;
    const reason = conn.reason ? `: ${conn.reason}` : "";
    if (conn.state === "reconnecting") return `herdr connection lost, reconnecting${reason}`;
    const off = this._backend()?.state === "disabled" ? " (auto start is off)" : "";
    return `herdr not reachable${off}${reason}`;
  });
  /** The version of this poimen build. */
  readonly appVersion = this._appVersion.asReadonly();
  /** Why herdr is not reachable; `null` while connected. */
  readonly connectionError = this._connectionError.asReadonly();
  /** Branch and worktree flag per pane directory; poimen asks git, herdr does not know them. */
  readonly connection = computed<ConnectionState>(() => {
    const reason = this._connectionError();
    if (reason === null) return { state: "connected" };
    return { state: this._watchLost() ? "reconnecting" : "unreachable", reason };
  });
  readonly repos = this._repos.asReadonly();
  /** Pull request of the branch per pane directory (`branch_pulls`). */
  readonly pulls = this._pulls.asReadonly();
  readonly tree = computed(() => buildTree(this._snapshot()));
  readonly spaces = computed(() => buildSpaces(this._snapshot(), this._repos(), this._pulls()));
  readonly waiting = computed(() => waitingCount(this._snapshot().terminals));
  readonly panes = computed(
    () => new Map(this._snapshot().terminals.map((p) => [p.terminal_id, p])),
  );

  private refreshTimer: ReturnType<typeof setTimeout> | undefined;
  private refreshing = false;
  private refreshAgain = false;
  private unlisten: Promise<Unlisten> | undefined;
  private reposKey: string | undefined;
  private reposStale = false;

  constructor() {
    this.watchRepoTriggers();
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
    void this.readAppVersion();
    await this.readBackendKind();
    const started = this.ensureBackend(false);
    await this.refresh();
    await started;
  }

  /**
   * Starts herdr when nothing answers (the backend checks the socket first and honors
   * `herdr.autoStart`), then re-reads the snapshot (at start only after a start). Runs at start and on Retry; one attempt, no loop.
   */
  async ensureBackend(retry = true): Promise<void> {
    try {
      const status = await this.bridge.invoke<BackendStatus | undefined>("ensure_backend");
      this._backend.set(status ?? null);
    } catch (err) {
      this._backend.set({ state: "failed", reason: String(err) });
    }
    // At start the first snapshot is already being read; only a started server needs another read.
    if (retry || this._backend()?.state === "started") {
      this._info.set(null);
      await this.refresh();
    }
  }

  /** The next blocked pane after `activeId`, wrapping around. */
  nextWaiting(activeId: string | null): Terminal | undefined {
    return nextWaiting(this._snapshot().terminals, activeId);
  }

  pane(paneId: string): Terminal | undefined {
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
      const snapshot = toSnapshot(await this.bridge.invoke<WireSnapshot>("snapshot"));
      this._snapshot.set(snapshot);
      this._loaded.set(true);
      if (!this._watchLost()) this._connectionError.set(null);
      if (!this._info()) await this.readInfo();
      await this.readRepos(snapshot);
    } catch (err) {
      this._watchLost.set(false);
      this._connectionError.set(String(err));
      this._info.set(null);
    } finally {
      this.refreshing = false;
      if (this.refreshAgain) {
        this.refreshAgain = false;
        void this.refresh();
      }
    }
  }

  private async readAppVersion(): Promise<void> {
    try {
      this._appVersion.set((await this.bridge.invoke<string | undefined>("app_version")) ?? null);
    } catch {
      // stays unknown
    }
  }

  /** Which backend this run talks to; a failure keeps herdr. */
  private async readBackendKind(): Promise<void> {
    try {
      const kind = await this.bridge.invoke<string | undefined>("backend_kind");
      this._backendKind.set(kind === "poimen" ? "poimen" : "herdr");
    } catch {
      // stays herdr
    }
  }

  /** Asks herdr for its version; a failure leaves it unknown and raises no error. */
  private async readInfo(): Promise<void> {
    try {
      this._info.set(
        (await this.bridge.invoke<HerdrInfo | null | undefined>("herdr_info")) ?? null,
      );
    } catch {
      this._info.set(null);
    }
  }

  /** Looks up branch and worktree of the pane directories. A failure only leaves them blank. */
  private async readRepos(snapshot: Snapshot): Promise<void> {
    const dirs = [...new Set(snapshot.terminals.map(paneDir).filter((d): d is string => !!d))];
    const key = dirs.join("\n");
    // Git runs when the pane set (its directories) changed or a trigger asked for it.
    if (!this.reposStale && key === this.reposKey) return;
    this.reposStale = false;
    try {
      const found = await this.bridge.invoke<Record<string, RepoInfo> | undefined>("repo_info", {
        dirs,
      });
      this._repos.set(new Map(Object.entries(found ?? {})));
      this.reposKey = key;
      void this.readPulls(dirs);
    } catch {
      // keep the previous values and try again at the next refresh
      this.reposKey = undefined;
    }
  }

  /**
   * Looks up the pull request of each directory's branch. It runs with the repo reads, so it has
   * the same triggers; the 30 s cache of the gh client keeps it from asking GitHub more often. A
   * failure keeps the previous values.
   */
  private async readPulls(dirs: string[]): Promise<void> {
    if (dirs.length === 0) return;
    try {
      const found = await this.bridge.invoke<Record<string, BranchPull> | undefined>(
        "branch_pulls",
        { dirs },
      );
      this._pulls.set(new Map(Object.entries(found ?? {})));
    } catch {
      // keep the previous values
    }
  }

  /** Re-reads branch, dirty state and ahead/behind at the next snapshot read. */
  refreshRepos(): void {
    this.reposStale = true;
    this.scheduleRefresh();
  }

  /** Window focus and a 60 s timer (focused windows only) refresh the repos, never in the background. */
  private watchRepoTriggers(): void {
    if (typeof window === "undefined") return;
    const onFocus = () => this.refreshRepos();
    const timer = setInterval(() => {
      if (windowFocused()) this.refreshRepos();
    }, REPOS_REFRESH_MS);
    window.addEventListener("focus", onFocus);
    inject(DestroyRef).onDestroy(() => {
      clearInterval(timer);
      window.removeEventListener("focus", onFocus);
    });
  }

  private handle(event: HerdrEvent): void {
    switch (event.kind) {
      case "poimen.connected":
        this._watchLost.set(false);
        this._connectionError.set(null);
        break;
      case "poimen.disconnected":
        this._watchLost.set(true);
        this._connectionError.set(event.data?.reason ?? "");
        this._info.set(null);
        return;
      case "pane.agent_status_changed":
        if (event.data?.pane_id && event.data.agent_status) {
          this._snapshot.update((s) =>
            applyStatus(s, event.data!.pane_id!, event.data!.agent_status!),
          );
          const status = event.data.agent_status;
          // An agent that finished may have changed the working tree; skip it in the background.
          if ((status === "done" || status === "idle") && windowFocused()) {
            this.reposStale = true;
          }
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
