import { Injectable, InjectionToken, inject, signal } from "@angular/core";

export const SIDEBAR_MIN_WIDTH = 180;
export const SIDEBAR_MAX_WIDTH = 480;
export const SIDEBAR_DEFAULT_WIDTH = 260;
export const ZOOM_LIMIT = 20;
export const DEFAULT_VIEW = "spaces";
export const UI_STATE_KEY = "poimen.ui-state";
/** Smallest share of a section of the split sidebar layout, so its title stays visible. */
export const SPLIT_MIN_SHARE = 0.15;
export const PANEL_MIN_HEIGHT = 80;
export const PANEL_MAX_HEIGHT = 900;
export const PANEL_DEFAULT_HEIGHT = 260;
/** Tabs of the bottom panel: subagent transcripts (Output) and the run as a graph (Flow). */
export const PANEL_TABS = ["output", "flow", "events", "log"] as const;
export const DEFAULT_PANEL_TAB = "output";
/** How many run command palette entries are remembered as "Recent". */
export const RECENT_COMMANDS_LIMIT = 5;
export const SPLIT_DEFAULT_SIZES: readonly [number, number] = [0.5, 0.5];

/** Minimal slice of `Storage`, so tests can inject a fake. */
export interface UiStorage {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
}

/** Where UI state lives: the webview's local storage (poimen's own, not the user settings file). */
export const UI_STORAGE = new InjectionToken<UiStorage | null>("UI_STORAGE", {
  providedIn: "root",
  factory: () => {
    try {
      return typeof localStorage === "undefined" ? null : localStorage;
    } catch {
      return null;
    }
  },
});

/** One review comment on a line range of a diff (Changes tab); `quote` is the commented text. */
export interface LineComment {
  id: string;
  path: string;
  /** `old` is the left side (removed lines), `new` the right side or the inline view. */
  side: "old" | "new";
  start: number;
  end: number;
  quote: string;
  text: string;
}

/** The comments of one worktree: not yet sent, and already sent. */
export interface CommentSet {
  pending: LineComment[];
  sent: LineComment[];
}

function parseComment(value: unknown): LineComment | null {
  if (typeof value !== "object" || value === null) return null;
  const c = value as Record<string, unknown>;
  if (
    typeof c["id"] !== "string" ||
    typeof c["path"] !== "string" ||
    (c["side"] !== "old" && c["side"] !== "new") ||
    !Number.isInteger(c["start"]) ||
    !Number.isInteger(c["end"]) ||
    (c["start"] as number) < 0 ||
    (c["end"] as number) < (c["start"] as number) ||
    typeof c["quote"] !== "string" ||
    typeof c["text"] !== "string"
  )
    return null;
  return {
    id: c["id"],
    path: c["path"],
    side: c["side"],
    start: c["start"] as number,
    end: c["end"] as number,
    quote: c["quote"],
    text: c["text"],
  };
}

/** Reads stored comments per worktree; malformed entries are dropped, empty worktrees too. */
export function parseLineComments(value: unknown): Record<string, CommentSet> {
  const out: Record<string, CommentSet> = {};
  if (typeof value !== "object" || value === null || Array.isArray(value)) return out;
  const list = (v: unknown): LineComment[] =>
    Array.isArray(v) ? v.map(parseComment).filter((c): c is LineComment => c !== null) : [];
  for (const [dir, set] of Object.entries(value)) {
    const s = (set ?? {}) as Record<string, unknown>;
    const entry = { pending: list(s["pending"]), sent: list(s["sent"]) };
    if (entry.pending.length || entry.sent.length) out[dir] = entry;
  }
  return out;
}

export type GithubPage = "inbox" | "pulls" | "issues" | "milestones";

export function isGithubPage(value: unknown): value is GithubPage {
  return value === "inbox" || value === "pulls" || value === "issues" || value === "milestones";
}

