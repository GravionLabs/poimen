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
import { TaskStarter } from "../core/task-start";
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
  private readonly tasks = inject(TaskStarter);
  private readonly children = signal<ReadonlyMap<string, readonly DirEntry[]>>(new Map());
  private readonly requested = new Set<string>();

  private readonly roots = computed(() => this.settings.settings().explorerRoots);
  private readonly hidden = computed(() => this.settings.settings().explorerShowHidden);

  protected readonly nodes = computed(() =>
    explorerNodes(this.roots(), this.children(), this.ui.expandedDirs()),
  );

  /** Directories that already have a space. */
  protected readonly withSpace = computed(() => spaceDirs(this.store.snapshot().terminals));
  protected readonly menuAt = signal<{ path: string; x: number; y: number } | null>(null);

  /** The directory of the open menu is a repository: it offers to start a task. */
  protected readonly menuRepo = signal(false);

  protected openMenu(event: MouseEvent, path: string): void {
    event.preventDefault();
    event.stopPropagation();
    this.menuRepo.set(false);
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
