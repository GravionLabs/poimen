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
import { applyUiFont } from "./core/ui-font";
import { HxTheme } from "@gravionlabs/helix-ui";
import {
  flowSession,
  olderHost,
  olderHostNote,
  paneDir,
  paneTitle,
  rowName,
  splitId,
  supportsFlow,
  tabHeader,
  treeTerminals,
  withMachine,
  type Tab,
  type Terminal,
} from "./core/snapshot";
import { windowTitle, type Mode } from "./core/backend";
import { fixedSchemeName, pickSchemeName } from "./core/color-scheme";
import { ColorSchemeService } from "./core/color-scheme-service";
import { Machines } from "./core/machines";
import { RemoteSpaces } from "./core/remote-spaces";
import { Attention } from "./core/attention";
import { AgentRuns } from "./github/agent-runs";
import { DockBadge } from "./core/dock-badge";
import { NotificationCenter } from "./core/notification-center";
import { Sounds } from "./core/sounds";
import { Notifications } from "./notifications/notifications";
import { SpacesStore } from "./core/spaces-store";
import { SettingsStore, type ThemeSetting } from "./core/settings";
import { TerminalColorSync } from "./core/terminal-colors";
import { UserSchemes } from "./core/user-schemes";
import { TauriBridge } from "./core/tauri";
import { Updater } from "./core/updater";
import { ACTIVITY_VIEWS, ActivityBar } from "./activity-bar/activity-bar";
import { Explorer } from "./explorer/explorer";
import { SIDEBAR_MAX_WIDTH, SIDEBAR_MIN_WIDTH, UiState, type GithubPage } from "./core/ui-state";
import { Sidebar } from "./sidebar/sidebar";
import { TabLayout } from "./tab-layout/tab-layout";
import { TerminalView } from "./terminal-view/terminal-view";
import {
  EditorTabs,
  changesDir,
  flowPane,
  spaceOfTab,
  spaceTab,
  tabKind,
  tabPaneId,
  type EditorGroup,
} from "./core/editor-tabs";
import { SettingsView } from "./settings-view/settings-view";
import { GithubView } from "./github/github-view";
import { Deliver } from "./deliver/deliver";
import { DeliverView } from "./deliver/deliver-view";
import { Keymap } from "./core/keymap";
import type { ShortcutAction } from "./core/shortcuts";
import type { LayoutNode } from "./core/editor-layout";
import { BottomPanel } from "./bottom-panel/bottom-panel";
import { ChangesView } from "./changes/changes-view";
import { SpaceOverview } from "./space-overview/space-overview";
import { FlowTab } from "./flow-tab/flow-tab";
import { Subagents } from "./core/transcripts";
import { Toolbar } from "./toolbar/toolbar";
import { PaneActions, type PaneClosed } from "./core/pane-actions";
import { PaneDialog } from "./pane-dialog/pane-dialog";
import { SpaceActions } from "./core/space-actions";
import { SpaceRebuild } from "./core/space-rebuild";
import { CommandPalette } from "./core/command-palette";
import { ExplorerSelection } from "./core/explorer-selection";
import { CommandPalette as PaletteView } from "./command-palette/command-palette";
import { ShortcutsDialog } from "./shortcuts-dialog/shortcuts-dialog";
import { ShortcutsOverview } from "./core/shortcuts-overview";
import { SpaceDialog } from "./space-dialog/space-dialog";
import { TaskDialog } from "./task-dialog/task-dialog";
import { HooksDialog } from "./hooks-dialog/hooks-dialog";
import { ServerDialog } from "./server-dialog/server-dialog";
import { ServerRestart } from "./core/server-restart";
import { ResultDialog } from "./result-dialog/result-dialog";
import { TrustDialog } from "./trust-dialog/trust-dialog";
import { spaceRepoDir, type SidebarMenuAction } from "./sidebar/sidebar";
import { AgentActionDialog } from "./github/agent-action-dialog/agent-action-dialog";
import { AgentActions } from "./github/agent-actions";
import { TaskStarter } from "./core/task-start";
import { ProjectTeardown } from "./core/project-teardown";
import { ProjectEntries, type ProjectEntry } from "./core/project-entries";
import { AgentPicker as AgentPickerView } from "./agent-picker/agent-picker";
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
    TabLayout,
    SettingsView,
    GithubView,
    DeliverView,
    TabBar,
    SplitPane,
    StatusBar,
    Toolbar,
    BottomPanel,
    FlowTab,
    ChangesView,
    SpaceOverview,
    NgTemplateOutlet,
    PaneDialog,
    SpaceDialog,
    HooksDialog,
    ServerDialog,
    TrustDialog,
    ResultDialog,
    AgentPickerView,
    TaskDialog,
    AgentActionDialog,
    PaletteView,
    ShortcutsDialog,
  ],
  templateUrl: "./app.html",
  styleUrl: "./app.css",
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class App {
  protected readonly store = inject(SpacesStore);
  protected readonly updater = inject(Updater);
  protected readonly serverRestart = inject(ServerRestart);
  private readonly bridge = inject(TauriBridge);
  private readonly theme = inject(HxTheme);
  private readonly schemes = inject(ColorSchemeService);
  private readonly userSchemes = inject(UserSchemes);
  private readonly machines = inject(Machines);
  private readonly terminalColors = inject(TerminalColorSync);
  private readonly prefs = inject(SettingsStore);
  private readonly keymap = inject(Keymap);
  private readonly paneActions = inject(PaneActions);
  private readonly explorerSelection = inject(ExplorerSelection);
  protected readonly spaceActions = inject(SpaceActions);
  protected readonly ui = inject(UiState);
  private readonly palette = inject(CommandPalette);
  protected readonly shortcutsOverview = inject(ShortcutsOverview);
  private readonly subagents = inject(Subagents);
  private readonly tasks = inject(TaskStarter);
  private readonly cleanup = inject(TaskCleanup);
  private readonly teardown = inject(ProjectTeardown);
  private readonly projectEntries = inject(ProjectEntries);
  protected readonly agentActions = inject(AgentActions);
  protected readonly deliver = inject(Deliver);
  private readonly spaceRebuild = inject(SpaceRebuild);

  /** A click on the activity bar: switch the view; the active one collapses the sidebar. */
  protected selectView(id: string): void {
    const tab = ACTIVITY_VIEWS.find((v) => v.id === id)?.tab;
    if (tab === "github") {
      this.editor.openGithub();
      return;
    }
    if (tab === "deliver") {
      this.editor.openDeliver();
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
  private readonly remoteSpaces = inject(RemoteSpaces);
  private readonly agentRuns = inject(AgentRuns);
  private readonly center = inject(NotificationCenter);
  private readonly sounds = inject(Sounds);
  /** Last known state of each open pane, kept when herdr drops the pane. */
  private readonly known = new Map<string, Terminal>();
  protected readonly groupItems = computed(() => {
    const snap = this.store.all();
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
          if (tabKind(tab.paneId) === "deliver") {
            return { id: tab.paneId, label: "Deliver", title: "Deliver loops" };
          }
          const spaceOf = spaceOfTab(tab.paneId);
          if (spaceOf !== null) {
            const name = snap.spaces.find((w) => w.space_id === spaceOf)?.label ?? spaceOf;
            return { id: tab.paneId, label: `Space: ${name}`, title: `Overview of space ${name}` };
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
          if (pane?.machine)
            header.group = `${pane.machine}: ${header.group ?? ""}`.replace(/: $/, "");
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
            note: this.hostNote(pane),
            title: `${withMachine(header.title, pane?.machine)}${tab.pinned ? "" : " (preview)"}`,
          };
        }),
      ]),
    );
  });
  /** The leaf the user focused last per split tab; the server's focus is the fallback. */
  private readonly leafFocus = signal<ReadonlyMap<string, string>>(new Map());
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
    effect(() => {
      const id = this.activePaneId();
      const pane = id ? this.pane(id) : undefined;
      this.projectEntries.setDir(pane ? paneDir(pane) : undefined);
    });
    // Every view shows the hook status combined with the runs a pane started (#540).
    effect(() => this.store.setStatusOf(this.deliver.paneStatus()));
    // Instantiated for its effect: the dock badge follows `Attention.count`.
    inject(DockBadge);
    const title = inject(Title);
    // Marks follow herdr's statuses; the active tab of each group counts as viewed.
    effect(() => {
      const panes = this.store.panes();
      const viewed = this.viewedPanes();
      const statuses = new Map([...panes].map(([id, pane]) => [id, pane.agent_status] as const));
      untracked(() => {
        const observed = this.attention.observe(statuses, viewed);
        this.agentRuns.observe(statuses);
        this.center.record(observed, (id) => this.tabTitle(id), Date.now());
        // Remote agents count like local ones, but nothing plays for a machine that is not connected.
        this.sounds.notify(observed.filter((c) => this.machines.blockedReason(c.paneId) === null));
      });
    });
    effect(() => {
      title.setTitle(windowTitle(this.store.appChannel(), this.store.waiting()));
    });

    const destroyRef = inject(DestroyRef);
    const menu = this.bridge.listen<string>("menu", (id) => this.run(id));
    destroyRef.onDestroy(() => void menu.then((stop) => stop()));
    // The deliver loops of the repositories of the open spaces (status files, watched, no polling).
    destroyRef.onDestroy(this.deliver.start());
    // Spaces back from a server restart without terminals get their layout again.
    destroyRef.onDestroy(this.spaceRebuild.start());
    effect(() => {
      const dirs = this.store
        .snapshot()
        .terminals.map(paneDir)
        .filter((d): d is string => !!d);
      untracked(() => void this.deliver.watch(dirs));
    });
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
    effect(() => {
      const { uiFontFamily, uiFontSize } = this.prefs.settings();
      applyUiFont(document.documentElement, uiFontFamily, uiFontSize);
    });
    destroyRef.onDestroy(this.followSystemTheme());
    // The output panel follows the active tab's agent (Claude Code, Copilot CLI); nothing is watched while it is hidden.
    effect(() => {
      const target = this.subagentTarget();
      untracked(() => void this.subagents.follow(target?.paneId ?? null, target?.session ?? null));
    });
    void this.prefs.start();
    void this.userSchemes.start();
    void this.machines.start();
    void this.store.start().then(() => this.dropMissing());
    void this.remoteSpaces.start();
    void this.seedDefaults();
  }

  /**
   * The first start of this version writes the default layout and template into config.json when
   * it has neither key (`seed_space_defaults`). The UI state remembers that it ran; a failure is
   * retried at the next start.
   */
  private async seedDefaults(): Promise<void> {
    if (this.ui.defaultsSeeded()) return;
    try {
      await this.bridge.invoke("seed_space_defaults");
      this.ui.markDefaultsSeeded();
    } catch {
      // retried at the next start
    }
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
    // A machine's terminals are not read yet (or the machine is offline): their tabs stay.
    const alive = new Set(this.store.panes().keys());
    for (const id of this.editor.openIds()) if (splitId(id).machine) alive.add(id);
    const gone = this.editor.prune(alive);
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
    const githubPage = {
      githubInbox: "inbox",
      githubPulls: "pulls",
      githubIssues: "issues",
      githubMilestones: "milestones",
    }[id] as GithubPage | undefined;
    if (githubPage) {
      this.ui.selectGithubPage(githubPage);
      this.editor.openGithub();
      return;
    }
    if (id === "openDeliver") {
      this.editor.openDeliver();
      return;
    }
    if (id === "showSpaceOverview") {
      const spaceId = this.focusedSpace();
      if (spaceId) this.editor.openSpace(spaceId);
      else this.spaceActions.notify("Show Space Overview: there is no focused space");
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
    if (id === "openTerminalHere") {
      this.openTerminalInSelectedDir();
      return;
    }
    if (id === "cleanupTasks") {
      this.cleanupInActiveRepo();
      return;
    }
    if (id === "quitAndStopServer") {
      this.serverRestart.askQuit();
      return;
    }
    if (id === "projectTeardown") {
      this.teardownInActiveSpace();
      return;
    }
    if (id === "renameSpace") {
      const paneId = this.activePaneId();
      const spaceId = paneId ? this.pane(paneId)?.space_id : undefined;
      if (spaceId) this.spaceActions.askRename(spaceId);
      return;
    }
    if (id === "moveSpaceUp" || id === "moveSpaceDown") {
      const paneId = this.activePaneId();
      const spaceId = paneId ? this.pane(paneId)?.space_id : undefined;
      if (spaceId) this.spaceActions.shift(spaceId, id === "moveSpaceUp" ? -1 : 1);
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

  /** The palette entry: a terminal in the folder last picked in the explorer. */
  private openTerminalInSelectedDir(): void {
    const dir = this.explorerSelection.dir();
    if (!dir) {
      this.spaceActions.notify("Open terminal: select a folder in the explorer first");
      return;
    }
    void this.paneActions.openTerminalIn(dir).then((pane) => pane && this.open(pane, true));
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

  /** A command or preset of the project file, run in the space of the active pane. */
  protected runEntry(entry: ProjectEntry): void {
    const id = this.activePaneId();
    const pane = id ? this.pane(id) : undefined;
    const dir = pane ? paneDir(pane) : undefined;
    if (pane && dir) void this.projectEntries.run(entry, pane.space_id, dir);
    else this.spaceActions.notify(`Project: ${entry.name}: there is no focused space`);
  }

  /** The palette entry: run the project's teardown in the directory of the active pane. */
  private teardownInActiveSpace(): void {
    const id = this.activePaneId();
    const pane = id ? this.pane(id) : undefined;
    const dir = pane ? paneDir(pane) : undefined;
    if (dir) void this.teardown.run(dir);
    else this.spaceActions.notify("Project: Run teardown: the active pane has no directory");
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
    else if (event.action === "rename-space") this.spaceActions.askRename(event.id);
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
    void this.spaceActions.requestClose(workspaceId).then((ids) => {
      if (!ids) return;
      this.onSpaceClosed(ids);
      this.editor.close(spaceTab(workspaceId));
    });
  }

  /** A click or Enter on a space row of the sidebar: its overview tab. */
  protected openSpaceOverview(spaceId: string): void {
    this.editor.openSpace(spaceId);
  }

  /** Bumped by the gear icon so an open overview scrolls to its configuration section. */
  protected readonly configNonce = signal(0);

  /** The gear icon of a space row: the overview, scrolled to the configuration section. */
  protected openSpaceConfig(spaceId: string): void {
    this.editor.openSpace(spaceId);
    this.configNonce.update((n) => n + 1);
  }

  /** "Output" of the space overview: the pane's tab with the Output panel. */
  protected openPaneOutput(paneId: string): void {
    this.editor.open(paneId, true);
    this.ui.selectPanelTab("output");
  }

  /** "Open in explorer" of the space overview: the explorer view with the directory picked. */
  protected openInExplorer(dir: string): void {
    this.explorerSelection.select(dir);
    if (this.ui.activeView() !== "explorer" || this.ui.sidebarCollapsed()) {
      this.selectView("explorer");
    }
  }

  /** The space of the group's active tab when that is a space overview. */
  protected spaceOf(group: EditorGroup): string | null {
    return group.activeId === null ? null : spaceOfTab(group.activeId);
  }

  protected isSpace(id: string): boolean {
    return tabKind(id) === "space";
  }

  /** A space closed in herdr: all its panes go, so their tabs close without a successor. */
  protected onSpaceClosed(paneIds: string[]): void {
    for (const id of paneIds) this.editor.closePane(id);
  }

  private hostNote(pane: Terminal | undefined): string | undefined {
    const host = olderHost(pane);
    return host ? olderHostNote(host) : undefined;
  }

  protected paneAction(paneId: string, action: string): void {
    if (action === "split-right" || action === "split-down") {
      const direction = action === "split-right" ? "right" : "down";
      void this.paneActions.split(paneId, direction).then((pane) => pane && this.open(pane, true));
    } else if (action === "start-agent") {
      this.paneActions.askStartAgent(paneId);
    } else if (action === "rename") {
      this.paneActions.askRename(paneId);
    } else if (action === "restart") {
      this.paneActions.askRestart(paneId);
    } else if (action === "close") {
      this.paneActions.askClose(paneId);
    }
  }

  /**
   * A pane closed in herdr: its tab and its flow tab go with it, but when the pane's split tab still
   * has other leaves its editor tab passes to the successor resolved before the close, so the
   * split layout stays visible.
   */
  protected onPaneClosed(closed: PaneClosed): void {
    this.editor.closePane(closed.paneId, closed.successor);
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
      {
        action: "pane-restart",
        label: "Restart terminal on the new host...",
        appliesTo: (id) => tabKind(id) === "pane" && olderHost(this.pane(id)) !== null,
      },
      { action: "pane-close", label: "Close pane..." },
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

  /** The tab of the group's active pane when it is split (more than one leaf), else `undefined`. */
  protected groupLayout(group: EditorGroup): Tab | undefined {
    const pane = this.groupPane(group);
    const tab = pane && this.store.snapshot().tabs.find((t) => t.tab_id === pane.tab_id);
    return tab?.root?.kind === "split" ? tab : undefined;
  }

  /** The focused leaf of a split tab: the last click here, else the server's, else the first leaf. */
  protected layoutFocus(tab: Tab): string | null {
    return (
      this.leafFocus().get(tab.tab_id) ?? tab.active_terminal ?? treeTerminals(tab.root)[0] ?? null
    );
  }

  protected focusLeaf(tabId: string, terminalId: string): void {
    if (this.leafFocus().get(tabId) === terminalId) return;
    this.leafFocus.update((m) => new Map(m).set(tabId, terminalId));
    void this.paneActions.focusPane(tabId, terminalId);
  }

  protected resizeSplit(tabId: string, change: { splitId: string; ratio: number }): void {
    void this.paneActions.resizeSplit(tabId, change.splitId, change.ratio);
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

  protected isDeliver(id: string | null): boolean {
    return id !== null && tabKind(id) === "deliver";
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
  private tabTitle(id: string): string {
    const pane = this.pane(id);
    return pane ? paneTitle(this.store.all(), pane) : id;
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