export interface LayoutState {
  sidebarWidth: number;
  sidebarCollapsed: boolean;
  /** The editor area (tabs, groups); validated by `EditorTabs`, absent when never stored. */
  editor?: unknown;
  /** The view shown in the sidebar (an id of `ACTIVITY_VIEWS`). */
  activeView: string;
  /** Paths of the explorer folders that are expanded. */
  expandedDirs: string[];
  /** The workspace selected in the split sidebar layout; absent until one is picked. */
  splitWorkspace?: string;
  /** Shares of the Spaces and Agents sections in the split sidebar layout; they sum to 1. */
  splitSizes?: [number, number];
  /** Legacy: plugin panes poimen opened; read once so their tabs can be dropped, never written. */
  pluginPanes?: Record<string, string>;
  /** The bottom panel below the editor area: shown, its height and its selected tab. */
  panelOpen: boolean;
  panelHeight: number;
  panelTab: string;
  /** Keys of the last command palette entries run, newest first. */
  recentCommands?: string[];
  /** The user's order of the spaces (`space_id`s); spaces not listed follow by number. */
  spaceOrder?: string[];
  /** The user's order of the tabs (`tab_id`s) per `space_id`; tabs not listed follow by number. */
  tabOrder?: Record<string, string[]>;
  /** How the Changes tab shows a diff: `inline` (unified) or `split` (side by side). */
  changesLayout?: string;
  /** Review comments of the Changes tab per worktree, kept until sent to the agent. */
  lineComments?: Record<string, CommentSet>;
  /** Per repository (`owner/name`), the numbers of the collapsed nodes of the GitHub issue tree. */
  collapsedIssues?: Record<string, number[]>;
  /** The selected sub-page of the GitHub tab (`pulls`, `issues` or `milestones`). */
  githubPage?: string;
  /** The GitHub tab also lists repositories without matches. */
  githubShowEmpty?: boolean;
  /** The page selected in the settings view (an id of its category list); may no longer exist. */
  settingsPage?: string;
  /** Ids of the collapsed groups of the settings category tree. */
  settingsCollapsed?: string[];
  /** The user answered "Not now" to the offer to install the agent hooks of the poimen backend. */
  hooksOfferDismissed?: boolean;
  /** Agents whose hooks were seen installed; a dismissed offer returns when one goes missing. */
  hooksSeen?: string[];
  /** The first-start defaults (`spaces.layout`, default space template) were written once. */
  defaultsSeeded?: boolean;
}

export function clampPanelHeight(height: number): number {
  if (!Number.isFinite(height)) return PANEL_DEFAULT_HEIGHT;
  return Math.min(PANEL_MAX_HEIGHT, Math.max(PANEL_MIN_HEIGHT, Math.round(height)));
}

/** Two shares that sum to 1, each at least {@link SPLIT_MIN_SHARE}; anything else is `null`. */
export function clampSplitSizes(sizes: unknown): [number, number] | null {
  if (!Array.isArray(sizes) || sizes.length !== 2) return null;
  const [a, b] = sizes as unknown[];
  if (typeof a !== "number" || typeof b !== "number") return null;
  if (!Number.isFinite(a) || !Number.isFinite(b) || a < 0 || b < 0 || a + b <= 0) return null;
  const share = Math.min(1 - SPLIT_MIN_SHARE, Math.max(SPLIT_MIN_SHARE, a / (a + b)));
  // Rounded to 0.1 % so the stored state stays readable (no 0.15000000000000002).
  const first = Math.round(share * 1000) / 1000;
  return [first, Math.round((1 - first) * 1000) / 1000];
}

export function clampSidebarWidth(width: number): number {
  if (!Number.isFinite(width)) return SIDEBAR_DEFAULT_WIDTH;
  return Math.min(SIDEBAR_MAX_WIDTH, Math.max(SIDEBAR_MIN_WIDTH, Math.round(width)));
}

