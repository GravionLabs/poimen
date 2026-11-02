import { Injectable, InjectionToken, inject, signal } from "@angular/core";

export const SIDEBAR_MIN_WIDTH = 180;
export const SIDEBAR_MAX_WIDTH = 480;
export const SIDEBAR_DEFAULT_WIDTH = 260;
export const ZOOM_LIMIT = 20;
export const DEFAULT_VIEW = "spaces";
export const UI_STATE_KEY = "poimen.ui-state";

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
  };
  if (!raw) return state;
  try {
    const data: unknown = JSON.parse(raw);
    if (typeof data !== "object" || data === null) return state;
    const { sidebarWidth, sidebarCollapsed, editor, activeView, expandedDirs } = data as Record<
      string,
      unknown
    >;
    if (typeof sidebarWidth === "number") state.sidebarWidth = clampSidebarWidth(sidebarWidth);
    if (typeof sidebarCollapsed === "boolean") state.sidebarCollapsed = sidebarCollapsed;
    if (typeof activeView === "string" && activeView) state.activeView = activeView;
    if (Array.isArray(expandedDirs)) {
      state.expandedDirs = expandedDirs.filter((d): d is string => typeof d === "string");
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
  /** Terminal font size offset in points from the View > Zoom items; not persisted. */
  readonly zoom = signal(0);

  /** The stored editor area, read once at start; `EditorTabs` validates it. */
  readonly storedEditor: unknown = this.initial.editor;
  private editor: unknown = this.initial.editor;

  setSidebarWidth(width: number): void {
    this.sidebarWidth.set(clampSidebarWidth(width));
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
    };
    try {
      this.storage?.setItem(UI_STATE_KEY, JSON.stringify(state));
    } catch {
      // storage full or blocked: the layout just does not persist
    }
  }
}
