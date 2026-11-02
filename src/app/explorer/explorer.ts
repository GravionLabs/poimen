import {
  ChangeDetectionStrategy,
  Component,
  computed,
  effect,
  inject,
  signal,
  untracked,
} from "@angular/core";
import { HxTree, HxTreeNodeTemplate } from "@gravionlabs/helix-ui";
import type { HxTreeNode, HxTreeNodeEvent } from "@gravionlabs/helix-ui";
import { HerdrStore } from "../core/herdr-store";
import { SpaceActions } from "../core/space-actions";
import { SettingsStore } from "../core/settings";
import { TauriBridge } from "../core/tauri";
import { UiState } from "../core/ui-state";

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

/** The open context menu: the folder, where it opened, and the root the folder lies under. */
export interface ExplorerMenu {
  path: string;
  x: number;
  y: number;
  /** Index of the root in `explorerRoots`. */
  root: number;
  /** Whether the folder is that root itself. */
  isRoot: boolean;
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
  private readonly store = inject(HerdrStore);
  private readonly spaceActions = inject(SpaceActions);
  private readonly children = signal<ReadonlyMap<string, readonly DirEntry[]>>(new Map());
  private readonly requested = new Set<string>();
  /** Index in `explorerRoots` of the root each listed folder lies under. */
  private readonly rootOf = new Map<string, number>();

  private readonly roots = computed(() => this.settings.settings().explorerRoots);
  private readonly hidden = computed(() => this.settings.settings().explorerShowHidden);

  protected readonly nodes = computed(() =>
    explorerNodes(this.roots(), this.children(), this.ui.expandedDirs()),
  );

  /** Directories that already have a space. */
  protected readonly withSpace = computed(() => spaceDirs(this.store.snapshot().panes));
  protected readonly menuAt = signal<ExplorerMenu | null>(null);

  protected openMenu(event: MouseEvent, path: string): void {
    event.preventDefault();
    event.stopPropagation();
    const roots = this.roots();
    const root = this.rootOf.get(path) ?? roots.indexOf(path);
    if (root < 0) return;
    const isRoot = roots[root] === path;
    this.menuAt.set({ path, x: event.clientX, y: event.clientY, root, isRoot });
  }

  /** Makes the folder the root it lies under; config.json keeps it for the next session. */
  protected setAsRoot(menu: ExplorerMenu): void {
    this.closeMenu();
    void this.settings.setExplorerRoot(menu.root, menu.path);
  }

  /** Moves a root up to its parent folder. */
  protected showParent(menu: ExplorerMenu): void {
    this.closeMenu();
    void this.settings.setExplorerRoot(menu.root, `${menu.path.replace(/\/+$/, "")}/..`);
  }

  protected closeMenu(): void {
    this.menuAt.set(null);
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
        this.rootOf.clear();
        roots.forEach((root, i) => {
          this.rootOf.set(root, i);
          void this.load(root, hidden);
        });
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
    const root = this.rootOf.get(path);
    if (root !== undefined) for (const d of dirs) this.rootOf.set(d.path, root);
    this.children.update((m) => new Map(m).set(path, dirs));
    // Remembered folders are keyed by absolute path, so follow the loaded tree instead of
    // matching a root prefix (a `~` root never prefixes them).
    const expanded = this.ui.expandedDirs();
    for (const d of dirs) if (expanded.has(d.path)) void this.load(d.path, hidden);
  }
}
