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
import { ColorSchemeService } from "./core/color-scheme-service";
import { HerdrStore } from "./core/herdr-store";
import { SettingsStore, type ThemeSetting } from "./core/settings";
import { TauriBridge } from "./core/tauri";
import { ACTIVITY_VIEWS, ActivityBar } from "./activity-bar/activity-bar";
import { Explorer } from "./explorer/explorer";
import { SIDEBAR_MAX_WIDTH, SIDEBAR_MIN_WIDTH, UiState } from "./core/ui-state";
import { Sidebar } from "./sidebar/sidebar";
import { TerminalView } from "./terminal-view/terminal-view";
import { EditorTabs, type EditorGroup } from "./core/editor-tabs";
import { Keymap } from "./core/keymap";
import type { ShortcutAction } from "./core/shortcuts";
import type { LayoutNode } from "./core/editor-layout";
import { Toolbar } from "./toolbar/toolbar";
import { PaneActions } from "./core/pane-actions";
import { PaneDialog } from "./pane-dialog/pane-dialog";
import { SpaceActions } from "./core/space-actions";
import { SpaceDialog } from "./space-dialog/space-dialog";
import type { SidebarMenuAction } from "./sidebar/sidebar";
import { StatusBar } from "./status-bar/status-bar";
import { SplitPane } from "./ui/helix-candidates/split-pane/split-pane";
import {
  TabBar,
  type TabBarItem,
  type TabBarMenuItem,
} from "./ui/helix-candidates/tab-bar/tab-bar";

/** The terminal font changes by this many points per zoom step. */
const ZOOM_STEP = 1;

