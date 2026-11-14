import {
  ChangeDetectionStrategy,
  Component,
  computed,
  effect,
  inject,
  signal,
  untracked,
  ElementRef,
  HostListener,
} from "@angular/core";
import { HxTree, HxTreeNodeTemplate } from "@gravionlabs/helix-ui";
import type { HxTreeNode, HxTreeNodeEvent } from "@gravionlabs/helix-ui";
import { EditorTabs } from "../core/editor-tabs";
import { ExplorerSelection } from "../core/explorer-selection";
import { PaneActions } from "../core/pane-actions";
import { SpacesStore } from "../core/spaces-store";
import { SpaceActions } from "../core/space-actions";
import { SettingsStore } from "../core/settings";
import { TaskStarter } from "../core/task-start";
import { TauriBridge } from "../core/tauri";
import { UiState } from "../core/ui-state";
import { GitSync } from "./git-sync";

/** A directory as the backend lists it (`list_child_dirs`). */
export interface DirEntry {
  name: string;
  path: string;
}

/**
 * Tree nodes of the explorer: the roots (labeled with their path) over the loaded children.
 * A directory whose children are not loaded yet keeps its toggle; a loaded empty one is a leaf.
 */
export function explorerNodes(
  roots: readonly string[],
  children: ReadonlyMap<string, readonly DirEntry[]>,
  expanded: ReadonlySet<string>,
): HxTreeNode[] {
  const build = (path: string, label: string): HxTreeNode => {
    const loaded = children.get(path);
    return {
      key: path,
      label,
      expanded: expanded.has(path),
      leaf: loaded !== undefined && loaded.length === 0,
      children: loaded?.map((c) => build(c.path, c.name)) ?? [],
    };
  };
  return roots.map((r) => build(r, r));
}

/**
 * Keeps the nodes whose label contains `query` (case-insensitive) and their ancestors; the
 * ancestors of a match are expanded and what does not match is dropped. An empty query keeps all.
 */
export function filterNodes(nodes: readonly HxTreeNode[], query: string): HxTreeNode[] {
  const q = query.trim().toLowerCase();
  if (q === "") return [...nodes];
  const keep = (node: HxTreeNode): HxTreeNode | null => {
    const kids = (node.children ?? []).map(keep).filter((c): c is HxTreeNode => c !== null);
    if (kids.length > 0) return { ...node, expanded: true, leaf: false, children: kids };
    if (node.label.toLowerCase().includes(q))
      return { ...node, expanded: false, leaf: true, children: [] };
    return null;
  };
  return nodes.map(keep).filter((n): n is HxTreeNode => n !== null);
}

/** Splits `label` into parts around the case-insensitive matches of `query`. */
export function labelParts(label: string, query: string): { text: string; hit: boolean }[] {
  const q = query.trim().toLowerCase();
  if (q === "") return [{ text: label, hit: false }];
  const parts: { text: string; hit: boolean }[] = [];
  const lower = label.toLowerCase();
  let at = 0;
  for (let i = lower.indexOf(q); i >= 0; i = lower.indexOf(q, at)) {
    if (i > at) parts.push({ text: label.slice(at, i), hit: false });
    parts.push({ text: label.slice(i, i + q.length), hit: true });
    at = i + q.length;
  }
  if (at < label.length) parts.push({ text: label.slice(at), hit: false });
  return parts;
}

/** Directories (trailing slashes ignored) that are the working directory of a pane of some space. */
export function spaceDirs(
  panes: readonly { cwd?: string; foreground_cwd?: string }[],
): Set<string> {
  const dirs = new Set<string>();
  for (const p of panes) {
    for (const d of [p.cwd, p.foreground_cwd]) if (d) dirs.add(d.replace(/(.)\/+$/, "$1"));
  }
  return dirs;
}