/** Reads stored layout JSON; anything missing or malformed falls back to the defaults. */
export function parseLayout(raw: string | null): LayoutState {
  const state: LayoutState = {
    sidebarWidth: SIDEBAR_DEFAULT_WIDTH,
    sidebarCollapsed: false,
    activeView: DEFAULT_VIEW,
    expandedDirs: [],
    panelOpen: false,
    panelHeight: PANEL_DEFAULT_HEIGHT,
    panelTab: DEFAULT_PANEL_TAB,
  };
  if (!raw) return state;
  try {
    const data: unknown = JSON.parse(raw);
    if (typeof data !== "object" || data === null) return state;
    const {
      sidebarWidth,
      sidebarCollapsed,
      editor,
      activeView,
      expandedDirs,
      splitWorkspace,
      splitSizes,
      pluginPanes,
      panelOpen,
      panelHeight,
      panelTab,
      recentCommands,
      spaceOrder,
      tabOrder,
      changesLayout,
      lineComments,
      collapsedIssues,
      githubPage,
      githubShowEmpty,
      settingsPage,
      settingsCollapsed,
      hooksOfferDismissed,
      hooksSeen,
      defaultsSeeded,
    } = data as Record<string, unknown>;
    const comments = parseLineComments(lineComments);
    if (Object.keys(comments).length) state.lineComments = comments;
    if (changesLayout === "inline" || changesLayout === "split")
      state.changesLayout = changesLayout;
    if (
      typeof collapsedIssues === "object" &&
      collapsedIssues !== null &&
      !Array.isArray(collapsedIssues)
    ) {
      const entries = Object.entries(collapsedIssues)
        .map(([repo, nrs]): [string, number[]] => [
          repo,
          Array.isArray(nrs) ? nrs.filter((n): n is number => Number.isInteger(n)) : [],
        ])
        .filter(([, nrs]) => nrs.length > 0);
      if (entries.length) state.collapsedIssues = Object.fromEntries(entries);
    }
    if (typeof settingsPage === "string" && settingsPage) state.settingsPage = settingsPage;
    if (isGithubPage(githubPage)) state.githubPage = githubPage;
    if (githubShowEmpty === true) state.githubShowEmpty = true;
    if (hooksOfferDismissed === true) state.hooksOfferDismissed = true;
    if (defaultsSeeded === true) state.defaultsSeeded = true;
    if (Array.isArray(hooksSeen)) {
      const seen = hooksSeen.filter((d): d is string => typeof d === "string" && d !== "");
      if (seen.length) state.hooksSeen = [...new Set(seen)];
    }
    if (Array.isArray(settingsCollapsed)) {
      const ids = settingsCollapsed.filter((d): d is string => typeof d === "string" && d !== "");
      if (ids.length) state.settingsCollapsed = [...new Set(ids)];
    }
    if (typeof sidebarWidth === "number") state.sidebarWidth = clampSidebarWidth(sidebarWidth);
    if (typeof sidebarCollapsed === "boolean") state.sidebarCollapsed = sidebarCollapsed;
    if (typeof activeView === "string" && activeView) state.activeView = activeView;
    if (Array.isArray(expandedDirs)) {
      state.expandedDirs = expandedDirs.filter((d): d is string => typeof d === "string");
    }
    if (typeof splitWorkspace === "string" && splitWorkspace) {
      state.splitWorkspace = splitWorkspace;
    }
    const sizes = clampSplitSizes(splitSizes);
    if (sizes) state.splitSizes = sizes;
    if (typeof pluginPanes === "object" && pluginPanes !== null && !Array.isArray(pluginPanes)) {
      state.pluginPanes = Object.fromEntries(
        Object.entries(pluginPanes).filter(
          (e): e is [string, string] => typeof e[1] === "string" && e[1] !== "",
        ),
      );
    }
    if (typeof panelOpen === "boolean") state.panelOpen = panelOpen;
    if (typeof panelHeight === "number") state.panelHeight = clampPanelHeight(panelHeight);
    if (typeof panelTab === "string" && (PANEL_TABS as readonly string[]).includes(panelTab)) {
      state.panelTab = panelTab;
    }
    if (Array.isArray(recentCommands)) {
      const keys = recentCommands.filter((k): k is string => typeof k === "string" && k !== "");
      state.recentCommands = [...new Set(keys)].slice(0, RECENT_COMMANDS_LIMIT);
    }
    if (Array.isArray(spaceOrder)) {
      const ids = spaceOrder.filter((k): k is string => typeof k === "string" && k !== "");
      if (ids.length) state.spaceOrder = [...new Set(ids)];
    }
    if (tabOrder && typeof tabOrder === "object" && !Array.isArray(tabOrder)) {
      const clean: Record<string, string[]> = {};
      for (const [space, list] of Object.entries(tabOrder)) {
        if (!space || !Array.isArray(list)) continue;
        const tabs = list.filter((k): k is string => typeof k === "string" && k !== "");
        if (tabs.length) clean[space] = [...new Set(tabs)];
      }
      if (Object.keys(clean).length) state.tabOrder = clean;
    }
    if (editor !== undefined) state.editor = editor;
  } catch {
    // corrupt state: use defaults
  }
  return state;
}

