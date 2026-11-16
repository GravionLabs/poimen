import { NgTemplateOutlet } from "@angular/common";
import {
  ChangeDetectionStrategy,
  Component,
  EventEmitter,
  Input,
  Output,
  computed,
  signal,
} from "@angular/core";
import {
  MAX_RATIO,
  MIN_RATIO,
  addTab,
  canIndent,
  canOutdent,
  editNode,
  editorGroups,
  flatTabs,
  indentTab,
  leaves,
  moveTab,
  outdentTab,
  removeNode,
  removeTab,
  renameTab,
  spacePreview,
  splitNode,
  updateTab,
  type GroupNode,
  type LayoutErrors,
  type LayoutNode,
  type LayoutTab,
  type NodePath,
  type TabPath,
} from "../core/named-layouts-edit";
import type { SplitDirection } from "../core/space-layout";

function value(event: Event): string {
  return (event.target as HTMLInputElement | HTMLSelectElement).value;
}

/** The ratio of a number input, clamped to what a split can show. */
function ratioOf(event: Event): number {
  const n = Number((event.target as HTMLInputElement).value);
  return Number.isFinite(n) ? Math.min(MAX_RATIO, Math.max(MIN_RATIO, n)) : 0.5;
}

/**
 * The tab editor of a layout (tabs with nested child tabs, per tab one terminal or a split tree,
 * the place in the editor groups, and the preview). Shared by the Layouts page and the space
 * overview, which edits the project file's `layout` and presets with it. Every change is emitted
 * as the new list of tabs; the host keeps the state.
 */
@Component({
  selector: "app-layout-tabs-editor",
  imports: [NgTemplateOutlet],
  templateUrl: "./layout-tabs-editor.html",
  styleUrls: ["./settings-view.css", "./layouts-editor.css"],
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class LayoutTabsEditor {
  // Decorator inputs: the specs run without the compiler transform of signal inputs.
  protected readonly tabs = signal<LayoutTab[]>([]);
  @Input({ required: true }) set tabList(v: LayoutTab[]) {
    this.tabs.set(v);
  }
  protected readonly errs = signal<LayoutErrors | null>(null);
  @Input() set errors(v: LayoutErrors | null) {
    this.errs.set(v);
  }
  @Output() readonly tabsChange = new EventEmitter<LayoutTab[]>();

  protected readonly flat = computed(() => flatTabs(this.tabs()));
  protected readonly preview = computed(() => spacePreview(this.tabs()));
  protected readonly groups = computed(() => editorGroups(this.tabs()));
  protected readonly ratioMin = MIN_RATIO;
  protected readonly ratioMax = MAX_RATIO;

  protected canIndent = canIndent;
  protected canOutdent = canOutdent;
  protected readonly ratioOf = ratioOf;
  protected readonly value = value;

  /** Where `of` of a tab can point: every other tab of the layout. */
  protected placeTargets(index: number): string[] {
    return this.flat()
      .filter((_, i) => i !== index)
      .map((t) => t.tab.name.trim())
      .filter(Boolean);
  }

  protected leafCount(n: LayoutNode): number {
    return leaves(n).length;
  }

  protected groupStyle(g: GroupNode): string {
    if (g.kind === "group") return "";
    return `flex-direction: ${g.direction === "right" ? "row" : "column"}`;
  }

  protected flexOf(g: GroupNode, child: 0 | 1): string {
    if (g.kind === "group") return "1";
    return String(child === 0 ? g.ratio : 1 - g.ratio);
  }

  private change(edit: (tabs: LayoutTab[]) => LayoutTab[]): void {
    this.tabsChange.emit(edit(this.tabs()));
  }

  private tabAtPath(path: TabPath): LayoutTab | undefined {
    return this.flat().find((t) => t.path.join() === path.join())?.tab;
  }

  protected addTab(parent: TabPath | null): void {
    this.change((tabs) => addTab(tabs, parent));
  }

  protected removeTab(path: TabPath): void {
    this.change((tabs) => removeTab(tabs, path));
  }

  protected moveTab(path: TabPath, delta: -1 | 1): void {
    this.change((tabs) => moveTab(tabs, path, delta));
  }

  protected indent(path: TabPath): void {
    this.change((tabs) => indentTab(tabs, path));
  }

  protected outdent(path: TabPath): void {
    this.change((tabs) => outdentTab(tabs, path));
  }

  protected setTabName(path: TabPath, event: Event): void {
    this.change((tabs) => renameTab(tabs, path, value(event)));
  }

  /** Switches a tab between one terminal and a split tree (a split of two terminals). */
  protected setShape(path: TabPath, shape: string): void {
    const tab = this.tabAtPath(path);
    if (!tab) return;
    const first = leaves(tab.root)[0];
    const root: LayoutNode =
      shape === "split" ? splitNode(first, [], "right") : { ...first, name: null };
    this.change((tabs) => updateTab(tabs, path, { root }));
  }

  private editRoot(path: TabPath, edit: (root: LayoutNode) => LayoutNode): void {
    const tab = this.tabAtPath(path);
    if (tab) this.change((tabs) => updateTab(tabs, path, { root: edit(tab.root) }));
  }

  protected setLeaf(
    path: TabPath,
    node: NodePath,
    field: "name" | "command" | "cwd",
    event: Event,
  ): void {
    const text = value(event);
    this.editRoot(path, (r) => editNode(r, node, { [field]: text === "" ? null : text }));
  }

  protected setSplit(
    path: TabPath,
    node: NodePath,
    patch: { direction?: SplitDirection; ratio?: number },
  ): void {
    this.editRoot(path, (r) => editNode(r, node, patch));
  }

  protected splitLeaf(path: TabPath, node: NodePath, direction: SplitDirection): void {
    this.editRoot(path, (r) => splitNode(r, node, direction));
  }

  protected removeLeaf(path: TabPath, node: NodePath): void {
    this.editRoot(path, (r) => removeNode(r, node));
  }

  /** Switches the `place` of a tab on or off; on, it splits the group of its parent to the right. */
  protected setPlaced(path: TabPath, on: boolean): void {
    this.change((tabs) =>
      updateTab(tabs, path, {
        place: on ? { direction: "right", of: null, ratio: 0.5 } : null,
      }),
    );
  }

  protected setPlace(
    path: TabPath,
    patch: { direction?: SplitDirection; of?: string | null; ratio?: number },
  ): void {
    const tab = this.tabAtPath(path);
    if (!tab?.place) return;
    const place = { ...tab.place, ...patch };
    this.change((tabs) => updateTab(tabs, path, { place }));
  }
}
