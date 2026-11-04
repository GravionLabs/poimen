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
export const PANEL_TABS = ["output", "flow"] as const;
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
  /** Plugin name per plugin pane id poimen opened (UI bookkeeping; pruned against live panes). */
  pluginPanes?: Record<string, string>;
  /** The bottom panel below the editor area: shown, its height and its selected tab. */
  panelOpen: boolean;
  panelHeight: number;
  panelTab: string;
  /** Keys of the last command palette entries run, newest first. */
  recentCommands?: string[];
  /** Per repository (`owner/name`), the numbers of the collapsed nodes of the GitHub issue tree. */
  collapsedIssues?: Record<string, number[]>;
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
      collapsedIssues,
    } = data as Record<string, unknown>;
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
  /** Plugin name per plugin pane id poimen opened; `PluginPanes` prunes it against live panes. */
  readonly pluginPanes = signal<Readonly<Record<string, string>>>(this.initial.pluginPanes ?? {});
  /** The bottom panel (Output) below the editor area. */
  readonly panelOpen = signal(this.initial.panelOpen);
  readonly panelHeight = signal(this.initial.panelHeight);
  readonly panelTab = signal(this.initial.panelTab);
  /** Keys of the last command palette entries run, newest first (at most 5). */
  readonly recentCommands = signal<readonly string[]>(this.initial.recentCommands ?? []);
  /** Collapsed nodes of the GitHub issue tree per repository (expanded is the default). */
  readonly collapsedIssues = signal<Readonly<Record<string, readonly number[]>>>(
    this.initial.collapsedIssues ?? {},
  );
  /** Terminal font size offset in points from the View > Zoom items; not persisted. */
  readonly zoom = signal(0);

  /** The stored editor area, read once at start; `EditorTabs` validates it. */
  readonly storedEditor: unknown = this.initial.editor;
  private editor: unknown = this.initial.editor;

  setSidebarWidth(width: number): void {
    this.sidebarWidth.set(clampSidebarWidth(width));
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

  /** Stores which plugin pane ids belong to which plugin. */
  setPluginPanes(names: Readonly<Record<string, string>>): void {
    this.pluginPanes.set(names);
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
      collapsedIssues: Object.keys(this.collapsedIssues()).length
        ? Object.fromEntries(Object.entries(this.collapsedIssues()).map(([r, n]) => [r, [...n]]))
        : undefined,
      pluginPanes: Object.keys(this.pluginPanes()).length ? { ...this.pluginPanes() } : undefined,
    };
    try {
      this.storage?.setItem(UI_STATE_KEY, JSON.stringify(state));
    } catch {
      // storage full or blocked: the layout just does not persist
    }
  }
}