/** Window layout that survives restarts. UI state only; pane state stays with herdr. */
@Injectable({ providedIn: "root" })
export class UiState {
  private readonly storage = inject(UI_STORAGE);
  private readonly initial = parseLayout(this.read());

  readonly sidebarWidth = signal(this.initial.sidebarWidth);
  readonly sidebarCollapsed = signal(this.initial.sidebarCollapsed);
  readonly activeView = signal(this.initial.activeView);
  readonly expandedDirs = signal<ReadonlySet<string>>(new Set(this.initial.expandedDirs));
  /** The workspace selected in the split sidebar layout (UI state, not a setting). */
  readonly splitWorkspace = signal<string | null>(this.initial.splitWorkspace ?? null);
  /** Shares of the Spaces and Agents sections of the split sidebar layout. */
  readonly splitSizes = signal<readonly [number, number]>(
    this.initial.splitSizes ?? SPLIT_DEFAULT_SIZES,
  );
  /** Ids of the panes the removed plugin integration opened as tabs; `EditorTabs` drops them on restore. */
  readonly legacyPluginPaneIds: ReadonlySet<string> = new Set(
    Object.keys(this.initial.pluginPanes ?? {}),
  );
  /** The bottom panel (Output) below the editor area. */
  readonly panelOpen = signal(this.initial.panelOpen);
  readonly panelHeight = signal(this.initial.panelHeight);
  readonly panelTab = signal(this.initial.panelTab);
  /** Keys of the last command palette entries run, newest first (at most 5). */
  /** The user's space order (`space_id`s, persisted). */
  readonly spaceOrder = signal<readonly string[]>(this.initial.spaceOrder ?? []);
  /** The user's tab order per space (`tab_id`s, persisted). */
  readonly tabOrder = signal<Readonly<Record<string, readonly string[]>>>(
    this.initial.tabOrder ?? {},
  );
  readonly recentCommands = signal<readonly string[]>(this.initial.recentCommands ?? []);
  /** Collapsed nodes of the GitHub issue tree per repository (expanded is the default). */
  readonly collapsedIssues = signal<Readonly<Record<string, readonly number[]>>>(
    this.initial.collapsedIssues ?? {},
  );
  /** The selected sub-page of the GitHub tab. */
  readonly githubPage = signal<GithubPage>(
    isGithubPage(this.initial.githubPage) ? this.initial.githubPage : "inbox",
  );
  /** Whether the GitHub tab lists repositories without matches (hidden by default). */
  readonly githubShowEmpty = signal(this.initial.githubShowEmpty === true);
  /** How the Changes tab shows a diff (side by side by default). */
  readonly changesLayout = signal<"inline" | "split">(
    this.initial.changesLayout === "inline" ? "inline" : "split",
  );
  /** Review comments of the Changes tab per worktree (pending until the agent got them). */
  readonly lineComments = signal<Readonly<Record<string, CommentSet>>>(
    this.initial.lineComments ?? {},
  );
  /** The selected page of the settings view; `null` until one is picked. */
  readonly settingsPage = signal<string | null>(this.initial.settingsPage ?? null);
  /** Collapsed groups of the settings category tree. */
  readonly settingsCollapsed = signal<ReadonlySet<string>>(
    new Set(this.initial.settingsCollapsed ?? []),
  );
  /** Whether the hooks offer was declined; it is not shown again. */
  readonly hooksOfferDismissed = signal(this.initial.hooksOfferDismissed === true);
  /** Agents whose hooks were seen installed (persisted). */
  readonly hooksSeen = signal<readonly string[]>(this.initial.hooksSeen ?? []);

  /** The first-start defaults were written; they are not written again (even when deleted later). */
  readonly defaultsSeeded = signal(this.initial.defaultsSeeded === true);
  /** Terminal font size offset in points from the View > Zoom items; not persisted. */
  readonly zoom = signal(0);

  /** The stored editor area, read once at start; `EditorTabs` validates it. */
  readonly storedEditor: unknown = this.initial.editor;
  private editor: unknown = this.initial.editor;

  setSidebarWidth(width: number): void {
    this.sidebarWidth.set(clampSidebarWidth(width));
    this.save();
  }

