import { Injectable, computed, inject, signal } from "@angular/core";
import { AgentActions } from "../github/agent-actions";
import { fuzzyRank } from "./fuzzy";
import { Keymap } from "./keymap";
import { ProjectEntries, type ProjectEntry } from "./project-entries";
import type { ShortcutAction } from "./shortcuts";
import { UiState } from "./ui-state";

/** A poimen command the palette offers; `id` is what the app runs (`App.run`). */
export interface PoimenCommand {
  id:
    | ShortcutAction
    | "openSettings"
    | "openGithub"
    | "githubInbox"
    | "githubPulls"
    | "githubIssues"
    | "githubMilestones"
    | "openDeliver"
    | "showChanges"
    | "showSpaceOverview"
    | "closeSpace"
    | "renameSpace"
    | "moveSpaceUp"
    | "moveSpaceDown"
    | "startTask"
    | "openTerminalHere"
    | "cleanupTasks"
    | "projectTeardown"
    | "quitAndStopServer"
    | "projectEntry"
    | "agentAction";
  title: string;
}

export const POIMEN_COMMANDS: readonly PoimenCommand[] = [
  { id: "newTerminal", title: "New Terminal..." },
  { id: "renameSpace", title: "Rename Space..." },
  { id: "moveSpaceUp", title: "Move Space Up" },
  { id: "moveSpaceDown", title: "Move Space Down" },
  { id: "closeSpace", title: "Close Space" },
  { id: "closeTab", title: "Close Tab" },
  { id: "closeAll", title: "Close All Tabs" },
  { id: "refresh", title: "Refresh" },
  { id: "toggleSidebar", title: "Toggle Sidebar" },
  { id: "splitRight", title: "Split Right" },
  { id: "splitDown", title: "Split Down" },
  { id: "zoomIn", title: "Zoom In" },
  { id: "zoomOut", title: "Zoom Out" },
  { id: "zoomReset", title: "Reset Zoom" },
  { id: "nextWaiting", title: "Next Waiting Agent" },
  { id: "toggleOutput", title: "Toggle Output Panel" },
  { id: "nextTab", title: "Next Tab" },
  { id: "previousTab", title: "Previous Tab" },
  { id: "showShortcuts", title: "Keyboard Shortcuts" },
  { id: "find", title: "Terminal: Find" },
  { id: "openSettings", title: "Open Settings" },
  { id: "openGithub", title: "Open GitHub" },
  { id: "githubInbox", title: "GitHub: Inbox" },
  { id: "githubPulls", title: "GitHub: Pull requests" },
  { id: "githubIssues", title: "GitHub: Issues" },
  { id: "githubMilestones", title: "GitHub: Milestones" },
  { id: "openDeliver", title: "Open Deliver" },
  { id: "showChanges", title: "Show Changes" },
  { id: "showSpaceOverview", title: "Show Space Overview" },
  { id: "startTask", title: "Start task from issue…" },
  { id: "openTerminalHere", title: "Open terminal in the selected folder" },
  { id: "cleanupTasks", title: "Clean up merged tasks" },
  { id: "projectTeardown", title: "Project: Run teardown" },
  { id: "quitAndStopServer", title: "Quit poimen and stop server" },
];

/** An entry of the palette: a poimen command; `shortcut` is the key binding label (empty when none). */
export interface PaletteItem {
  key: string;
  title: string;
  shortcut: string;
  command: PoimenCommand["id"];
  /** A command or preset of the project file (`command` is `projectEntry`). */
  entry?: ProjectEntry;
  /** An agent action of the GitHub view (`command` is `agentAction`). */
  agentActionId?: string;
  /** The project file is not trusted: running the entry asks first. */
  locked?: boolean;
}

/** A line of the list: a group heading (not selectable) or a selectable entry. */
export type PaletteRow =
  | { type: "heading"; key: string; label: string }
  | {
      type: "item";
      key: string;
      item: PaletteItem;
      /** Index among the selectable entries (what `selected` refers to). */
      index: number;
      /** Matched character indexes in `item.title`. */
      positions: readonly number[];
    };

/** The group headings of the list without a query. */
export const POIMEN_GROUP = "Poimen";
export const RECENT_GROUP = "Recent";
export const PROJECT_GROUP = "Project";
export const GITHUB_GROUP = "GitHub";

/**
 * The command palette: the poimen commands, fuzzy filtered.
 */
@Injectable({ providedIn: "root" })
export class CommandPalette {
  private readonly keymap = inject(Keymap);
  private readonly ui = inject(UiState);
  private readonly project = inject(ProjectEntries);
  private readonly agentActions = inject(AgentActions);

  private readonly _open = signal(false);
  private readonly _query = signal("");
  private readonly _selected = signal(0);

  readonly isOpen = this._open.asReadonly();
  readonly query = this._query.asReadonly();

