import {
  ChangeDetectionStrategy,
  Component,
  computed,
  inject,
  input,
  output,
  signal,
} from "@angular/core";
import { HxInput, HxMessage, HxTree, HxTreeNodeTemplate } from "@gravionlabs/helix-ui";
import type { HxTreeNode } from "@gravionlabs/helix-ui";
import {
  filterSpaces,
  shortPath,
  type Pane,
  type PaneRow,
  type SpaceNode,
  type Workspace,
} from "../core/herdr";
import { HerdrStore } from "../core/herdr-store";

/** What a tree node carries back to the template. */
export type SpaceData =
  { kind: "workspace"; workspace: Workspace } | { kind: "pane"; row: PaneRow };

/** Tree keys: the pane id for a pane, a prefixed workspace id for a workspace. */
const WORKSPACE_PREFIX = "workspace:";

/** Tree nodes of the (filtered) spaces; a workspace starts expanded (helix only applies it once). */
export function spaceNodes(spaces: SpaceNode[]): HxTreeNode<SpaceData>[] {
  return spaces.map((space) => ({
    key: WORKSPACE_PREFIX + space.workspace.workspace_id,
    label: space.workspace.label,
    expanded: true,
    data: { kind: "workspace", workspace: space.workspace },
    children: space.rows.map((row) => ({
      key: row.pane.pane_id,
      label: row.name,
      leaf: true,
      data: { kind: "pane", row },
    })),
  }));
}

/** Workspaces and their panes with live agent status; a click or Enter opens the pane. */
@Component({
  selector: "app-sidebar",
  imports: [HxMessage, HxTree, HxTreeNodeTemplate, HxInput],
  templateUrl: "./sidebar.html",
  styleUrl: "./sidebar.css",
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class Sidebar {
  /** Ids of the panes that have an open view. */
  readonly openIds = input<ReadonlySet<string>>(new Set());
  readonly selected = output<Pane>();

  protected readonly store = inject(HerdrStore);
  protected readonly query = signal("");
  protected readonly selection = signal<string | null>(null);
  protected readonly shortPath = shortPath;

  protected readonly nodes = computed(() =>
    spaceNodes(filterSpaces(this.store.spaces(), this.query())),
  );

  protected data(node: HxTreeNode): SpaceData {
    return node.data as SpaceData;
  }

  protected onSelect(key: string | readonly string[] | null): void {
    if (typeof key !== "string") return;
    const row = this.store
      .spaces()
      .flatMap((s) => s.rows)
      .find((r) => r.pane.pane_id === key);
    // Reset so the same row can be picked again.
    this.selection.set(null);
    if (row) this.selected.emit(row.pane);
  }

  protected setQuery(event: Event): void {
    this.query.set((event.target as HTMLInputElement).value);
  }

  protected rowTitle(row: PaneRow): string {
    return [row.pane.pane_id, row.dir, row.branch].filter(Boolean).join(" · ");
  }

  protected glyph(row: PaneRow): string {
    return row.agent ? row.agent.slice(0, 2) : ">_";
  }
}
