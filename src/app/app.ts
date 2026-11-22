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
  viewChild,
  type TemplateRef,
} from "@angular/core";
import { NgTemplateOutlet } from "@angular/common";
import { Title } from "@angular/platform-browser";
import { applyUiFont } from "./core/ui-font";
import { HxActivityBar, HxAppShell, type HxSidebarState } from "@gravionlabs/helix-desktop";
import { HxTheme } from "@gravionlabs/helix-ui";
import {
  flowSession,
  olderHost,
  olderHostNote,
  paneDir,
  paneTitle,
  rowName,
  splitId,
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
import { Attention, canMarkUnread, warningIds } from "./core/attention";
import { Presence } from "./core/presence";
import { AgentRuns } from "./github/agent-runs";
import { DockBadge } from "./core/dock-badge";
import { NotificationCenter } from "./core/notification-center";
import { tabMarks } from "./core/agent-priority";
import { TokenTotalsService } from "./core/token-totals";
import { Sounds, soundChanges } from "./core/sounds";
import { Notifications } from "./notifications/notifications";
import { SpacesStore } from "./core/spaces-store";
import { openAtFor } from "./core/tab-place";
import { SettingsStore, type ThemeSetting } from "./core/settings";
import { TerminalColorSync } from "./core/terminal-colors";
import { UserSchemes } from "./core/user-schemes";
import { TauriBridge } from "./core/tauri";
import { Updater } from "./core/updater";
import { SIDEBAR_VIEW_IDS, activityItems, tabOf } from "./core/activity-items";
import { Explorer } from "./explorer/explorer";
import { SIDEBAR_MAX_WIDTH, SIDEBAR_MIN_WIDTH, UiState, type GithubPage } from "./core/ui-state";
import { followSystemTheme } from "./core/system-theme";
import { RecentTabs } from "./recent-tabs/recent-tabs";
import { Toasts } from "./ui/toasts/toasts";
import { Sidebar } from "./sidebar/sidebar";
import { TabLayout } from "./tab-layout/tab-layout";
import { HiddenViews } from "./terminal-view/hidden-views";
import { TerminalView } from "./terminal-view/terminal-view";
import {
  EditorTabs,
  changesDir,
  flowPane,
  spaceOfTab,
  spaceTab,
  tabKind,
  withKindIcons,
  tabPaneId,
  type EditorGroup,
} from "./core/editor-tabs";
import { SettingsView } from "./settings-view/settings-view";
import { GithubView } from "./github/github-view";
import { ClaudeLimits } from "./core/claude-limits";
import { Deliver } from "./deliver/deliver";
import { DeliverView } from "./deliver/deliver-view";
import { Keymap } from "./core/keymap";
import { TerminalSearch } from "./terminal-view/search";
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
import { tabMenuItems } from "./tab-menu";
import { ResumeDialog } from "./resume-dialog/resume-dialog";
import { SessionResume } from "./core/session-resume";
import { SpaceActions } from "./core/space-actions";
import { SpaceRebuild } from "./core/space-rebuild";
import { CommandPalette } from "./core/command-palette";
import { JumpPalette, type JumpEntry } from "./core/jump-palette";
import { JumpPaletteView } from "./jump-palette/jump-palette";
import { ExternalEditor } from "./core/external-editor";
import { ExplorerSelection } from "./core/explorer-selection";
import { CommandPalette as PaletteView } from "./command-palette/command-palette";
import { ShortcutsDialog } from "./shortcuts-dialog/shortcuts-dialog";
import { ShortcutsOverview } from "./core/shortcuts-overview";
import { SpaceDialog } from "./space-dialog/space-dialog";
import { TaskDialog } from "./task-dialog/task-dialog";
import { HooksDialog } from "./hooks-dialog/hooks-dialog";
import { ServerDialog } from "./server-dialog/server-dialog";
import { AgentConfirmDialog } from "./agent-confirm-dialog/agent-confirm-dialog";
import { AgentConfirm } from "./core/agent-confirm";
import { ServerRestart } from "./core/server-restart";
import { ResultDialog } from "./result-dialog/result-dialog";
import { TrustDialog } from "./trust-dialog/trust-dialog";
import { spaceRepoDir, type SidebarMenuAction } from "./sidebar/sidebar";
import { AgentActionDialog } from "./github/agent-action-dialog/agent-action-dialog";
import { AgentActions } from "./github/agent-actions";
import { IssueTasks } from "./github/issue-task";
import { GithubPulls } from "./github/pulls";
import { TaskStarter } from "./core/task-start";
import { ProjectTeardown } from "./core/project-teardown";
import { ProjectEntries, type ProjectEntry } from "./core/project-entries";
import { AgentPicker as AgentPickerView } from "./agent-picker/agent-picker";
import { TaskCleanup } from "./core/task-cleanup";
import { StatusBar } from "./status-bar/status-bar";
import { Icon } from "./ui/icon/icon";
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
    HxActivityBar,
    RecentTabs,
    Toasts,
    HxAppShell,
    Icon,
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
    ResumeDialog,
    SpaceDialog,
    HooksDialog,
    ServerDialog,
    AgentConfirmDialog,
    TrustDialog,
    ResultDialog,
    AgentPickerView,
    TaskDialog,
    AgentActionDialog,
    PaletteView,
    JumpPaletteView,
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
  private readonly agentConfirm = inject(AgentConfirm);
  private readonly bridge = inject(TauriBridge);
  private readonly theme = inject(HxTheme);
  private readonly schemes = inject(ColorSchemeService);
  private readonly userSchemes = inject(UserSchemes);
  private readonly machines = inject(Machines);
  private readonly terminalColors = inject(TerminalColorSync);
  private readonly prefs = inject(SettingsStore);
  private readonly keymap = inject(Keymap);
  private readonly terminalSearch = inject(TerminalSearch);
  private readonly paneActions = inject(PaneActions);
  private readonly sessionResume = inject(SessionResume);
  private readonly explorerSelection = inject(ExplorerSelection);
  private readonly externalEditor = inject(ExternalEditor);
  protected readonly spaceActions = inject(SpaceActions);
  protected readonly ui = inject(UiState);
  private readonly palette = inject(CommandPalette);
  private readonly jump = inject(JumpPalette);
  protected readonly shortcutsOverview = inject(ShortcutsOverview);
  private readonly subagents = inject(Subagents);
  private readonly tasks = inject(TaskStarter);
  protected readonly issueTasks = inject(IssueTasks);
  private readonly cleanup = inject(TaskCleanup);
  private readonly teardown = inject(ProjectTeardown);
  private readonly projectEntries = inject(ProjectEntries);
  protected readonly agentActions = inject(AgentActions);
  protected readonly pulls = inject(GithubPulls);
  protected readonly deliver = inject(Deliver);
  protected readonly claudeLimits = inject(ClaudeLimits);
  private readonly spaceRebuild = inject(SpaceRebuild);

  private readonly spacesIcon = viewChild.required<TemplateRef<unknown>>("spacesIcon");
  private readonly explorerIcon = viewChild.required<TemplateRef<unknown>>("explorerIcon");
  private readonly notificationsIcon =
    viewChild.required<TemplateRef<unknown>>("notificationsIcon");
  private readonly githubIcon = viewChild.required<TemplateRef<unknown>>("githubIcon");
  private readonly deliverIcon = viewChild.required<TemplateRef<unknown>>("deliverIcon");
  private readonly settingsIcon = viewChild.required<TemplateRef<unknown>>("settingsIcon");

  /** The items of the activity bar, with the SVG templates of app.html. */
  protected readonly activityItems = computed(() =>
    activityItems({
      spaces: this.spacesIcon(),
      explorer: this.explorerIcon(),
      notifications: this.notificationsIcon(),
      github: this.githubIcon(),
      deliver: this.deliverIcon(),
      settings: this.settingsIcon(),
    }),
  );

  /** A click on the activity bar: switch the view; the active one collapses the sidebar. */
  protected selectView(id: string): void {
    const tab = tabOf(id);
    if (tab === "github") {
      this.editor.openGithub();
      return;
    }
    if (tab === "deliver") {
      this.editor.openDeliver();
      return;
    }
    if (tab === "settings") {
      this.openSettings();
      return;
    }
    this.ui.selectView(id, SIDEBAR_VIEW_IDS);
  }
  protected readonly minWidth = SIDEBAR_MIN_WIDTH;
  protected readonly maxWidth = SIDEBAR_MAX_WIDTH;

  protected readonly editor = inject(EditorTabs);
  private readonly hiddenViews = inject(HiddenViews);
  private readonly attention = inject(Attention);
  private readonly remoteSpaces = inject(RemoteSpaces);
  private readonly agentRuns = inject(AgentRuns);
  private readonly center = inject(NotificationCenter);
  private readonly tokenTotals = inject(TokenTotalsService);
  private readonly sounds = inject(Sounds);
  /** Last known state of each open pane, kept when herdr drops the pane. */
  private readonly known = new Map<string, Terminal>();
  /** The clock of the silent marks on tabs; ticks once a minute. */
  private readonly clock = signal(Date.now());
  private readonly baseItems = computed(() => {
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
            return { id: tab.paneId, label: "Runs", title: "Agent runs" };
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
          const base = `${withMachine(header.title, pane?.machine)}${tab.pinned ? "" : " (preview)"}`;
          return {
            id: tab.paneId,
            label: header.label,
            group: header.group,
            preview: !tab.pinned,
            attention: ringing.has(tab.paneId),
            unread: unread.has(tab.paneId),
            note: this.hostNote(pane),
            title: tabMarks(base, pane, this.clock()),
          };
        }),
      ]),
    );
  });
  protected readonly groupItems = computed(() => withKindIcons(this.baseItems()));
  /** The leaf the user focused last per split tab; the server's focus is the fallback. */
  private readonly leafFocus = signal<ReadonlyMap<string, string>>(new Map());
  /** Size of each terminal that reported one; the status bar shows the focused group's. */
  private readonly sizes = signal<ReadonlyMap<string, { cols: number; rows: number }>>(new Map());
  protected readonly termSize = computed(() => {
    const id = this.editor.activeId();
    return id ? this.sizes().get(id) : undefined;
  });

  private readonly presence = inject(Presence);
  /** Active tab of each editor group, while the user is present (#1030). */
  private readonly viewedPanes = computed(() =>
    this.presence.whenPresent(this.editor.groups().flatMap((g) => g.activeId ?? [])),
  );

  constructor() {
    effect(() => this.projectEntries.setDir(this.activeDir()));
    // The space of the active tab (a pane or the overview) is the one the user focused last (#712).
    effect(() => {
      const id = this.editor.activeId();
      const spaceId = id === null ? null : (spaceOfTab(id) ?? this.pane(id)?.space_id ?? null);
      if (!spaceId) return;
      untracked(() => {
        // Only this Mac's spaces: `recent` falls back to the name for a machine's.
        if (this.store.snapshot().spaces.some((s) => s.space_id === spaceId))
          this.ui.touchSpace(spaceId, Date.now());
      });
    });
    // Every view shows the hook status combined with the runs a pane started (#540).
    effect(() => this.store.setStatusOf(this.deliver.paneStatus()));
    // Token totals of every agent pane with a transcript (#895).
    effect(() => {
      const panes = new Map<string, string>();
      for (const pane of this.store.panes().values()) {
        const session = flowSession(pane);
        // A machine that is not connected is not followed: its watch ends with the connection
        // and starts again (from the beginning) when the machine is back.
        if (session && !this.machines.blockedReason(pane.terminal_id)) {
          panes.set(pane.terminal_id, session);
        }
      }
      void this.tokenTotals.follow(panes);
    });
    // Instantiated for its effect: the dock badge follows `Attention.count`.
    inject(DockBadge);
    const title = inject(Title);
    // Marks follow herdr's statuses; the active tab of each group counts as viewed.
    effect(() => {
      const panes = this.store.panes();
      const viewed = this.viewedPanes();
      const statuses = new Map([...panes].map(([id, pane]) => [id, pane.agent_status] as const));
      untracked(() => {
        const observed = this.attention.observe(statuses, viewed, Date.now(), warningIds(panes));
        this.agentRuns.observe(statuses);
        this.center.record(observed, (id) => this.tabTitle(id), Date.now());
        this.center.observeSilent(
          new Map([...panes].map(([id, pane]) => [id, pane.agent_silent_since] as const)),
          (id) => this.tabTitle(id),
          Date.now(),
        );
        // Remote agents count like local ones, but nothing plays for a pane being viewed or for a
        // machine that is not connected.
        this.sounds.notify(soundChanges(observed, viewed, (id) => this.machines.blockedReason(id)));
      });
    });
    effect(() => {
      title.setTitle(windowTitle(this.store.appChannel(), this.store.waiting()));
    });

    const destroyRef = inject(DestroyRef);
    const clockTimer = setInterval(() => this.clock.set(Date.now()), 60_000);
    destroyRef.onDestroy(() => clearInterval(clockTimer));
    const menu = this.bridge.listen<string>("menu", (id) => this.run(id));
    destroyRef.onDestroy(() => void menu.then((stop) => stop()));
    // The deliver loops of the repositories of the open spaces (status files, watched, no polling).
    destroyRef.onDestroy(this.deliver.start());
    // The Claude limits (#784): at start, every 5 minutes and on focus, only while the app runs.
    destroyRef.onDestroy(this.claudeLimits.start());
    // Spaces back from a server restart without terminals get their layout again.
    destroyRef.onDestroy(this.spaceRebuild.start());
    // Questions of agents that want to write into another space (#615).
    destroyRef.onDestroy(this.agentConfirm.start());
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
    destroyRef.onDestroy(followSystemTheme((dark) => this.systemDark.set(dark)));
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

  /** After the first snapshot, closes restored tabs whose pane is gone, and says so. */
  private dropMissing(): void {
    if (this.store.connectionError() !== null) return;
    // A machine's terminals are not read yet (or the machine is offline): their tabs stay.
    const alive = new Set(this.store.panes().keys());
    for (const id of this.editor.openIds()) if (splitId(id).machine) alive.add(id);
    const gone = this.editor.prune(alive);
    if (gone.length === 0) return;
    this.spaceActions.notify(
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
    if (id === "addMachine") {
      this.machines.openForm();
      this.editor.openSettings("machines");
      return;
    }
    if (id === "openMachines") {
      this.editor.openSettings("machines");
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
    if (id === "markUnread") {
      const paneId = this.activePaneId();
      const pane = paneId ? this.pane(paneId) : undefined;
      if (canMarkUnread(pane, this.attention.unread())) this.attention.markUnread(pane.terminal_id);
      else this.spaceActions.notify("Mark as unread: the active pane is not an idle or done agent");
      return;
    }
    if (id === "showChanges") {
      this.showChangesOfActiveSpace();
      return;
    }
    if (id === "startIssueTask") {
      const [t] = this.agentActions.current();
      if (t?.kind === "issue") void this.issueTasks.start(t.root, t.number);
      return;
    }
    if (id === "startTask") return this.startTaskInActiveRepo();
    if (id === "resumeSession") return this.sessionResume.openActive(this.activePaneId());
    if (id === "openInEditor") {
      this.externalEditor.openDir(this.activeDir());
      return;
    }
    if (id === "openTerminalHere") return this.openTerminalInSelectedDir();
    if (id === "cleanupTasks") return this.cleanupInActiveRepo();
    if (id === "quitAndStopServer") return this.serverRestart.askQuit();
    if (id === "projectTeardown") return this.teardownInActiveSpace();
    if (id === "renameSpace") {
      const paneId = this.activePaneId();
      const spaceId = paneId ? this.pane(paneId)?.space_id : undefined;
      if (spaceId) this.spaceActions.askRename(spaceId);
      return;
    }
    if (id === "pinSpace" || id === "unpinSpace") {
      const paneId = this.activePaneId();
      const spaceId = paneId ? this.pane(paneId)?.space_id : undefined;
      if (spaceId) this.spaceActions.setPinned(spaceId, id === "pinSpace");
      return;
    }
    if (id === "moveSpaceUp" || id === "moveSpaceDown") {
      const paneId = this.activePaneId();
      const spaceId = paneId ? this.pane(paneId)?.space_id : undefined;
      if (spaceId) this.spaceActions.shift(spaceId, id === "moveSpaceUp" ? -1 : 1);
      return;
    }
    const sorts = {
      sortSpacesManual: "manual",
      sortSpacesByName: "name",
      sortSpacesByAttention: "attention",
      sortSpacesRecent: "recent",
    } as const;
    if (id in sorts) {
      void this.prefs.setSidebarSort(sorts[id as keyof typeof sorts]);
      return;
    }
    if (id === "sortSpacesNow") {
      this.spaceActions.sortNow();
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
      case "jump":
        {
          const id = this.editor.activeId();
          this.jump.toggle(
            id === null ? null : (spaceOfTab(id) ?? this.pane(id)?.space_id ?? null),
          );
        }
        break;
      case "showShortcuts":
        this.shortcutsOverview.open();
        break;
      case "find":
        this.terminalSearch.open();
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
    const dir = this.activeDir();
    if (dir) this.editor.openChanges(this.activeRepo() ?? dir);
    else this.spaceActions.notify("Show Changes: there is no focused space");
  }

  /** The directory of the active pane. */
  private activeDir(): string | undefined {
    const id = this.activePaneId();
    const pane = id ? this.pane(id) : undefined;
    return pane ? paneDir(pane) : undefined;
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
    const dir = this.activeDir();
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
    const dir = this.activeDir();
    return dir
      ? [...this.store.repos().keys()]
          .filter((r) => dir === r || dir.startsWith(r.endsWith("/") ? r : `${r}/`))
          .sort((a, b) => b.length - a.length)[0]
      : undefined;
  }

  /** Context menu of a sidebar row: new terminal in a workspace, split or close a pane. */
  protected onSidebarMenu(event: SidebarMenuAction): void {
    if (event.action === "new-terminal") this.paneActions.askNew(event.id);
    else if (event.action === "resume-session" && event.kind === "workspace")
      void this.sessionResume.open({ kind: "space", spaceId: event.id });
    else if (event.action === "close-space") this.closeSpace(event.id);
    else if (event.action === "pin-space" || event.action === "unpin-space")
      this.spaceActions.setPinned(event.id, event.action === "pin-space");
    else if (event.action === "rename-space") this.spaceActions.askRename(event.id);
    else if (event.action === "open-in-editor") this.externalEditor.openSpace(event.id);
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

  /** A space closed in herdr: all its panes go, so their tabs close without a successor. */
  protected onSpaceClosed(paneIds: string[]): void {
    for (const id of paneIds) this.editor.closePane(id);
  }

  private hostNote(pane: Terminal | undefined): string | undefined {
    const host = olderHost(pane);
    return host ? olderHostNote(host) : undefined;
  }

  protected paneAction(paneId: string, action: string): void {
    if (action === "mark-unread") {
      this.attention.markUnread(paneId);
    } else if (action === "split-right" || action === "split-down") {
      const direction = action === "split-right" ? "right" : "down";
      void this.paneActions.split(paneId, direction).then((pane) => pane && this.open(pane, true));
    } else if (action === "start-agent") {
      this.paneActions.askStartAgent(paneId);
    } else if (action === "resume-session") {
      void this.sessionResume.open({ kind: "pane", paneId });
    } else if (action === "rename") {
      this.paneActions.askRename(paneId);
    } else if (action === "save-layout") {
      this.paneActions.askSaveLayout(paneId);
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
    return tabMenuItems(group.tabs.length < 2, {
      shortcut: (action) => this.keymap.label(action),
      pane: (id) => this.pane(id),
      unread: () => this.attention.unread(),
    });
  }

  protected onTabMenu(id: string, action: string): void {
    if (action === "split-right") this.editor.split(id, "row");
    else if (action === "split-down") this.editor.split(id, "column");
    else if (action === "close") this.editor.close(id);
    else if (action === "open-flow") this.editor.openFlow(id);
    else if (action.startsWith("pane-") && tabKind(id) === "pane")
      this.paneAction(id, action.slice("pane-".length));
  }

  /** The panes a group keeps mounted: its active one and the recently hidden (#978). */
  protected viewIds(group: EditorGroup): readonly string[] {
    return this.hiddenViews.of(group.id);
  }

  /** The tab of a pane when it is split (more than one leaf), else `undefined`. */
  protected paneLayout(id: string): Tab | undefined {
    const tab = this.store.snapshot().tabs.find((t) => t.tab_id === this.pane(id)?.tab_id);
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

  /** The shell resized or collapsed the sidebar: persist it through `UiState`. */
  protected onSidebarChange(state: HxSidebarState): void {
    if (state.width !== this.ui.sidebarWidth()) this.ui.setSidebarWidth(state.width);
    if (state.collapsed !== this.ui.sidebarCollapsed()) this.ui.toggleSidebar();
  }

  /** Single click previews, a double click pins; an open pane is only activated. */
  /** Opens the tab of a pane by id; a pane herdr no longer has is ignored. */
  protected openPaneById(paneId: string): void {
    const pane = this.pane(paneId);
    if (pane) this.open(pane);
  }

  protected open(pane: Terminal, pin = false): void {
    this.openIn(pane, pin, new Set());
  }

  /**
   * Opens `pane` in the editor group its layout tab names (#657). The pane the placement is
   * relative to is opened first, so "claude left, Terminal right" works from any tab.
   */
  private openIn(pane: Terminal, pin: boolean, seen: Set<string>): void {
    this.known.set(pane.terminal_id, pane);
    seen.add(pane.terminal_id);
    const snapshot = this.store.all();
    const tab = snapshot.tabs.find((t) => t.tab_id === pane.tab_id);
    const at = openAtFor(snapshot, tab);
    if (!at || this.editor.groupOf(pane.terminal_id)) {
      this.editor.open(pane.terminal_id, pin);
      return;
    }
    const anchor = at.place?.ofPaneId ?? at.besidePaneId;
    const anchorPane = anchor ? this.pane(anchor) : undefined;
    if (anchor && anchorPane && !this.editor.groupOf(anchor) && !seen.has(anchor)) {
      this.openIn(anchorPane, true, seen);
    }
    this.editor.openAt(pane.terminal_id, at, pin);
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

  /** A subagent node of a flow tab was clicked: show its transcript in the Output tab. */
  protected openAgentOutput(agentId: string): void {
    this.subagents.pick(agentId);
    this.ui.selectPanelTab("output");
  }

  /** The directory of the group's active tab when that is a changes tab. */
  protected changesOf(group: EditorGroup): string | null {
    return group.activeId === null ? null : changesDir(group.activeId);
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

  /** A jump palette entry: focus its pane, else open the overview of its space. */
  protected jumpTo(entry: JumpEntry): void {
    if (entry.pane) {
      this.open(entry.pane);
      this.focusLeaf(entry.pane.tab_id, entry.pane.terminal_id);
    } else this.editor.openSpace(entry.spaceId);
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

  protected pane(id: string): Terminal | undefined {
    return this.store.panes().get(id) ?? this.known.get(id);
  }

  /** The OS appearance; light when it cannot be read. */
  private readonly systemDark = signal(false);
}
