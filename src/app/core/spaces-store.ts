import { DestroyRef, Injectable, computed, inject, signal } from "@angular/core";
import {
  applyStatus,
  buildSpaces,
  buildTree,
  orderSpaces,
  paneDir,
  nextWaiting,
  waitingCount,
  type Terminal,
  type AgentStatus,
  type RepoInfo,
  type Snapshot,
  type WireSnapshot,
  toSnapshot,
  type BranchPull,
  mergeSnapshots,
} from "./snapshot";
import { type BackendEvent, type BackendInfo, type AppChannel } from "./backend";
import type { BackendKind } from "./settings";
import { TauriBridge, type Unlisten } from "./tauri";
import { UiState } from "./ui-state";
import { RemoteSpaces } from "./remote-spaces";

/** Delay that collapses a burst of backend events into one re-read. */
export const REFRESH_DEBOUNCE_MS = 80;

/** Interval of the repo refresh (dirty state, ahead/behind) while the window is focused. */
export const REPOS_REFRESH_MS = 60_000;

/** `reconnecting`: the event watcher lost herdr and retries; `unreachable`: a snapshot read failed. */
export type ConnectionState =
  { state: "connected" } | { state: "reconnecting" | "unreachable"; reason: string };

/**
 * What `ensure_backend` found: `running` (already), `started`, `disabled` (`herdr.autoStart` off),
 * `outdated` (a poimen-server older than the bundled one runs), `incompatible` (one with another
 * protocol version runs) or `failed`. The last two are only reported; restarting is the user's call.
 */
export interface BackendStatus {
  state: "running" | "started" | "disabled" | "outdated" | "incompatible" | "failed";
  reason: string | null;
  /** Version of the running server (`outdated`, `incompatible`). */
  running?: string;
  /** Version poimen bundles (`outdated`). */
  bundled?: string;
  /** Protocol version of the running server (`incompatible`). */
  protocol?: number;
}

/** Whether the window has focus; false outside a browser. */
const windowFocused = (): boolean => typeof document !== "undefined" && document.hasFocus();

const EMPTY: Snapshot = { spaces: [], tabs: [], terminals: [] };

/**
 * The spaces, tabs and terminals of the backend as poimen shows them. The backend owns the state:
 * the snapshot is re-read from it, events only invalidate it. `terminal.agent_status_changed` is
 * applied right away so the status dots react without waiting for the re-read.
 */
@Injectable({ providedIn: "root" })
export class SpacesStore {
  private readonly bridge = inject(TauriBridge);
  private readonly ui = inject(UiState, { optional: true });
  private readonly remote = inject(RemoteSpaces, { optional: true });

  private readonly _snapshot = signal<Snapshot>(EMPTY);
  private readonly _connectionError = signal<string | null>(null);
  private readonly _watchLost = signal(false);
  private readonly _repos = signal<ReadonlyMap<string, RepoInfo>>(new Map());
  private readonly _pulls = signal<ReadonlyMap<string, BranchPull>>(new Map());

  private readonly _loaded = signal(false);
  private readonly _info = signal<BackendInfo | null>(null);
  private readonly _appVersion = signal<string | null>(null);
  private readonly _appChannel = signal<AppChannel>("stable");
  private readonly _backend = signal<BackendStatus | null>(null);
  private readonly _backendKind = signal<BackendKind>("poimen");

