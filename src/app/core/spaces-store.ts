import { DestroyRef, Injectable, computed, inject, signal } from "@angular/core";
import {
  applyStatus,
  buildSpaces,
  buildTree,
  type SpaceNode,
  NO_AGENT_RANK,
  orderSpaces,
  spaceAttentionRanks,
  type SortKeys,
  paneDir,
  agentKind,
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
import { SettingsStore, type SidebarSort } from "./settings";
import { TauriBridge, type Unlisten } from "./tauri";
import { UiState } from "./ui-state";
import { Attention } from "./attention";
import { agentSection, orderAgents, type AgentSection } from "./agent-priority";
import { SortSettle } from "./sort-settle";
import { keptCardIds } from "./card-marks";
import { RemoteSpaces } from "./remote-spaces";

/** Delay that collapses a burst of backend events into one re-read. */
export const REFRESH_DEBOUNCE_MS = 80;

/** Interval of the repo refresh (dirty state, ahead/behind) while the window is focused. */
export const REPOS_REFRESH_MS = 60_000;

/** `reconnecting`: the event watcher lost the server and retries; `unreachable`: a snapshot read failed. */
export type ConnectionState =
  { state: "connected" } | { state: "reconnecting" | "unreachable"; reason: string };

/**
 * What `ensure_backend` found: `running` (already), `started`,
 * `outdated` (a poimen-server older than the bundled one runs), `incompatible` (one with another
 * protocol version runs) or `failed`. The last two are only reported; restarting is the user's call.
 */
export interface BackendStatus {
  state: "running" | "started" | "outdated" | "incompatible" | "failed";
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
  private readonly settings = inject(SettingsStore, { optional: true });
  private readonly attention = inject(Attention, { optional: true });

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
  /** Whether a snapshot was read from the server at least once (before that the snapshot is empty). */
  readonly loaded = this._loaded.asReadonly();
  /** Version and protocol of the running server; `null` when unreachable or too old. Read once per connection. */
  readonly info = this._info.asReadonly();
  /** The last result of starting the server; `null` before the first one. */
  readonly backend = this._backend.asReadonly();
  /** Why the server could not be started; `null` when it ran or started. */
  readonly startFailure = computed(() => {
    const b = this._backend();
    return b?.state === "failed" ? (b.reason ?? "poimen could not be started") : null;
  });
  /**
   * The one message for a disconnected server (start failed, reconnecting, unreachable); `null` while connected, even after a failed start. A failed start wins over the connection error it causes.
   */
  readonly disconnected = computed<string | null>(() => {
    const conn = this.connection();
    if (conn.state === "connected") return null;
    const name = "poimen";
    const failure = this.startFailure();
    if (failure !== null) return `${name} was not started: ${failure}`;
    const reason = conn.reason ? `: ${conn.reason}` : "";
    if (conn.state === "reconnecting") return `${name} connection lost, reconnecting${reason}`;
    return `${name} not reachable${reason}`;
  });
  /** The version of this poimen build. */
  readonly appVersion = this._appVersion.asReadonly();
  /** The release channel of this build: `rc` for the side-by-side "Poimen RC" app, else `stable`. */
  readonly appChannel = this._appChannel.asReadonly();
  /** Why the server is not reachable; `null` while connected. */
  readonly connectionError = this._connectionError.asReadonly();
  /** Branch and worktree flag per pane directory; poimen asks git. */
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
  /** The pinned local spaces (ids), shown first in every sort mode (#710). */
  readonly pinnedSpaces = computed<readonly string[]>(() => this.ui?.pinnedSpaces() ?? []);
  /** `sidebar.sort`: `manual` uses the user's order, `name` sorts alphabetically. */
  readonly sidebarSort = computed<SidebarSort>(
    () => this.settings?.settings().sidebarSort ?? "manual",
  );
  /** Attention rank per space as the statuses are now (only computed in `attention` mode). */
  private readonly liveRanks = computed<ReadonlyMap<string, number>>(() => {
    if (this.sidebarSort() !== "attention") return new Map();
    const all = this.all();
    const ranks = spaceAttentionRanks(all.terminals, this.attention?.unread() ?? new Set());
    return new Map(all.spaces.map((s) => [s.space_id, ranks.get(s.space_id) ?? NO_AGENT_RANK]));
  });
  /**
   * The ranks the tree is sorted by: `liveRanks`, held back while statuses still change (#712).
   * Evaluated lazily; the timer only asks it to look again once the settle time has passed.
   */
  private readonly settledRanks = computed<ReadonlyMap<string, number>>(() => {
    const live = this.liveRanks();
    const mode = this.sidebarSort();
    this.settleTick();
    clearTimeout(this.settleTimer);
    if (this.sortHeld()) return this.shownRanks;
    const result = this.settle.update(live, mode !== this.settleMode);
    this.settleMode = mode;
    this.shownRanks = result.keys;
    if (result.wait > 0) {
      this.settleTimer = setTimeout(() => this.settleTick.update((n) => n + 1), result.wait);
    }
    return result.keys;
  });
  private readonly settleTick = signal(0);
  private shownRanks: ReadonlyMap<string, number> = new Map();
  private readonly settle = new SortSettle<number>(() => Date.now());
  private settleTimer: ReturnType<typeof setTimeout> | undefined;
  private settleMode: SidebarSort | undefined;
  /** Whether the Agents section is ordered by priority (`sidebar.agents`, #722). */
  readonly agentsPriority = computed(() => this.settings?.settings().sidebarAgents === "priority");
  /** The section of every agent as the statuses are now (only computed in `priority` mode). */
  private readonly liveAgentSections = computed<ReadonlyMap<string, AgentSection>>(() => {
    if (!this.agentsPriority()) return new Map();
    const unread = this.attention?.unread() ?? new Set<string>();
    return new Map(
      this.all()
        .terminals.filter((t) => agentKind(t) !== undefined)
        .map((t) => [t.terminal_id, agentSection(t.agent_status, unread.has(t.terminal_id))]),
    );
  });
  private readonly agentTick = signal(0);
  private readonly agentSettle = new SortSettle<AgentSection>(() => Date.now());
  private agentTimer: ReturnType<typeof setTimeout> | undefined;
  private shownAgentSections: ReadonlyMap<string, AgentSection> = new Map();
  private agentMode = false;
  /**
   * The section of each agent the list is ordered by: the live one, held back like the space
   * ranks while statuses still change (#712, #722).
   */
  readonly agentSections = computed<ReadonlyMap<string, AgentSection>>(() => {
    const live = this.liveAgentSections();
    const mode = this.agentsPriority();
    this.agentTick();
    clearTimeout(this.agentTimer);
    if (this.sortHeld()) return this.shownAgentSections;
    const result = this.agentSettle.update(live, mode !== this.agentMode);
    this.agentMode = mode;
    this.shownAgentSections = result.keys;
    if (result.wait > 0) {
      this.agentTimer = setTimeout(() => this.agentTick.update((n) => n + 1), result.wait);
    }
    return result.keys;
  });
  private readonly timeTick = signal(0);
  private readonly timeSettle = new SortSettle<number>(() => Date.now());
  private timeTimer: ReturnType<typeof setTimeout> | undefined;
  private shownTimes: ReadonlyMap<string, number> = new Map();
  private timeMode = false;
  /**
   * When each agent's status last changed, as the order inside a section uses it: held back like
   * the sections (#712, #722), so a change that keeps the section does not reorder at once, and a
   * held row keeps its place.
   */
  readonly agentChangedAt = computed<ReadonlyMap<string, number>>(() => {
    const mode = this.agentsPriority();
    const all = this.attention?.changedAt() ?? new Map<string, number>();
    const live = new Map([...this.liveAgentSections().keys()].map((id) => [id, all.get(id) ?? 0]));
    this.timeTick();
    clearTimeout(this.timeTimer);
    if (this.sortHeld()) return this.shownTimes;
    const result = this.timeSettle.update(live, mode !== this.timeMode);
    this.timeMode = mode;
    this.shownTimes = result.keys;
    if (result.wait > 0) {
      this.timeTimer = setTimeout(() => this.timeTick.update((n) => n + 1), result.wait);
    }
    return result.keys;
  });
  /** Terminal ids of the agents in the shown priority order, remote agents after the local ones. */
  private readonly agentOrder = computed<string[]>(() => {
    const sections = this.agentSections();
    const changedAt = this.agentChangedAt();
    const now = Date.now();
    const local = this.spaces().flatMap((s) => s.rows.filter((r) => r.agent));
    const localIds = new Set(local.map((r) => r.pane.terminal_id));
    const remote = this.all().terminals.filter(
      (t) => agentKind(t) !== undefined && !localIds.has(t.terminal_id),
    );
    return [
      ...orderAgents(
        local,
        (r) => r.pane.terminal_id,
        (r) => r.name,
        sections,
        changedAt,
        now,
      ),
      ...orderAgents(
        remote,
        (t) => t.terminal_id,
        (t) => t.label ?? t.terminal_id,
        sections,
        changedAt,
        now,
      ),
    ].map((x) => ("pane" in x ? x.pane.terminal_id : x.terminal_id));
  });

  /** True while the pointer is pressed in the sidebar: the order does not change meanwhile. */
  private readonly sortHeld = signal(false);
  private readonly sortKeys = computed<SortKeys>(() => ({
    attention: this.settledRanks(),
    recent: this.ui?.recentSpaces(),
  }));

  /** Holds the order while the pointer is pressed on a row; releasing applies what waited. */
  holdSort(held: boolean): void {
    this.sortHeld.set(held);
  }

  /** The spaces in the user's order; every list, shortcut and neighbour lookup reads this. */
  readonly orderedSpaces = computed(() =>
    orderSpaces(
      this._snapshot().spaces,
      this.spaceOrder(),
      this.sidebarSort(),
      this.pinnedSpaces(),
      this.sortKeys(),
    ),
  );
  /** The user's tab order per space (ids); tabs outside it follow by number. */
  readonly tabOrder = computed<Readonly<Record<string, readonly string[]>>>(
    () => this.ui?.tabOrder() ?? {},
  );
  readonly tree = computed(() =>
    buildTree(
      this.snapshot(),
      this.spaceOrder(),
      this.tabOrder(),
      this.sidebarSort(),
      this.pinnedSpaces(),
      this.sortKeys(),
    ),
  );
  readonly spaces = computed(() =>
    buildSpaces(
      this.snapshot(),
      this._repos(),
      this._pulls(),
      this.spaceOrder(),
      this.tabOrder(),
      this.sidebarSort(),
      this.settings?.settings().sidebarGroupWorktrees ?? true,
      this.pinnedSpaces(),
      this.sortKeys(),
    ),
  );
  /** The spaces of one machine as a tree (`machine` set on every space), last known when offline. */
  remoteTree(machine: string) {
    return this.remoteTrees().get(machine) ?? [];
  }
  /** The spaces of one machine as sidebar rows (cards, #938): `remoteTree` with pane rows. */
  remoteSpaces(machine: string): SpaceNode[] {
    return this.remoteSpaceMap().get(machine) ?? [];
  }
  private readonly remoteSpaceMap = computed(() => {
    const snapshot = this.remote?.snapshot();
    const spaces = new Map<string, SpaceNode[]>();
    if (!snapshot) return spaces;
    const sort = this.sidebarSort() === "recent" ? "name" : this.sidebarSort();
    const nodes = buildSpaces(
      snapshot,
      new Map(),
      new Map(),
      [],
      {},
      sort,
      false,
      [],
      this.sortKeys(),
    );
    for (const node of nodes) {
      const name = node.workspace.machine!;
      spaces.set(name, [...(spaces.get(name) ?? []), node]);
    }
    return spaces;
  });
  private readonly remoteTrees = computed(() => {
    const snapshot = this.remote?.snapshot();
    if (!snapshot) return new Map<string, ReturnType<typeof buildTree>>();
    const trees = new Map<string, ReturnType<typeof buildTree>>();
    // Remote spaces have no manual order: server order, or by name in `name` mode. `attention`
    // applies, `recent` falls back to the name (focus times are of this Mac's spaces).
    const sort = this.sidebarSort() === "recent" ? "name" : this.sidebarSort();
    for (const node of buildTree(snapshot, [], {}, sort, [], this.sortKeys())) {
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
      clearTimeout(this.settleTimer);
      clearTimeout(this.agentTimer);
      clearTimeout(this.timeTimer);
      void this.unlisten?.then((stop) => stop());
    });
  }

  /** Subscribes to backend events and loads the first snapshot. Calling it again does nothing. */
  async start(): Promise<void> {
    if (this.unlisten) return;
    this.unlisten = this.bridge.listen<BackendEvent>("backend", (event) => this.handle(event));
    await this.unlisten;
    void this.readAppVersion();
    void this.readAppChannel();
    const started = this.ensureBackend(false);
    await this.refresh();
    await started;
  }

  /**
   * Starts the server when nothing answers (the backend checks the socket first), then re-reads the snapshot (at start only after a start). Runs at start and on Retry; one attempt, no loop.
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
    // The shown order of the Agents section applies only to the split layout in priority mode.
    const shown =
      this.settings?.settings().sidebarLayout === "split" && this.agentsPriority()
        ? this.agentOrder()
        : undefined;
    return nextWaiting(this.all().terminals, activeId, shown);
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
      if (snapshot.spaces.length) {
        const ids = snapshot.spaces.map((s) => s.space_id);
        this.ui?.pruneSpaceOrder(ids);
        this.ui?.prunePinnedSpaces(ids);
        const remote = this.remote?.snapshot()?.spaces.map((s) => s.space_id) ?? [];
        this.ui?.pruneCollapsedCards(keptCardIds([...ids, ...remote], this.ui.collapsedCards()));
        this.ui?.pruneRecentSpaces(ids);
      }
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

  /** Asks the server for its version; a failure leaves it unknown and raises no error. */
  private async readInfo(): Promise<void> {
    try {
      this._info.set(
        (await this.bridge.invoke<BackendInfo | null | undefined>("backend_info")) ?? null,
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
              {
                progress: event.data!.agent_progress,
                kind: event.data!.agent_kind,
                title: event.data!.agent_title,
              },
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
