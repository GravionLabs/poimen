import {
  ChangeDetectionStrategy,
  Component,
  computed,
  effect,
  inject,
  signal,
  untracked,
} from "@angular/core";
import { HxTree } from "@gravionlabs/helix-ui";
import type { HxTreeNode, HxTreeNodeEvent } from "@gravionlabs/helix-ui";
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

/** Directory tree of the configured roots, directories only, loaded when a folder expands. */
@Component({
  selector: "app-explorer",
  imports: [HxTree],
  templateUrl: "./explorer.html",
  styleUrl: "./explorer.css",
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class Explorer {
  private readonly bridge = inject(TauriBridge);
  private readonly ui = inject(UiState);
  private readonly settings = inject(SettingsStore);
  private readonly children = signal<ReadonlyMap<string, readonly DirEntry[]>>(new Map());
  private readonly requested = new Set<string>();

  private readonly roots = computed(() => this.settings.settings().explorerRoots);
  private readonly hidden = computed(() => this.settings.settings().explorerShowHidden);

  protected readonly nodes = computed(() =>
    explorerNodes(this.roots(), this.children(), this.ui.expandedDirs()),
  );

  constructor() {
    // Roots or the hidden switch changed: start over and reload the roots and the remembered folders.
    effect(() => {
      const roots = this.roots();
      const hidden = this.hidden();
      untracked(() => {
        this.children.set(new Map());
        this.requested.clear();
        for (const root of roots) void this.load(root, hidden);
        for (const dir of this.ui.expandedDirs()) {
          if (roots.some((r) => dir.startsWith(r === "/" ? "/" : r + "/"))) {
            void this.load(dir, hidden);
          }
        }
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
  }
}
