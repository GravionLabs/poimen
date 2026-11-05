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
import {
  flowSession,
  paneDir,
  rowName,
  supportsFlow,
  tabHeader,
  type Mode,
  type Terminal,
} from "./core/herdr";
import { fixedSchemeName, pickSchemeName } from "./core/color-scheme";
import { ColorSchemeService } from "./core/color-scheme-service";
import { Attention, type StatusChange } from "./core/attention";
import { DockBadge } from "./core/dock-badge";
import { Notifier } from "./core/notifier";
import { NotificationCenter } from "./core/notification-center";
import { Notifications } from "./notifications/notifications";
import { HerdrStore } from "./core/herdr-store";
import { SettingsStore, type ThemeSetting } from "./core/settings";
import { TerminalColorSync } from "./core/terminal-colors";
import { UserSchemes } from "./core/user-schemes";
import { TauriBridge } from "./core/tauri";
import { Updater } from "./core/updater";
import { ACTIVITY_VIEWS, ActivityBar } from "./activity-bar/activity-bar";
import { Explorer } from "./explorer/explorer";
import { SIDEBAR_MAX_WIDTH, SIDEBAR_MIN_WIDTH, UiState } from "./core/ui-state";
import { Sidebar } from "./sidebar/sidebar";
import { TerminalView } from "./terminal-view/terminal-view";
import {
  EditorTabs,
  changesDir,
  flowPane,
  flowTab,
  tabKind,
  tabPaneId,
  type EditorGroup,
} from "./core/editor-tabs";
import { SettingsView } from "./settings-view/settings-view";
import { GithubView } from "./github/github-view";
import { Keymap } from "./core/keymap";
import type { ShortcutAction } from "./core/shortcuts";
import type { LayoutNode } from "./core/editor-layout";
import { BottomPanel } from "./bottom-panel/bottom-panel";
import { ChangesView } from "./changes/changes-view";
import { FlowTab } from "./flow-tab/flow-tab";
import { Subagents } from "./core/transcripts";
import { Toolbar } from "./toolbar/toolbar";
import { PaneActions } from "./core/pane-actions";
import { PaneDialog } from "./pane-dialog/pane-dialog";
import { SpaceActions } from "./core/space-actions";
import { CommandPalette } from "./core/command-palette";
import { CommandPalette as PaletteView } from "./command-palette/command-palette";
import { ShortcutsDialog } from "./shortcuts-dialog/shortcuts-dialog";
import { ShortcutsOverview } from "./core/shortcuts-overview";
import { SpaceDialog } from "./space-dialog/space-dialog";
import { TaskDialog } from "./task-dialog/task-dialog";
import { TrustDialog } from "./trust-dialog/trust-dialog";
import { spaceRepoDir, type SidebarMenuAction } from "./sidebar/sidebar";
import { TaskStarter } from "./core/task-start";
import { TaskCleanup } from "./core/task-cleanup";
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
    Notifications,
    TerminalView,
    SettingsView,
    GithubView,
    TabBar,
    SplitPane,
    StatusBar,
    Toolbar,
    BottomPanel,
    FlowTab,
    ChangesView,
    NgTemplateOutlet,
    PaneDialog,
    SpaceDialog,
    TrustDialog,
    TaskDialog,
    PaletteView,
    ShortcutsDialog,
  ],
  templateUrl: "./app.html",
  styleUrl: "./app.css",
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class App {
  protected readonly store = inject(HerdrStore);
  protected readonly updater = inject(Updater);
  private readonly bridge = inject(TauriBridge);
  private readonly theme = inject(HxTheme);
  private readonly schemes = inject(ColorSchemeService);
  private readonly userSchemes = inject(UserSchemes);
  private readonly terminalColors = inject(TerminalColorSync);
  private readonly prefs = inject(SettingsStore);
  private readonly keymap = inject(Keymap);
  private readonly paneActions = inject(PaneActions);
  protected readonly spaceActions = inject(SpaceActions);
  protected readonly ui = inject(UiState);
  private readonly palette = inject(CommandPalette);
  protected readonly shortcutsOverview = inject(ShortcutsOverview);
  private readonly subagents = inject(Subagents);
  private readonly tasks = inject(TaskStarter);
  private readonly cleanup = inject(TaskCleanup);

  /** A click on the activity bar: switch the view; the active one collapses the sidebar. */
  protected selectView(id: string): void {
    if (ACTIVITY_VIEWS.find((v) => v.id === id)?.tab === "github") {
      this.editor.openGithub();
      return;
    }
    this.ui.selectView(
      id,
      ACTIVITY_VIEWS.filter((v) => !v.tab).map((v) => v.id),
    );
  }
  protected readonly minWidth = SIDEBAR_MIN_WIDTH;
  protected readonly maxWidth = SIDEBAR_MAX_WIDTH;
  private drag: { startX: number; startWidth: number } | null = null;

  protected readonly editor = inject(EditorTabs);
  private readonly attention = inject(Attention);
  private readonly notifier = inject(Notifier);
  private readonly center = inject(NotificationCenter);
  private readonly dockBadge = inject(DockBadge);
  /** Last known state of each open pane, kept when herdr drops the pane. */
  private readonly known = new Map<string, Terminal>();
  protected readonly groupItems = computed(() => {
    const snap = this.store.snapshot();
    this.store.panes();
    const ringing = this.attention.attention();
    const unread = this.attention.unread();
    return new Map(
      this.editor.groups().map((group) => [
        group.id,
        group.tabs.map((tab): TabBarItem => {
          if (tabKind(tab.paneId) === "settings") {
            return { id: tab.paneId, label: "Settings", title: "Settings" };
          }
          if (tabKind(tab.paneId) === "github") {
            return {
              id: tab.paneId,
              label: "Pull requests",
              group: "GitHub",
              title: "GitHub pull requests",
            };
          }
          const changesOf = changesDir(tab.paneId);
          if (changesOf !== null) {
            const name = changesOf.split("/").filter(Boolean).pop() ?? changesOf;
            return {
              id: tab.paneId,
              label: `Changes: ${name}`,
              title: `Changes of ${changesOf}`,
            };
          }
          const flowOf = flowPane(tab.paneId);
          const paneId = flowOf ?? tab.paneId;
          const pane = this.pane(paneId);
          const tabLabel = pane
            ? snap.tabs.find((t) => t.tab_id === pane.tab_id)?.label
            : undefined;
          const workspace = pane
            ? snap.spaces.find((w) => w.space_id === pane.space_id)?.label
            : undefined;
          const header = tabHeader(workspace, pane ? rowName(pane, tabLabel) : paneId);
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
            attention: ringing.has(tab.paneId),
            unread: unread.has(tab.paneId),
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

  /** Panes whose tab is on screen: the active tab of each editor group. */
  private readonly viewedPanes = computed(
    () => new Set(this.editor.groups().flatMap((g) => (g.activeId ? [g.activeId] : []))),
  );

  constructor() {
    const title = inject(Title);
    // Marks follow herdr's statuses; the active tab of each group counts as viewed.
    effect(() => {
      const panes = this.store.panes();
      const viewed = this.viewedPanes();
      const statuses = new Map([...panes].map(([id, pane]) => [id, pane.agent_status] as const));
      untracked(() => {
        const changes = this.attention.observe(statuses, viewed);
        this.center.record(changes, (id) => this.tabTitle(id), Date.now());
        this.notifyChanges(changes);
      });
    });
    effect(() => {
      const waiting = this.store.waiting();
      title.setTitle(waiting > 0 ? `poimen (${waiting} waiting)` : "poimen");
      untracked(() => this.dockBadge.set(waiting));
    });

    const destroyRef = inject(DestroyRef);
    // Coming back to the window sees the active tabs again: withdraw their notifications.
    const onFocus = () => this.notifyChanges([]);
    window.addEventListener("focus", onFocus);
    destroyRef.onDestroy(() => window.removeEventListener("focus", onFocus));
    const menu = this.bridge.listen<string>("menu", (id) => this.run(id));
    destroyRef.onDestroy(() => void menu.then((stop) => stop()));
    // A click on a system notification opens the tab of the pane it was about.
    const clicks = this.notifier.listen((paneId) => this.openPaneById(paneId));
    destroyRef.onDestroy(() => void clicks.then((stop) => stop()));
    effect(() => {
      const { theme: setting, colorSchema } = this.prefs.settings();
      const systemDark = this.systemDark();
      this.schemes.restored();
      if (this.schemes.sync(pickSchemeName(colorSchema, setting, systemDark))) return;
      this.theme.setDark(setting === "system" ? systemDark : setting === "dark");
    });
    // poimen-server answers color queries while no app is attached, with the colors shown here.
    // The tokens land on <html> after the effect above, so they are read when they change.
    const tokens = new MutationObserver(() =>
      this.terminalColors.update(this.prefs.settings().colors),
    );
    tokens.observe(document.documentElement, {
      attributes: true,
      attributeFilter: ["class", "data-theme", "style"],
    });
    destroyRef.onDestroy(() => tokens.disconnect());
    effect(() => this.terminalColors.update(this.prefs.settings().colors));
    destroyRef.onDestroy(this.followSystemTheme());
    // The output panel follows the active tab's agent (Claude Code, Copilot CLI); nothing is watched while it is hidden.
    effect(() => {
      const target = this.subagentTarget();
      untracked(() => void this.subagents.follow(target?.paneId ?? null, target?.session ?? null));
    });
    void this.prefs.start();
    void this.userSchemes.start();
    void this.store.start().then(() => this.dropMissing());
  }

  /**
   * The pane and agent session the open output panel follows (the active tab's pane, or the pane
   * of the active flow tab); `null` when hidden or its agent has no transcripts to follow.
   */
  private readonly subagentTarget = computed(
    () => {
      const active = this.editor.activeId();
      const id = active === null ? null : tabPaneId(active);
      const pane = id && this.ui.panelOpen() ? this.pane(id) : undefined;
      const session = flowSession(pane);
      return pane && session ? { paneId: pane.terminal_id, session } : null;
    },
    { equal: (a, b) => a?.paneId === b?.paneId && a?.session === b?.session },
  );

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

  /** Shortcut of the output panel toggle, for the status bar button. */
  protected readonly panelShortcut = computed(() => this.keymap.label("toggleOutput"));
  protected readonly shortcutsShortcut = computed(() => this.keymap.label("showShortcuts"));

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
    if (id === "openGithub") {
      this.editor.openGithub();
      return;
    }
    if (id === "showChanges") {
      this.showChangesOfActiveSpace();
      return;
    }
    if (id === "startTask") {
      this.startTaskInActiveRepo();
      return;
    }
    if (id === "cleanupTasks") {
      this.cleanupInActiveRepo();
      return;
    }
    if (id === "closeSpace") {
      const paneId = this.activePaneId();
      this.closeSpace(paneId ? this.pane(paneId)?.space_id : undefined);
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
      case "showShortcuts":
        this.shortcutsOverview.open();
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
        this.paneActions.askNew(active ? this.pane(active)?.space_id : undefined);
        break;
    }
  }

  /** The palette entry: the Changes tab of the directory of the focused space's active pane. */
  private showChangesOfActiveSpace(): void {
    const id = this.activePaneId();
    const pane = id ? this.pane(id) : undefined;
    const dir = pane ? paneDir(pane) : undefined;
    if (dir) this.editor.openChanges(this.activeRepo() ?? dir);
    else this.spaceActions.notify("Show Changes: there is no focused space");
  }

  /** The palette entry: the picker for the repository of the active pane. */
  private startTaskInActiveRepo(): void {
    const repo = this.activeRepo();
    if (repo) void this.tasks.start(repo);
    else this.spaceActions.notify("Start task: the active pane is not in a repository");
  }

  /** The palette entry: clean up the merged tasks of the repository of the active pane. */
  private cleanupInActiveRepo(): void {
    const repo = this.activeRepo();
    if (repo) void this.cleanup.run(repo, this.focusedSpace());
    else this.spaceActions.notify("Clean up merged tasks: the active pane is not in a repository");
  }

  /** The space of the active pane: a cleanup never removes it. */
  private focusedSpace(): string | null {
    const id = this.activePaneId();
    return (id ? this.pane(id)?.space_id : undefined) ?? null;
  }

  /** The repository of the active pane (a pane in a subdirectory: the deepest known one above). */
  private activeRepo(): string | undefined {
    const id = this.activePaneId();
    const pane = id ? this.pane(id) : undefined;
    const dir = pane ? paneDir(pane) : undefined;
    return dir
      ? [...this.store.repos().keys()]
          .filter((r) => dir === r || dir.startsWith(r.endsWith("/") ? r : `${r}/`))
          .sort((a, b) => b.length - a.length)[0]
      : undefined;
  }

  /** Context menu of a sidebar row: new terminal in a workspace, split or close a pane. */
  protected onSidebarMenu(event: SidebarMenuAction): void {
    if (event.action === "new-terminal") this.paneActions.askNew(event.id);
    else if (event.action === "close-space") this.closeSpace(event.id);
    else if (event.action === "show-changes") {
      const dir = spaceRepoDir(this.store.spaces(), event.id);
      if (dir) this.editor.openChanges(dir);
    } else if (event.action === "start-task") {
      const dir = spaceRepoDir(this.store.spaces(), event.id);
      if (dir) void this.tasks.start(dir);
    } else if (event.action === "cleanup-tasks") {
      const dir = spaceRepoDir(this.store.spaces(), event.id);
      if (dir) void this.cleanup.run(dir, this.focusedSpace());
    } else this.paneAction(event.id, event.action);
  }

  /** Closes a space (asks first when it has panes); `undefined` is a no-op (no active space). */
  protected closeSpace(workspaceId: string | undefined): void {
    if (!workspaceId) return;
    void this.spaceActions.requestClose(workspaceId).then((ids) => ids && this.onSpaceClosed(ids));
  }

  /** A space closed in herdr: the tabs of its panes go with it. */
  protected onSpaceClosed(paneIds: string[]): void {
    for (const id of paneIds) this.onPaneClosed(id);
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

  /** A pane closed in herdr: its tab and its flow tab go with it. */
  protected onPaneClosed(paneId: string): void {
    this.editor.close(paneId);
    this.editor.close(flowTab(paneId));
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
        appliesTo: (id) => tabKind(id) === "pane" && supportsFlow(this.pane(id)),
      },
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
    else if (action === "open-flow") this.editor.openFlow(id);
    else if (action.startsWith("pane-") && tabKind(id) === "pane")
      this.paneAction(id, action.slice("pane-".length));
  }

  protected groupPane(group: EditorGroup): Terminal | undefined {
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
  /** Opens the tab of a pane by id; a pane herdr no longer has is ignored. */
  protected openPaneById(paneId: string): void {
    const pane = this.pane(paneId);
    if (pane) this.open(pane);
  }

  protected open(pane: Terminal, pin = false): void {
    this.known.set(pane.terminal_id, pane);
    this.editor.open(pane.terminal_id, pin);
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

  /** The directory of the group's active tab when that is a changes tab. */
  protected changesOf(group: EditorGroup): string | null {
    return group.activeId === null ? null : changesDir(group.activeId);
  }

  protected isChanges(id: string): boolean {
    return tabKind(id) === "changes";
  }

  protected isGithub(id: string | null): boolean {
    return id !== null && tabKind(id) === "github";
  }

  protected openSettings(): void {
    this.editor.openSettings();
  }

  protected tabPane(id: string): Terminal | undefined {
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

  /** Space and name of a pane, as its tab shows them. */
  /** Sends notifications for `changes` and withdraws those of panes the user sees now. */
  private notifyChanges(changes: readonly StatusChange[]): void {
    const { notifyOnBlocked, notifyOnDone } = this.prefs.settings();
    this.notifier.notify(changes, {
      // Read live: a delayed done notification checks again when it is due.
      seen: (id) => document.hasFocus() && this.viewedPanes().has(id),
      status: (id) => this.store.panes().get(id)?.agent_status,
      onBlocked: notifyOnBlocked,
      onDone: notifyOnDone,
      label: (id) => this.tabTitle(id),
      now: Date.now(),
    });
  }

  private tabTitle(id: string): string {
    const snap = this.store.snapshot();
    const pane = this.pane(id);
    if (!pane) return id;
    const tabLabel = snap.tabs.find((t) => t.tab_id === pane.tab_id)?.label;
    const workspace = snap.spaces.find((w) => w.space_id === pane.space_id)?.label;
    return tabHeader(workspace, rowName(pane, tabLabel)).title;
  }

  private pane(id: string): Terminal | undefined {
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