/** Directory tree of the configured roots, directories only, loaded when a folder expands. */
@Component({
  selector: "app-explorer",
  imports: [HxTree, HxTreeNodeTemplate],
  templateUrl: "./explorer.html",
  styleUrl: "./explorer.css",
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class Explorer {
  private readonly bridge = inject(TauriBridge);
  private readonly ui = inject(UiState);
  private readonly settings = inject(SettingsStore);
  private readonly store = inject(SpacesStore);
  private readonly spaceActions = inject(SpaceActions);
  private readonly tasks = inject(TaskStarter);
  private readonly editor = inject(EditorTabs);
  private readonly panes = inject(PaneActions);
  private readonly selection = inject(ExplorerSelection);
  private readonly sync = inject(GitSync);
  private readonly children = signal<ReadonlyMap<string, readonly DirEntry[]>>(new Map());
  private readonly requested = new Set<string>();

  private readonly roots = computed(() => this.settings.settings().explorerRoots);
  private readonly hidden = computed(() => this.settings.settings().explorerShowHidden);

  /** The filter text; narrows the loaded directories by name without touching `expandedDirs`. */
  protected readonly query = signal("");
  private readonly host = inject<ElementRef<HTMLElement>>(ElementRef);

  protected readonly nodes = computed(() =>
    filterNodes(explorerNodes(this.roots(), this.children(), this.ui.expandedDirs()), this.query()),
  );

  protected parts(label: string): { text: string; hit: boolean }[] {
    return labelParts(label, this.query());
  }

  protected setQuery(event: Event): void {
    this.query.set((event.target as HTMLInputElement).value);
  }

  /** Cmd/Ctrl+F in the explorer moves to the filter field. */
  @HostListener("keydown", ["$event"])
  protected onViewKey(event: KeyboardEvent): void {
    if ((event.metaKey || event.ctrlKey) && !event.altKey && event.key.toLowerCase() === "f") {
      const filter = this.host.nativeElement.querySelector<HTMLInputElement>("input.filter");
      event.preventDefault();
      filter?.focus();
      filter?.select();
    }
  }

  /** Escape in the filter field clears it. */
  protected filterKey(event: KeyboardEvent): void {
    if (event.key === "Escape" && this.query() !== "") {
      event.stopPropagation();
      this.query.set("");
      (event.target as HTMLInputElement).value = "";
    }
  }

  /** Directories that already have a space. */
  protected readonly withSpace = computed(() => spaceDirs(this.store.snapshot().terminals));
  protected readonly menuAt = signal<{ path: string; x: number; y: number } | null>(null);

  /** The directory of the open menu is a repository: it offers to start a task. */
  protected readonly menuRepo = signal<boolean | null>(null);

  protected select(path: string): void {
    this.selection.select(path);
  }

  /** Opens a terminal in `path`: in the space containing it, else in a new space. */
  protected openTerminal(path: string): void {
    this.closeMenu();
    this.selection.select(path);
    void this.panes
      .openTerminalIn(path)
      .then((pane) => pane && this.editor.open(pane.terminal_id, true));
  }

  protected openMenu(event: MouseEvent, path: string): void {
    this.selection.select(path);
    event.preventDefault();
    event.stopPropagation();
    this.menuRepo.set(null);
    this.menuAt.set({ path, x: event.clientX, y: event.clientY });
    void this.checkRepo(path);
  }

  private async checkRepo(path: string): Promise<void> {
    try {
      const found = await this.bridge.invoke<Record<string, unknown> | undefined>("repo_info", {
        dirs: [path],
      });
      if (this.menuAt()?.path === path) this.menuRepo.set(!!found && path in found);
    } catch {
      // no entry: the menu stays without the task entry
    }
  }

  protected closeMenu(): void {
    this.menuAt.set(null);
  }

  /** Starts a task for the repository at `path`. */
  protected startTask(path: string): void {
    this.closeMenu();
    void this.tasks.start(path);
  }

  /** Opens the Changes tab of the repository at `path`. */
  protected showChanges(path: string): void {
    this.closeMenu();
    this.editor.openChanges(path);
  }

  protected fetchRepo(path: string): void {
    this.closeMenu();
    void this.sync.fetch(path);
  }

  protected pullRepo(path: string): void {
    this.closeMenu();
    void this.sync.pull(path);
  }

  protected fetchAll(path: string): void {
    this.closeMenu();
    void this.sync.fetchAll(path);
  }

  /** Creates the space of a directory, or focuses the one it already has. */
  protected createSpace(path: string): void {
    this.closeMenu();
    void this.spaceActions.create(path, "");
  }

  constructor() {
    // Roots or the hidden switch changed: start over and reload the roots; remembered folders reload as their parents load.
    effect(() => {
      const roots = this.roots();
      const hidden = this.hidden();
      untracked(() => {
        this.children.set(new Map());
        this.requested.clear();
        for (const root of roots) void this.load(root, hidden);
      });
    });
  }

  protected onExpand(event: HxTreeNodeEvent): void {
    this.ui.setDirExpanded(event.node.key, true);
    void this.load(event.node.key, this.hidden());
  }

  protected onCollapse(event: HxTreeNodeEvent): void {
    this.ui.setDirExpanded(event.node.key, false);
  }

  /** Lists `path` once; an unreadable directory (or a failed call) shows no children. */
  private async load(path: string, hidden: boolean): Promise<void> {
    if (this.requested.has(path)) return;
    this.requested.add(path);
    let dirs: DirEntry[];
    try {
      dirs = await this.bridge.invoke<DirEntry[]>("list_child_dirs", { path, hidden });
    } catch {
      dirs = [];
    }
    if (hidden !== this.hidden()) return;
    this.children.update((m) => new Map(m).set(path, dirs));
    // Remembered folders are keyed by absolute path, so follow the loaded tree instead of
    // matching a root prefix (a `~` root never prefixes them).
    const expanded = this.ui.expandedDirs();
    for (const d of dirs) if (expanded.has(d.path)) void this.load(d.path, hidden);
  }
}