  setChangesLayout(layout: "inline" | "split"): void {
    this.changesLayout.set(layout);
    this.save();
  }

  /** Stores the comments of a worktree (an empty set removes the entry). */
  setLineComments(dir: string, set: CommentSet): void {
    const next = { ...this.lineComments() };
    if (set.pending.length || set.sent.length) next[dir] = set;
    else delete next[dir];
    this.lineComments.set(next);
    this.save();
  }

  selectGithubPage(page: GithubPage): void {
    this.githubPage.set(page);
    this.save();
  }

  setGithubShowEmpty(show: boolean): void {
    this.githubShowEmpty.set(show);
    this.save();
  }

  selectSettingsPage(id: string): void {
    this.settingsPage.set(id);
    this.save();
  }

  setSettingsGroupCollapsed(id: string, collapsed: boolean): void {
    const next = new Set(this.settingsCollapsed());
    if (collapsed) next.add(id);
    else next.delete(id);
    this.settingsCollapsed.set(next);
    this.save();
  }

  dismissHooksOffer(): void {
    this.hooksOfferDismissed.set(true);
    this.save();
  }

  markDefaultsSeeded(): void {
    this.defaultsSeeded.set(true);
    this.save();
  }

  /** Records which agents have hooks installed (`installed`) and which do not (`gone`). */
  setHooksSeen(installed: readonly string[], gone: readonly string[]): void {
    const next = [...new Set([...this.hooksSeen().filter((a) => !gone.includes(a)), ...installed])];
    const cur = this.hooksSeen();
    if (next.length === cur.length && next.every((a) => cur.includes(a))) return;
    this.hooksSeen.set(next);
    this.save();
  }

  togglePanel(): void {
    this.panelOpen.update((open) => !open);
    this.save();
  }

  setPanelHeight(height: number): void {
    this.panelHeight.set(clampPanelHeight(height));
    this.save();
  }

  /** Selects a tab of the bottom panel and shows the panel; an unknown tab is ignored. */
  selectPanelTab(tab: string): void {
    if (!(PANEL_TABS as readonly string[]).includes(tab)) return;
    this.panelTab.set(tab);
    this.panelOpen.set(true);
    this.save();
  }

  /** Remembers whether the node `number` of the issue tree of `repo` is collapsed. */
  setIssueCollapsed(repo: string, number: number, collapsed: boolean): void {
    const rest = (this.collapsedIssues()[repo] ?? []).filter((n) => n !== number);
    const list = collapsed ? [...rest, number] : rest;
    const next = { ...this.collapsedIssues(), [repo]: list };
    if (!list.length) delete next[repo];
    this.collapsedIssues.set(next);
    this.save();
  }

  /** Stores the order of the spaces. */
  setSpaceOrder(ids: readonly string[]): void {
    this.spaceOrder.set([...new Set(ids)]);
    this.save();
  }

  /** Stores the tab order of one space. */
  setTabOrder(spaceId: string, ids: readonly string[]): void {
    this.tabOrder.set({ ...this.tabOrder(), [spaceId]: [...new Set(ids)] });
    this.save();
  }

  /**
   * Drops closed tabs and closed spaces from the tab orders (`existing`: `tab_id`s per `space_id`);
   * saves only when something went.
   */
  pruneTabOrder(existing: ReadonlyMap<string, readonly string[]>): void {
    const next: Record<string, readonly string[]> = {};
    let changed = false;
    for (const [space, ids] of Object.entries(this.tabOrder())) {
      const alive = existing.get(space);
      const keep = alive ? ids.filter((id) => alive.includes(id)) : [];
      if (keep.length !== ids.length) changed = true;
      if (keep.length) next[space] = keep;
    }
    if (!changed) return;
    this.tabOrder.set(next);
    this.save();
  }

  /** Drops the ids of spaces that no longer exist; saves only when something went. */
  pruneSpaceOrder(existing: readonly string[]): void {
    const keep = this.spaceOrder().filter((id) => existing.includes(id));
    if (keep.length === this.spaceOrder().length) return;
    this.spaceOrder.set(keep);
    this.save();
  }

  /** Remembers a run command palette entry as the newest "Recent" one. */
  addRecentCommand(key: string): void {
    if (!key) return;
    this.recentCommands.update((keys) =>
      [key, ...keys.filter((k) => k !== key)].slice(0, RECENT_COMMANDS_LIMIT),
    );
    this.save();
  }

