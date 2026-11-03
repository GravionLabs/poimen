import {
  ChangeDetectionStrategy,
  Component,
  DestroyRef,
  HostListener,
  computed,
  effect,
  untracked,
  signal,
  inject,
} from "@angular/core";
import { NgTemplateOutlet } from "@angular/common";
import { Title } from "@angular/platform-browser";
import { HxTheme } from "@gravionlabs/helix-ui";
import { rowName, tabHeader, type Mode, type Pane, type PluginAction } from "./core/herdr";
import { fixedSchemeName, pickSchemeName } from "./core/color-scheme";
import { ColorSchemeService } from "./core/color-scheme-service";
import { HerdrStore } from "./core/herdr-store";
import { SettingsStore, type ThemeSetting } from "./core/settings";
import { UserSchemes } from "./core/user-schemes";
import { TauriBridge } from "./core/tauri";
import { ACTIVITY_VIEWS, ActivityBar } from "./activity-bar/activity-bar";
import { Explorer } from "./explorer/explorer";
import { SIDEBAR_MAX_WIDTH, SIDEBAR_MIN_WIDTH, UiState } from "./core/ui-state";
import { Sidebar } from "./sidebar/sidebar";
import { TerminalView } from "./terminal-view/terminal-view";
import { EditorTabs, flowPane, tabKind, tabPaneId, type EditorGroup } from "./core/editor-tabs";
import { SettingsView } from "./settings-view/settings-view";
import { DeliverBoardView } from "./deliver-board/deliver-board-view";
import { BOARD_ENTRYPOINT, BOARD_PLUGIN } from "./core/deliver-board";
import { Keymap } from "./core/keymap";
import type { ShortcutAction } from "./core/shortcuts";
import type { LayoutNode } from "./core/editor-layout";
import { BottomPanel } from "./bottom-panel/bottom-panel";
import { FlowTab } from "./flow-tab/flow-tab";
import { Subagents } from "./core/transcripts";
import { Toolbar } from "./toolbar/toolbar";
import { PaneActions } from "./core/pane-actions";
import { PaneDialog } from "./pane-dialog/pane-dialog";
import { SpaceActions } from "./core/space-actions";
import { CommandPalette } from "./core/command-palette";
import { PluginActions, pluginMenuId } from "./core/plugin-actions";
import { PluginPanes, type PluginPaneDef } from "./core/plugin-panes";
import { CommandPalette as PaletteView } from "./command-palette/command-palette";
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
    SettingsView,
    DeliverBoardView,
    TabBar,
    SplitPane,
    StatusBar,
    Toolbar,
    BottomPanel,
    FlowTab,
    NgTemplateOutlet,
    PaneDialog,
    SpaceDialog,
    PaletteView,
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
  private readonly userSchemes = inject(UserSchemes);
  private readonly prefs = inject(SettingsStore);
  private readonly keymap = inject(Keymap);
  private readonly paneActions = inject(PaneActions);
  protected readonly spaceActions = inject(SpaceActions);
  protected readonly ui = inject(UiState);
  private readonly palette = inject(CommandPalette);
  protected readonly plugins = inject(PluginActions);
  private readonly pluginPanes = inject(PluginPanes);
  private readonly subagents = inject(Subagents);

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
    const names = this.pluginPanes.names();
    return new Map(
      this.editor.groups().map((group) => [
        group.id,
        group.tabs.map((tab): TabBarItem => {
          if (tabKind(tab.paneId) === "settings") {
            return { id: tab.paneId, label: "Settings", title: "Settings" };
          }
          if (tabKind(tab.paneId) === "board") {
            return { id: tab.paneId, label: "Deliver board", title: "Deliver board" };
          }
          const flowOf = flowPane(tab.paneId);
          const paneId = flowOf ?? tab.paneId;
          const pane = this.pane(paneId);
          const tabLabel = pane
            ? snap.tabs.find((t) => t.tab_id === pane.tab_id)?.label
            : undefined;
          const workspace = pane
            ? snap.workspaces.find((w) => w.workspace_id === pane.workspace_id)?.label
            : undefined;
          const pluginName = names.get(paneId);
          const header = tabHeader(
            workspace,
            pluginName ?? (pane ? rowName(pane, tabLabel) : paneId),
          );
          if (flowOf !== null) {
            return {
              id: tab.paneId,
              label: `Flow: ${header.label}`,
              group: header.group,
              title: `Agent flow of ${header.title}`,
            };
          }
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
    effect(() => {
      const { theme: setting, colorSchema } = this.prefs.settings();
      const systemDark = this.systemDark();
      this.schemes.restored();
      if (this.schemes.sync(pickSchemeName(colorSchema, setting, systemDark))) return;
      this.theme.setDark(setting === "system" ? systemDark : setting === "dark");
    });
    destroyRef.onDestroy(this.followSystemTheme());
    // The output panel follows the active tab's Claude Code agent; nothing is watched while it is hidden.
    effect(() => {
      const target = this.subagentTarget();
      untracked(() => void this.subagents.follow(target?.paneId ?? null, target?.session ?? null));
    });
    void this.plugins.load();
    void this.prefs.start();
    void this.userSchemes.start();
    // herdr may reuse pane ids: forget plugin panes that are gone once a snapshot is read.
    effect(() => {
      const panes = this.store.panes();
      if (this.store.loaded()) untracked(() => this.pluginPanes.prune(panes));
    });
    void this.store.start().then(() => this.dropMissing());
  }

  /**
   * The pane and Claude session the open output panel follows (the active tab's pane, or the pane
   * of the active flow tab); `null` when hidden or not Claude.
   */
  private readonly subagentTarget = computed(
    () => {
      const active = this.editor.activeId();
      const id = active === null ? null : tabPaneId(active);
      const pane = id && this.ui.panelOpen() ? this.pane(id) : undefined;
      const session = pane?.agent === "claude" ? pane.agent_session?.value : undefined;
      return pane && session ? { paneId: pane.pane_id, session } : null;
    },
    { equal: (a, b) => a?.paneId === b?.paneId && a?.session === b?.session },
  );

  /** Set when restored tabs lost their pane, until dismissed. */
  protected readonly notice = signal<string | null>(null);

  protected dismissNotice(): void {
    this.notice.set(null);
  }

  /** A plugin action failed in the command palette. */
  protected showError(message: string): void {
    this.notice.set(message);
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

  /** The scheme that fixes light or dark, so the theme button cannot act; `null` when it can. */
  protected readonly fixedBy = computed(() =>
    fixedSchemeName(this.prefs.settings().colorSchema, this.userSchemes.registry()),
  );

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
    if (id === "openSettings") {
      this.openSettings();
      return;
    }
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
      case "toggleOutput":
        this.ui.togglePanel();
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
      case "commandPalette":
        this.palette.toggle();
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
    if (event.kind === "pane" && event.action.startsWith("plugin:")) {
      void this.runPlugin(this.plugins.find(event.action), event.id);
    } else if (event.action === "new-terminal") this.paneActions.askNew(event.id);
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
      {
        action: "open-flow",
        label: "Open agent flow",
        appliesTo: (id) => tabKind(id) === "pane" && this.pane(id)?.agent === "claude",
      },
      {
        action: "plugin-pane-close",
        label: "Close plugin pane",
        appliesTo: (id) => this.pluginPanes.isPluginPane(id),
      },
      { action: "pane-split-right", label: "New pane to the right" },
      { action: "pane-split-down", label: "New pane below" },
      { action: "pane-start-agent", label: "Start agent..." },
      { action: "pane-rename", label: "Rename pane..." },
      { action: "pane-close", label: "Close herdr pane..." },
      ...this.plugins.actions().map((a) => ({ action: pluginMenuId(a), label: a.title })),
    ];
  }

  protected onTabMenu(id: string, action: string): void {
    if (action.startsWith("plugin:")) {
      if (tabKind(id) === "pane") void this.runPlugin(this.plugins.find(action), id);
    } else if (action === "plugin-pane-close") {
      void this.pluginPanes.close(id).then((error) => error && this.showError(error));
    } else if (action === "split-right") this.editor.split(id, "row");
    else if (action === "split-down") this.editor.split(id, "column");
    else if (action === "close") this.editor.close(id);
    else if (action === "open-flow") this.editor.openFlow(id);
    else if (action.startsWith("pane-") && tabKind(id) === "pane")
      this.paneAction(id, action.slice("pane-".length));
  }

  /** A tab is picked: show it, and bring a plugin pane to the front in herdr too. */
  protected onTabSelected(id: string): void {
    this.editor.activate(id);
    void this.pluginPanes.focus(id);
  }

  /** A plugin pane chosen in the palette opens as a herdr tab and as an editor tab. */
  protected async openPluginPane(def: PluginPaneDef): Promise<void> {
    if (def.plugin_id === BOARD_PLUGIN && def.entrypoint === BOARD_ENTRYPOINT) {
      this.editor.openBoard();
      return;
    }
    await this.openHerdrPluginPane(def);
  }

  private async openHerdrPluginPane(def: PluginPaneDef): Promise<void> {
    const active = this.activePaneId();
    const result = await this.pluginPanes.open(
      def,
      active ? this.pane(active)?.workspace_id : undefined,
    );
    if ("error" in result) {
      this.showError(result.error);
      return;
    }
    const pane = this.pane(result.paneId);
    if (pane) this.open(pane, true);
  }

  /** Runs a plugin action with `paneId` as context; a refusal shows as a notice. */
  protected async runPlugin(
    action: PluginAction | undefined,
    paneId: string | null,
  ): Promise<void> {
    if (!action) return;
    const error = await this.plugins.invoke(action, paneId);
    if (error) this.showError(error);
  }

  /** The Plugins menu opens: re-read the actions; the context menus use the same list. */
  @HostListener("window:focus")
  protected loadPlugins(): void {
    void this.plugins.load();
  }

  /** An action of the Plugins menu runs with the active pane as context. */
  protected onPluginChosen(action: PluginAction): void {
    void this.runPlugin(action, this.activePaneId());
  }

  protected groupPane(group: EditorGroup): Pane | undefined {
    return group.activeId && tabKind(group.activeId) === "pane"
      ? this.pane(group.activeId)
      : undefined;
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

  /** The active tab's pane id; `null` for no tab or the settings tab. */
  protected readonly activePaneId = computed(() => {
    const id = this.editor.activeId();
    return id !== null && tabKind(id) === "pane" ? id : null;
  });

  protected isSettings(id: string | null): boolean {
    return id !== null && tabKind(id) === "settings";
  }

  /** The pane of the group's active tab when that is a flow tab. */
  protected flowOf(group: EditorGroup): string | null {
    return group.activeId === null ? null : flowPane(group.activeId);
  }

  protected isFlow(id: string): boolean {
    return tabKind(id) === "flow";
  }

  /** A subagent node of a flow tab was clicked: show its transcript in the Output tab. */
  protected openAgentOutput(agentId: string): void {
    this.subagents.pick(agentId);
    this.ui.selectPanelTab("output");
  }

  protected isBoard(id: string | null): boolean {
    return id !== null && tabKind(id) === "board";
  }

  /** The terminal board of the plugin (herdr pane), for the unsupported state of the native one. */
  protected openTerminalBoard(): void {
    const def = this.pluginPanes
      .defs()
      .find((d) => d.plugin_id === BOARD_PLUGIN && d.entrypoint === BOARD_ENTRYPOINT);
    if (def) void this.openHerdrPluginPane(def);
    else this.showError("The deliver-board plugin has no terminal board pane.");
  }

  protected openSettings(): void {
    this.editor.openSettings();
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
    const id = this.activePaneId();
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

  /** The OS appearance; light when it cannot be read. */
  private readonly systemDark = signal(false);

  /** HxTheme reads the system setting once; track the OS appearance as it changes. */
  private followSystemTheme(): () => void {
    if (typeof matchMedia !== "function") return () => undefined;
    const query = matchMedia("(prefers-color-scheme: dark)");
    const update = () => this.systemDark.set(query.matches);
    update();
    query.addEventListener("change", update);
    return () => query.removeEventListener("change", update);
  }
}