/** The window: the activity bar, the sidebar and the editor group (tab bar over the live terminal). */
@Component({
  selector: "app-root",
  imports: [
    ActivityBar,
    Sidebar,
    Explorer,
    TerminalView,
    TabBar,
    SplitPane,
    StatusBar,
    Toolbar,
    NgTemplateOutlet,
    PaneDialog,
    SpaceDialog,
  ],
  templateUrl: "./app.html",
  styleUrl: "./app.css",
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class App {
  protected readonly store = inject(HerdrStore);
  private readonly bridge = inject(TauriBridge);
  private readonly theme = inject(HxTheme);
  private readonly schemes = inject(ColorSchemeService);
  private readonly prefs = inject(SettingsStore);
  private readonly keymap = inject(Keymap);
  private readonly paneActions = inject(PaneActions);
  protected readonly spaceActions = inject(SpaceActions);
  protected readonly ui = inject(UiState);

  /** A click on the activity bar: switch the view; the active one collapses the sidebar. */
  protected selectView(id: string): void {
    this.ui.selectView(
      id,
      ACTIVITY_VIEWS.map((v) => v.id),
    );
  }
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
    const menu = this.bridge.listen<string>("menu", (id) => this.run(id));
    destroyRef.onDestroy(() => void menu.then((stop) => stop()));
    effect((onCleanup) => {
      const { theme: setting, colorSchema } = this.prefs.settings();
      if (this.schemes.sync(colorSchema)) return;
      if (setting !== "system") {
        this.theme.setDark(setting === "dark");
        return;
      }
      onCleanup(this.followSystemTheme());
    });
    void this.prefs.start();
    void this.store.start().then(() => this.dropMissing());
  }

  /** Set when restored tabs lost their pane, until dismissed. */
  protected readonly notice = signal<string | null>(null);

  protected dismissNotice(): void {
    this.notice.set(null);
  }

  /** After the first snapshot, closes restored tabs whose pane is gone, and says so. */
  private dropMissing(): void {
    if (this.store.connectionError() !== null) return;
    const gone = this.editor.prune(new Set(this.store.panes().keys()));
    if (gone.length === 0) return;
    this.notice.set(
      gone.length === 1
        ? "1 tab was closed because its pane no longer exists."
        : `${gone.length} tabs were closed because their panes no longer exist.`,
    );
  }

  protected readonly rootNode = computed<LayoutNode>(() => this.editor.layout());

  protected readonly theme$ = computed(() => this.prefs.settings().theme);

  protected setTheme(theme: ThemeSetting): void {
    void this.prefs.setTheme(theme);
  }

  /**
   * Keyboard shortcuts (editable in the settings file). A focused terminal only yields the ones
   * that use Cmd; the rest of its keys belong to the program in the pane.
   */
  @HostListener("window:keydown", ["$event"])
  protected onKey(event: KeyboardEvent): void {
    const target = event.target instanceof Element ? event.target : null;
    const action = this.keymap.match(event, target?.closest(".xterm") != null);
    if (!action) return;
    event.preventDefault();
    this.run(action);
  }

  /** Runs a menu or shortcut action; ids come from the native menu and the shortcut table. */
  protected run(id: string): void {
    const action = id as ShortcutAction;
    const active = this.editor.activeId();
    switch (action) {
      case "closeTab":
        if (active) this.editor.close(active);
        break;
      case "closeAll":
        this.editor.closeAll();
        break;
      case "refresh":
        void this.store.refresh();
        break;
      case "toggleSidebar":
        this.ui.toggleSidebar();
        break;
      case "splitRight":
        if (active) this.editor.split(active, "row");
        break;
      case "splitDown":
        if (active) this.editor.split(active, "column");
        break;
      case "zoomIn":
        this.ui.zoomBy(ZOOM_STEP);
        break;
      case "zoomOut":
        this.ui.zoomBy(-ZOOM_STEP);
        break;
      case "zoomReset":
        this.ui.zoomBy(null);
        break;
      case "nextWaiting":
        this.jumpToWaiting();
        break;
      case "nextTab":
        this.editor.cycle(1);
        break;
      case "previousTab":
        this.editor.cycle(-1);
        break;
      case "newTerminal":
        this.paneActions.askNew(active ? this.pane(active)?.workspace_id : undefined);
        break;
    }
  }

  /** Context menu of a sidebar row: new terminal in a workspace, split or close a pane. */
  protected onSidebarMenu(event: SidebarMenuAction): void {
    if (event.action === "new-terminal") this.paneActions.askNew(event.id);
    else this.paneAction(event.id, event.action);
  }

  private paneAction(paneId: string, action: string): void {
    if (action === "split-right" || action === "split-down") {
      const direction = action === "split-right" ? "right" : "down";
      void this.paneActions.split(paneId, direction).then((pane) => pane && this.open(pane, true));
    } else if (action === "start-agent") {
      this.paneActions.askStartAgent(paneId);
    } else if (action === "rename") {
      this.paneActions.askRename(paneId);
    } else if (action === "close") {
      this.paneActions.askClose(paneId);
    }
  }

  /** A pane closed in herdr: its tab goes with it. */
  protected onPaneClosed(paneId: string): void {
    this.editor.close(paneId);
  }

  /** Context menu of a group's tabs; splitting needs a second tab in the group. */
  protected menuFor(group: EditorGroup): TabBarMenuItem[] {
    const single = group.tabs.length < 2;
    return [
      {
        action: "split-right",
        label: "Split right",
        shortcut: this.keymap.label("splitRight"),
        disabled: single,
      },
      {
        action: "split-down",
        label: "Split down",
        shortcut: this.keymap.label("splitDown"),
        disabled: single,
      },
      { action: "close", label: "Close" },
      { action: "pane-split-right", label: "New pane to the right" },
      { action: "pane-split-down", label: "New pane below" },
      { action: "pane-start-agent", label: "Start agent..." },
      { action: "pane-rename", label: "Rename pane..." },
      { action: "pane-close", label: "Close herdr pane..." },
    ];
  }

  protected onTabMenu(id: string, action: string): void {
    if (action === "split-right") this.editor.split(id, "row");
    else if (action === "split-down") this.editor.split(id, "column");
    else if (action === "close") this.editor.close(id);
    else if (action.startsWith("pane-")) this.paneAction(id, action.slice("pane-".length));
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
