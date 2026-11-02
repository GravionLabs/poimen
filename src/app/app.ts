import {
  ChangeDetectionStrategy,
  Component,
  DestroyRef,
  HostListener,
  computed,
  effect,
  signal,
  inject,
} from "@angular/core";
import { NgTemplateOutlet } from "@angular/common";
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
import { EditorTabs, splitKeyDirection, type EditorGroup } from "./core/editor-tabs";
import type { LayoutNode } from "./core/editor-layout";
import { Toolbar } from "./toolbar/toolbar";
import { StatusBar } from "./status-bar/status-bar";
import { SplitPane } from "./ui/helix-candidates/split-pane/split-pane";
import {
  TabBar,
  type TabBarItem,
  type TabBarMenuItem,
} from "./ui/helix-candidates/tab-bar/tab-bar";

/** Ids the native menu emits on the `menu` event (see `src-tauri/src/lib.rs`). */
type MenuAction = "close_all" | "refresh";

/** The window: the activity bar, the sidebar and the editor group (tab bar over the live terminal). */
@Component({
  selector: "app-root",
  imports: [
    ActivityBar,
    Sidebar,
    TerminalView,
    TabBar,
    SplitPane,
    StatusBar,
    Toolbar,
    NgTemplateOutlet,
  ],
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
  protected readonly groupItems = computed(() => {
    const snap = this.store.snapshot();
    this.store.panes();
    return new Map(
      this.editor.groups().map((group) => [
        group.id,
        group.tabs.map((tab): TabBarItem => {
          const pane = this.pane(tab.paneId);
          const tabLabel = pane
            ? snap.tabs.find((t) => t.tab_id === pane.tab_id)?.label
            : undefined;
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
      ]),
    );
  });
  /** Size of each terminal that reported one; the status bar shows the focused group's. */
  private readonly sizes = signal<ReadonlyMap<string, { cols: number; rows: number }>>(new Map());
  protected readonly termSize = computed(() => {
    const id = this.editor.activeId();
    return id ? this.sizes().get(id) : undefined;
  });

  constructor() {
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

  protected readonly rootNode = computed<LayoutNode>(() => this.editor.layout());

  protected readonly theme$ = computed(() => this.prefs.settings().theme);

  protected setTheme(theme: ThemeSetting): void {
    void this.prefs.setTheme(theme);
  }

  /** `Cmd+B` / `Ctrl+B` toggles the sidebar. */
  @HostListener("window:keydown", ["$event"])
  protected onKey(event: KeyboardEvent): void {
    const dir = splitKeyDirection(event);
    if (dir) {
      const id = this.editor.activeId();
      if (!id) return;
      event.preventDefault();
      this.editor.split(id, dir);
      return;
    }
    if (!isToggleSidebarKey(event)) return;
    event.preventDefault();
    this.ui.toggleSidebar();
  }

  /** Context menu of a group's tabs; splitting needs a second tab in the group. */
  protected menuFor(group: EditorGroup): TabBarMenuItem[] {
    const single = group.tabs.length < 2;
    const mod = /Mac/i.test(navigator.platform) ? "⌘" : "Ctrl+";
    return [
      { action: "split-right", label: "Split right", shortcut: `${mod}\\`, disabled: single },
      { action: "split-down", label: "Split down", shortcut: `${mod}⇧\\`, disabled: single },
      { action: "close", label: "Close" },
    ];
  }

  protected onTabMenu(id: string, action: string): void {
    if (action === "split-right") this.editor.split(id, "row");
    else if (action === "split-down") this.editor.split(id, "column");
    else if (action === "close") this.editor.close(id);
  }

  protected groupPane(group: EditorGroup): Pane | undefined {
    return group.activeId ? this.pane(group.activeId) : undefined;
  }

  protected group(id: number): EditorGroup | undefined {
    return this.editor.groups().find((g) => g.id === id);
  }

  protected onSized(paneId: string, size: { cols: number; rows: number }): void {
    this.sizes.update((m) => new Map(m).set(paneId, size));
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