  private readonly all = computed<PaletteItem[]>(() => [
    ...POIMEN_COMMANDS.map((c): PaletteItem => ({
      key: `command:${c.id}`,
      title: c.title,
      shortcut:
        c.id === "openSettings" ||
        c.id === "openGithub" ||
        c.id === "githubInbox" ||
        c.id === "githubPulls" ||
        c.id === "githubIssues" ||
        c.id === "githubMilestones" ||
        c.id === "openDeliver" ||
        c.id === "showChanges" ||
        c.id === "showSpaceOverview" ||
        c.id === "closeSpace" ||
        c.id === "renameSpace" ||
        c.id === "moveSpaceUp" ||
        c.id === "moveSpaceDown" ||
        c.id === "startTask" ||
        c.id === "openTerminalHere" ||
        c.id === "cleanupTasks" ||
        c.id === "projectTeardown" ||
        c.id === "quitAndStopServer" ||
        c.id === "projectEntry" ||
        c.id === "agentAction"
          ? ""
          : this.keymap.label(c.id),
      command: c.id,
    })),
    ...this.projectItems(),
    ...this.agentActions.entries().map((e): PaletteItem => ({
      key: `agentAction:${e.action.id}`,
      title: e.title,
      shortcut: "",
      command: "agentAction",
      agentActionId: e.action.id,
    })),
  ]);

  private projectItems(): PaletteItem[] {
    const { commands, presets, locked } = this.project.entries();
    const item = (entry: ProjectEntry, title: string): PaletteItem => ({
      key: `project:${entry.kind}:${entry.name}`,
      title,
      shortcut: "",
      command: "projectEntry",
      entry,
      locked,
    });
    return [
      ...commands.map((c) => item({ kind: "command", name: c.name }, `Project: ${c.name}`)),
      ...presets.map((name) => item({ kind: "preset", name }, `Preset: ${name}`)),
    ];
  }

  private readonly ranked = computed(() => fuzzyRank(this.all(), this._query(), (i) => i.title));

  /** The lines of the list. With an empty query: Recent, then Poimen. */
  readonly rows = computed<PaletteRow[]>(() => {
    const ranked = this.ranked();
    const rows: PaletteRow[] = [];
    let index = 0;
    const push = (item: PaletteItem, positions: readonly number[]): void => {
      rows.push({ type: "item", key: item.key, item, index: index++, positions });
    };
    if (this._query().replace(/\s+/g, "") !== "") {
      for (const { item, positions } of ranked) {
        push(
          item,
          positions.filter((p) => p < item.title.length),
        );
      }
      return rows;
    }
    const items = ranked.map((r) => r.item);
    const recent = this.ui.recentCommands().flatMap((key) => {
      const item = items.find((i) => i.key === key);
      return item ? [item] : [];
    });
    const rest = items.filter((i) => !recent.includes(i));
    const groups: [string, PaletteItem[]][] = [
      [RECENT_GROUP, recent],
      [POIMEN_GROUP, rest.filter((i) => !i.entry && !i.agentActionId)],
      [GITHUB_GROUP, rest.filter((i) => i.agentActionId)],
      [PROJECT_GROUP, rest.filter((i) => i.entry)],
    ];
    for (const [label, members] of groups) {
      if (members.length === 0) continue;
      rows.push({ type: "heading", key: `heading:${label}`, label });
      for (const item of members) push(item, []);
    }
    return rows;
  });

  /** The selectable entries in display order (headings left out). */
  readonly items = computed(() => this.rows().flatMap((r) => (r.type === "item" ? [r.item] : [])));

  /** Index of the highlighted row, kept inside the list. */
  readonly selected = computed(() =>
    Math.max(0, Math.min(this._selected(), this.items().length - 1)),
  );

  /** Opens the palette with an empty query. */
  open(): void {
    void this.project.reload();
    void this.agentActions.reload();
    this._query.set("");
    this._selected.set(0);
    this._open.set(true);
  }

  close(): void {
    this._open.set(false);
  }

  toggle(): void {
    if (this._open()) this.close();
    else this.open();
  }

  setQuery(query: string): void {
    this._query.set(query);
    this._selected.set(0);
  }

  /** Highlights row `index` (mouse hover). */
  hover(index: number): void {
    this._selected.set(index);
  }

  /** Moves the highlight by `delta`, wrapping around. */
  move(delta: number): void {
    const n = this.items().length;
    if (n > 0) this._selected.set((this.selected() + delta + n) % n);
  }

  /** The highlighted row, if any. */
  current(): PaletteItem | undefined {
    return this.items()[this.selected()];
  }

  /** Remembers `item` as the most recently run entry. */
  recordRun(item: PaletteItem): void {
    this.ui.addRecentCommand(item.key);
  }
}