  setSplitWorkspace(id: string): void {
    this.splitWorkspace.set(id);
    this.save();
  }

  /** Resizes the sections of the split sidebar layout; malformed sizes are ignored. */
  setSplitSizes(sizes: readonly number[]): void {
    const next = clampSplitSizes(sizes);
    if (!next) return;
    this.splitSizes.set(next);
    this.save();
  }

  /** Persists the editor area (open tabs, groups, split sizes, focus). */
  saveEditor(editor: unknown): void {
    this.editor = editor;
    this.save();
  }

  /** Zooms the terminals by `delta` points, or back to the configured size when `delta` is null. */
  zoomBy(delta: number | null): void {
    this.zoom.update((z) =>
      delta === null ? 0 : Math.max(-ZOOM_LIMIT, Math.min(ZOOM_LIMIT, z + delta)),
    );
  }

  toggleSidebar(): void {
    this.sidebarCollapsed.update((collapsed) => !collapsed);
    this.save();
  }

  /**
   * A click on a view of the activity bar: the active view collapses the sidebar, any other view
   * (also while collapsed) shows it. `known` are the valid view ids; an unknown id is ignored.
   */
  selectView(id: string, known: readonly string[]): void {
    if (!known.includes(id)) return;
    if (id === this.activeView() && !this.sidebarCollapsed()) {
      this.sidebarCollapsed.set(true);
    } else {
      this.activeView.set(id);
      this.sidebarCollapsed.set(false);
    }
    this.save();
  }

  /** Remembers whether the explorer folder at `path` is expanded. */
  setDirExpanded(path: string, expanded: boolean): void {
    const next = new Set(this.expandedDirs());
    if (expanded) next.add(path);
    else next.delete(path);
    this.expandedDirs.set(next);
    this.save();
  }

  private read(): string | null {
    try {
      return this.storage?.getItem(UI_STATE_KEY) ?? null;
    } catch {
      return null;
    }
  }

  private save(): void {
    const state: LayoutState = {
      sidebarWidth: this.sidebarWidth(),
      sidebarCollapsed: this.sidebarCollapsed(),
      editor: this.editor,
      activeView: this.activeView(),
      expandedDirs: [...this.expandedDirs()],
      splitWorkspace: this.splitWorkspace() ?? undefined,
      // Like splitWorkspace: stored only once it differs from the default.
      splitSizes:
        this.splitSizes()[0] === SPLIT_DEFAULT_SIZES[0] ? undefined : [...this.splitSizes()],
      panelOpen: this.panelOpen(),
      panelHeight: this.panelHeight(),
      panelTab: this.panelTab(),
      recentCommands: this.recentCommands().length ? [...this.recentCommands()] : undefined,
      spaceOrder: this.spaceOrder().length ? [...this.spaceOrder()] : undefined,
      tabOrder: Object.keys(this.tabOrder()).length
        ? Object.fromEntries(Object.entries(this.tabOrder()).map(([s, t]) => [s, [...t]]))
        : undefined,
      collapsedIssues: Object.keys(this.collapsedIssues()).length
        ? Object.fromEntries(Object.entries(this.collapsedIssues()).map(([r, n]) => [r, [...n]]))
        : undefined,
      githubPage: this.githubPage() === "inbox" ? undefined : this.githubPage(),
      githubShowEmpty: this.githubShowEmpty() ? true : undefined,
      changesLayout: this.changesLayout() === "inline" ? "inline" : undefined,
      lineComments: Object.keys(this.lineComments()).length ? this.lineComments() : undefined,
      settingsPage: this.settingsPage() ?? undefined,
      settingsCollapsed: this.settingsCollapsed().size ? [...this.settingsCollapsed()] : undefined,
      hooksOfferDismissed: this.hooksOfferDismissed() ? true : undefined,
      hooksSeen: this.hooksSeen().length ? [...this.hooksSeen()] : undefined,
      defaultsSeeded: this.defaultsSeeded() ? true : undefined,
    };
    try {
      this.storage?.setItem(UI_STATE_KEY, JSON.stringify(state));
    } catch {
      // storage full or blocked: the layout just does not persist
    }
  }
}