  /**
   * Combines a pane's status with what poimen knows besides the hook (running agent-events runs
   * it started, `Deliver.paneStatus`); identity until set.
   */
  private readonly statusOf = signal<(terminalId: string, hook: AgentStatus) => AgentStatus>(
    (_id, hook) => hook,
  );
  /** The snapshot with the effective status on every terminal; what all views read. */
  readonly snapshot = computed(() => {
    const raw = this._snapshot();
    const statusOf = this.statusOf();
    let changed = false;
    const terminals = raw.terminals.map((t) => {
      const status = statusOf(t.terminal_id, t.agent_status);
      if (status === t.agent_status) return t;
      changed = true;
      return { ...t, agent_status: status };
    });
    return changed ? { ...raw, terminals } : raw;
  });
  /**
   * This Mac's snapshot with the spaces of the machines (#280) after it, their ids qualified by
   * the machine name. Attention, notifications, the dock badge and the tab bar read this; the
   * space tree and every space command work on `snapshot`.
   */
  readonly all = computed(() =>
    mergeSnapshots(this.snapshot(), this.remote ? [this.remote.snapshot()] : []),
  );
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
    return b?.state === "failed"
      ? (b.reason ?? `${this.backendKind()} could not be started`)
      : null;
  });
  /**
   * The one message for a disconnected herdr (start failed, reconnecting, unreachable, auto start
   * off); `null` while connected, even after a failed start. A failed start wins over the connection error it causes.
   */
  readonly disconnected = computed<string | null>(() => {
    const conn = this.connection();
    if (conn.state === "connected") return null;
    const name = this._backendKind();
    const failure = this.startFailure();
    if (failure !== null) return `${name} was not started: ${failure}`;
    const reason = conn.reason ? `: ${conn.reason}` : "";
    if (conn.state === "reconnecting") return `${name} connection lost, reconnecting${reason}`;
    const off = this._backend()?.state === "disabled" ? " (auto start is off)" : "";
    return `${name} not reachable${off}${reason}`;
  });
  /** The version of this poimen build. */
  readonly appVersion = this._appVersion.asReadonly();
  /** The release channel of this build: `rc` for the side-by-side "Poimen RC" app, else `stable`. */
  readonly appChannel = this._appChannel.asReadonly();
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
  /** The user's space order (ids); empty without UI state. Spaces outside it follow by number. */
  readonly spaceOrder = computed<readonly string[]>(() => this.ui?.spaceOrder() ?? []);
  /** The spaces in the user's order; every list, shortcut and neighbour lookup reads this. */
  readonly orderedSpaces = computed(() => orderSpaces(this._snapshot().spaces, this.spaceOrder()));
  /** The user's tab order per space (ids); tabs outside it follow by number. */
  readonly tabOrder = computed<Readonly<Record<string, readonly string[]>>>(
    () => this.ui?.tabOrder() ?? {},
  );
  readonly tree = computed(() => buildTree(this.snapshot(), this.spaceOrder(), this.tabOrder()));
  readonly spaces = computed(() =>
    buildSpaces(this.snapshot(), this._repos(), this._pulls(), this.spaceOrder(), this.tabOrder()),
  );
  /** The spaces of one machine as a tree (`machine` set on every space), last known when offline. */
  remoteTree(machine: string) {
    return this.remoteTrees().get(machine) ?? [];
  }
  private readonly remoteTrees = computed(() => {
    const snapshot = this.remote?.snapshot();
    if (!snapshot) return new Map<string, ReturnType<typeof buildTree>>();
    const trees = new Map<string, ReturnType<typeof buildTree>>();
    for (const node of buildTree(snapshot)) {
      const name = node.workspace.machine!;
      trees.set(name, [...(trees.get(name) ?? []), node]);
    }
    return trees;
  });
  readonly waiting = computed(() => waitingCount(this.all().terminals));
  readonly panes = computed(() => new Map(this.all().terminals.map((p) => [p.terminal_id, p])));

  /** Sets how a pane's hook status is combined with its runs; `App` wires `Deliver.paneStatus`. */
  setStatusOf(fn: (terminalId: string, hook: AgentStatus) => AgentStatus): void {
    this.statusOf.set(fn);
  }

  private refreshTimer: ReturnType<typeof setTimeout> | undefined;
  private refreshing = false;
  private refreshAgain = false;
  /** Callers waiting for the queued re-read. */
  private refreshWaiters: (() => void)[] = [];
  private unlisten: Promise<Unlisten> | undefined;
  private reposKey: string | undefined;
  private reposStale = false;
  /** Set until the first snapshot read of a backend session (start, reconnect, started server). */
  private freshSession = true;
  private readonly sessionListeners = new Set<(snapshot: Snapshot) => void>();

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
    this.unlisten = this.bridge.listen<BackendEvent>("herdr", (event) => this.handle(event));
    await this.unlisten;
    void this.readAppVersion();
    void this.readAppChannel();
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
    // A server that was just started has its spaces back, but only the terminals it adopted.
    if (this._backend()?.state === "started") this.freshSession = true;
    // At start the first snapshot is already being read; only a started server needs another read.
    if (retry || this._backend()?.state === "started") {
      this._info.set(null);
      await this.refresh();
    }
  }

  /**
   * Calls `listener` with the first snapshot read of every backend session: at start, after the
   * event watcher reconnected and after `ensure_backend` started the server. Returns the unsubscribe.
   */
  onSessionStart(listener: (snapshot: Snapshot) => void): () => void {
    this.sessionListeners.add(listener);
    return () => this.sessionListeners.delete(listener);
  }

  /** The next blocked pane after `activeId`, wrapping around. */
  nextWaiting(activeId: string | null): Terminal | undefined {
    return nextWaiting(this.all().terminals, activeId);
  }

  pane(paneId: string): Terminal | undefined {
    return this.panes().get(paneId);
  }

  /**
   * Re-reads the snapshot. A call during a running re-read queues exactly one more and resolves
   * once that one finished, so the caller sees a snapshot read after the call.
   */
  async refresh(): Promise<void> {
    if (this.refreshing) {
      this.refreshAgain = true;
      return new Promise<void>((resolve) => this.refreshWaiters.push(resolve));
    }
    this.refreshing = true;
    try {
      const snapshot = toSnapshot(await this.bridge.invoke<WireSnapshot>("snapshot"));
      this._snapshot.set(snapshot);
      // Closed spaces leave the stored order (not on an empty read, which may be transient).
      if (snapshot.spaces.length) this.ui?.pruneSpaceOrder(snapshot.spaces.map((s) => s.space_id));
      if (snapshot.spaces.length) {
        const alive = new Map<string, string[]>();
        for (const tab of snapshot.tabs) {
          alive.set(tab.space_id, [...(alive.get(tab.space_id) ?? []), tab.tab_id]);
        }
        this.ui?.pruneTabOrder(alive);
      }
      this._loaded.set(true);
      if (this.freshSession) {
        this.freshSession = false;
        for (const listener of this.sessionListeners) listener(snapshot);
      }
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
        const waiters = this.refreshWaiters;
        this.refreshWaiters = [];
        void this.refresh().finally(() => waiters.forEach((resolve) => resolve()));
      }
    }
  }

  private async readAppChannel(): Promise<void> {
    try {
      const channel = await this.bridge.invoke<string | undefined>("app_channel");
      this._appChannel.set(channel === "rc" ? "rc" : "stable");
    } catch {
      // stays stable
    }
  }

  private async readAppVersion(): Promise<void> {
    try {
      this._appVersion.set((await this.bridge.invoke<string | undefined>("app_version")) ?? null);
    } catch {
      // stays unknown
    }
  }

  /** Which backend this run talks to; a failure keeps poimen. */
  private async readBackendKind(): Promise<void> {
    try {
      const kind = await this.bridge.invoke<string | undefined>("backend_kind");
      this._backendKind.set(kind === "herdr" ? "herdr" : "poimen");
    } catch {
      // stays poimen
    }
  }

  /** Asks herdr for its version; a failure leaves it unknown and raises no error. */
  private async readInfo(): Promise<void> {
    try {
      this._info.set(
        (await this.bridge.invoke<BackendInfo | null | undefined>("herdr_info")) ?? null,
      );
    } catch {
      this._info.set(null);
    }
  }

  /** Looks up branch and worktree of the pane directories. A failure only leaves them blank. */
  private async readRepos(snapshot: Snapshot): Promise<void> {
    const dirs = [
      ...new Set(
        [...snapshot.spaces.map((s) => s.cwd), ...snapshot.terminals.map(paneDir)].filter(
          (d): d is string => !!d,
        ),
      ),
    ];
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

  private handle(event: BackendEvent): void {
    switch (event.kind) {
      case "poimen.connected":
        this._watchLost.set(false);
        this._connectionError.set(null);
        break;
      case "poimen.disconnected":
        this._watchLost.set(true);
        this._connectionError.set(event.data?.reason ?? "");
        this._info.set(null);
        // The server may come back restarted: its spaces, and the terminals whose hosts it adopted.
        this.freshSession = true;
        return;
      case "terminal.agent_status_changed":
        if (event.data?.terminal_id && event.data.agent_status) {
          this._snapshot.update((s) =>
            applyStatus(
              s,
              event.data!.terminal_id!,
              event.data!.agent_status!,
              event.data!.agent_message
                ? {
                    text: event.data!.agent_message,
                    at: event.data!.agent_message_at ?? Date.now(),
                  }
                : undefined,
            ),
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
