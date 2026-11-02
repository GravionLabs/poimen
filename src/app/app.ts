import {
  ChangeDetectionStrategy,
  Component,
  DestroyRef,
  HostListener,
  computed,
  effect,
  signal,
  untracked,
  inject,
} from "@angular/core";
import { Title } from "@angular/platform-browser";
import { HxTheme } from "@gravionlabs/helix-ui";
import { rowName, tabHeader, type Mode, type Pane } from "./core/herdr";
import { HerdrStore } from "./core/herdr-store";
import { SettingsStore, type ThemeSetting } from "./core/settings";
import { TauriBridge } from "./core/tauri";
import { ActivityBar } from "./activity-bar/activity-bar";
import { SIDEBAR_MAX_WIDTH, SIDEBAR_MIN_WIDTH, UiState, isToggleSidebarKey } from "./core/ui-state";
import { Sidebar } from "./sidebar/sidebar";
import { TerminalView } from "./terminal-view/terminal-view";
import { EditorTabs } from "./core/editor-tabs";
import { Toolbar } from "./toolbar/toolbar";
import { StatusBar } from "./status-bar/status-bar";
import { TabBar, type TabBarItem } from "./ui/helix-candidates/tab-bar/tab-bar";

/** Ids the native menu emits on the `menu` event (see `src-tauri/src/lib.rs`). */
type MenuAction = "close_all" | "refresh";

/** The window: the activity bar, the sidebar and the editor group (tab bar over the live terminal). */
@Component({
  selector: "app-root",
  imports: [ActivityBar, Sidebar, TerminalView, TabBar, StatusBar, Toolbar],
  templateUrl: "./app.html",
  styleUrl: "./app.css",
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class App {
  protected readonly store = inject(HerdrStore);
  private readonly bridge = inject(TauriBridge);
  private readonly theme = inject(HxTheme);
  private readonly prefs = inject(SettingsStore);
  protected readonly ui = inject(UiState);
  protected readonly minWidth = SIDEBAR_MIN_WIDTH;
  protected readonly maxWidth = SIDEBAR_MAX_WIDTH;
  private drag: { startX: number; startWidth: number } | null = null;

  protected readonly editor = inject(EditorTabs);
  /** Last known state of each open pane, kept when herdr drops the pane. */
  private readonly known = new Map<string, Pane>();
  protected readonly tabItems = computed<TabBarItem[]>(() =>
    this.editor.tabs().map((tab) => {
      const pane = this.pane(tab.paneId);
      const snap = this.store.snapshot();
      const tabLabel = pane ? snap.tabs.find((t) => t.tab_id === pane.tab_id)?.label : undefined;
      const workspace = pane
        ? snap.workspaces.find((w) => w.workspace_id === pane.workspace_id)?.label
        : undefined;
      const header = tabHeader(workspace, pane ? rowName(pane, tabLabel) : tab.paneId);
      return {
        id: tab.paneId,
        label: header.label,
        group: header.group,
        preview: !tab.pinned,
        title: `${header.title}${tab.pinned ? "" : " (preview)"}`,
      };
    }),
  );
  /** The pane shown in the editor group; only this one keeps a stream open. */
  protected readonly activePane = computed(() => {
    const id = this.editor.activeId();
    return id ? this.pane(id) : undefined;
  });
  /** Size of the active terminal; undefined until it is measured. */
  protected readonly termSize = signal<{ cols: number; rows: number } | undefined>(undefined);

  constructor() {
    effect(() => {
      this.editor.activeId();
      untracked(() => this.termSize.set(undefined));
    });
    const title = inject(Title);
    effect(() => {
      const waiting = this.store.waiting();
      title.setTitle(waiting > 0 ? `poimen (${waiting} waiting)` : "poimen");
    });

    const destroyRef = inject(DestroyRef);
    const menu = this.bridge.listen<MenuAction>("menu", (action) => this.onMenu(action));
    destroyRef.onDestroy(() => void menu.then((stop) => stop()));
    effect((onCleanup) => {
      const setting = this.prefs.settings().theme;
      if (setting !== "system") {
        this.theme.setDark(setting === "dark");
        return;
      }
      onCleanup(this.followSystemTheme());
    });
    void this.prefs.start();
    void this.store.start();
  }

  protected readonly theme$ = computed(() => this.prefs.settings().theme);

  protected setTheme(theme: ThemeSetting): void {
    void this.prefs.setTheme(theme);
  }

  /** `Cmd+B` / `Ctrl+B` toggles the sidebar. */
  @HostListener("window:keydown", ["$event"])
  protected onKey(event: KeyboardEvent): void {
    if (!isToggleSidebarKey(event)) return;
    event.preventDefault();
    this.ui.toggleSidebar();
  }

  protected dragStart(event: PointerEvent): void {
    const handle = event.target as HTMLElement;
    handle.setPointerCapture(event.pointerId);
    handle.classList.add("dragging");
    this.drag = { startX: event.clientX, startWidth: this.ui.sidebarWidth() };
  }

  protected dragMove(event: PointerEvent): void {
    if (!this.drag) return;
    this.ui.setSidebarWidth(this.drag.startWidth + event.clientX - this.drag.startX);
  }

  protected dragEnd(event: PointerEvent): void {
    this.drag = null;
    (event.target as HTMLElement).classList.remove("dragging");
  }

  protected resizeKey(event: KeyboardEvent): void {
    const step = event.shiftKey ? 40 : 10;
    if (event.key === "ArrowLeft") this.ui.setSidebarWidth(this.ui.sidebarWidth() - step);
    else if (event.key === "ArrowRight") this.ui.setSidebarWidth(this.ui.sidebarWidth() + step);
    else return;
    event.preventDefault();
  }

  /** Single click previews, a double click pins; an open pane is only activated. */
  protected open(pane: Pane, pin = false): void {
    this.known.set(pane.pane_id, pane);
    this.editor.open(pane.pane_id, pin);
  }

  protected tabPane(id: string): Pane | undefined {
    return this.pane(id);
  }

  /** The mode a tab asks for; the terminal view falls back to read-only when it is not free. */
  protected wanted(paneId: string): Mode {
    return this.editor.wanted(paneId, this.prefs.settings().autoControl);
  }

  /** The status bar switch for the active tab. */
  protected setControl(on: boolean): void {
    const id = this.editor.activeId();
    if (id) this.editor.choose(id, on ? "control" : "observe");
  }

  /** Opens the next blocked pane after the active one. */
  protected jumpToWaiting(): void {
    const next = this.store.nextWaiting(this.editor.activeId());
    if (next) this.open(next);
  }

  private pane(id: string): Pane | undefined {
    return this.store.panes().get(id) ?? this.known.get(id);
  }

  private onMenu(action: MenuAction): void {
    if (action === "close_all") this.editor.closeAll();
    else void this.store.refresh();
  }

  /** HxTheme reads the system setting once; keep light/dark in step when it changes. */
  private followSystemTheme(): () => void {
    if (typeof matchMedia !== "function") return () => undefined;
    const query = matchMedia("(prefers-color-scheme: dark)");
    const update = () => this.theme.setDark(query.matches);
    update();
    query.addEventListener("change", update);
    return () => query.removeEventListener("change", update);
  }
}
