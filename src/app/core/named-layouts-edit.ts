/**
 * The edits of the Layouts page on `named-layouts.ts`: validation, the delete check, the pure tab
 * and split-tree edits and the previews. Only the lazy Layouts page imports it.
 */
import type { Snapshot, TabTree } from "./snapshot";
import {
  text,
  leaves,
  uniqueName,
  MAX_RATIO,
  MIN_RATIO,
  type LayoutNode,
  type LayoutTab,
  type NamedLayout,
  type NodePath,
  type TabPath,
} from "./named-layouts";
import type { SplitDirection } from "./space-layout";
import type { SpaceTemplate } from "./space-templates";

export type { LayoutNode, LayoutTab, NamedLayout, NodePath, TabPath };
export { MAX_RATIO, MIN_RATIO, leaves, uniqueName };

/** The tabs of the layout in document order (a tab, then its children), with their path. */
export function flatTabs(
  tabs: readonly LayoutTab[],
  prefix: TabPath = [],
): { tab: LayoutTab; path: TabPath; depth: number }[] {
  return tabs.flatMap((tab, i) => {
    const path = [...prefix, i];
    return [{ tab, path, depth: prefix.length }, ...flatTabs(tab.children, path)];
  });
}

/** The names of every tab of the layout, in document order. */
export function tabNames(tabs: readonly LayoutTab[]): string[] {
  return flatTabs(tabs).map((t) => t.tab.name.trim());
}

export interface LayoutErrors {
  name?: string;
  tabs?: string;
  /** Per tab, by its index in `flatTabs`. */
  tab: Record<number, string>;
}

function validRatio(r: number): boolean {
  return Number.isFinite(r) && r > 0 && r < 1;
}

function nodeError(n: LayoutNode): string | null {
  if (n.kind === "terminal") return null;
  if (!validRatio(n.ratio)) return "A split ratio is between 0 and 1";
  return nodeError(n.children[0]) ?? nodeError(n.children[1]);
}

/** Problems of the draft; `others` are the names of the other layouts. */
export function layoutErrors(l: NamedLayout, others: readonly string[]): LayoutErrors {
  const out: LayoutErrors = { tab: {} };
  const name = l.name.trim();
  if (!name) out.name = "A name is required";
  else if (others.includes(name)) out.name = `Another layout is called "${name}"`;
  if (l.tabs.length === 0) out.tabs = "Add a tab";
  const all = flatTabs(l.tabs);
  const names = all.map((t) => t.tab.name.trim());
  all.forEach(({ tab }, i) => {
    const n = tab.name.trim();
    if (!n) out.tab[i] = "A tab needs a name";
    else if (names.indexOf(n) !== i) out.tab[i] = `Another tab is called "${n}"`;
    else if (tab.place) {
      const of = tab.place.of?.trim();
      if (!validRatio(tab.place.ratio)) out.tab[i] = "The place ratio is between 0 and 1";
      else if (of && of === n) out.tab[i] = "A tab cannot be placed of itself";
      else if (of && !names.includes(of)) out.tab[i] = `"${of}" is not a tab of the layout`;
    }
    const tree = nodeError(tab.root);
    if (out.tab[i] === undefined && tree) out.tab[i] = tree;
  });
  // A place that leads back to its own tab is a cycle.
  all.forEach(({ tab }, i) => {
    if (out.tab[i]) return;
    let at = tab;
    for (let step = 0; step <= all.length; step++) {
      const of = at.place?.of?.trim();
      if (!of) return;
      const next = all.find((t) => t.tab.name.trim() === of)?.tab;
      if (!next) return;
      if (next === tab) {
        out.tab[i] = "The places form a cycle";
        return;
      }
      at = next;
    }
  });
  return out;
}

export function hasLayoutErrors(e: LayoutErrors): boolean {
  return Boolean(e.name) || Boolean(e.tabs) || Object.keys(e.tab).length > 0;
}

/** The templates that refer to the layout `name`, by their names (the delete and rename check). */
export function layoutUsers(name: string, templates: readonly SpaceTemplate[]): string[] {
  return templates.filter((t) => t.layout === name).map((t) => t.name);
}

/** The refusal text of deleting or renaming a layout a template uses; `null` when it is free. */
export function usedMessage(
  name: string,
  templates: readonly SpaceTemplate[],
  verb: "delete" | "rename",
): string | null {
  const users = layoutUsers(name, templates);
  if (users.length === 0) return null;
  return `Cannot ${verb} "${name}": used by ${users.length === 1 ? "template" : "templates"} ${users.join(", ")}.`;
}

export function emptyLayout(name: string): NamedLayout {
  return { name, tabs: [newTab("main", [])] };
}

export function newTab(base: string, taken: readonly string[]): LayoutTab {
  return {
    name: uniqueName(base, taken),
    root: { kind: "terminal", name: null, command: null, cwd: null },
    children: [],
    place: null,
  };
}

/** The same layout under another name. */
export function duplicateLayout(l: NamedLayout, name: string): NamedLayout {
  return structuredClone({ ...l, name });
}

function replaceAt(
  tabs: readonly LayoutTab[],
  path: TabPath,
  edit: (list: LayoutTab[], index: number) => LayoutTab[],
): LayoutTab[] {
  const [head, ...rest] = path;
  if (rest.length === 0) return edit([...tabs], head);
  return tabs.map((t, i) =>
    i === head ? { ...t, children: replaceAt(t.children, rest, edit) } : t,
  );
}

/** The tab at `path`, or `undefined`. */
export function tabAt(tabs: readonly LayoutTab[], path: TabPath): LayoutTab | undefined {
  let list = tabs;
  let found: LayoutTab | undefined;
  for (const i of path) {
    found = list[i];
    if (!found) return undefined;
    list = found.children;
  }
  return found;
}

/** A new tab at the end of the children of `parent` (top level for `null`). */
export function addTab(tabs: readonly LayoutTab[], parent: TabPath | null): LayoutTab[] {
  const tab = newTab("tab", tabNames(tabs));
  if (parent === null) return [...tabs, tab];
  const p = tabAt(tabs, parent);
  if (!p) return [...tabs];
  return replaceAt(tabs, parent, (list, i) => {
    list[i] = { ...p, children: [...p.children, tab] };
    return list;
  });
}

export function updateTab(
  tabs: readonly LayoutTab[],
  path: TabPath,
  patch: Partial<Omit<LayoutTab, "children">>,
): LayoutTab[] {
  return replaceAt(tabs, path, (list, i) => {
    if (list[i]) list[i] = { ...list[i], ...patch };
    return list;
  });
}

/** Removes the tab with its children. */
export function removeTab(tabs: readonly LayoutTab[], path: TabPath): LayoutTab[] {
  return replaceAt(tabs, path, (list, i) => {
    list.splice(i, 1);
    return list;
  });
}

/**
 * Renames the tab at `path` and every `place.of` that pointed at its old name, so the layout
 * stays valid.
 */
export function renameTab(tabs: readonly LayoutTab[], path: TabPath, name: string): LayoutTab[] {
  const old = tabAt(tabs, path)?.name;
  const renamed = updateTab(tabs, path, { name });
  if (!old) return renamed;
  const fix = (list: LayoutTab[]): LayoutTab[] =>
    list.map((t) => ({
      ...t,
      place: t.place && t.place.of === old ? { ...t.place, of: name } : t.place,
      children: fix(t.children),
    }));
  return fix(renamed);
}

/** Moves the tab one place up or down among its siblings; unchanged at the ends. */
export function moveTab(tabs: readonly LayoutTab[], path: TabPath, delta: -1 | 1): LayoutTab[] {
  return replaceAt(tabs, path, (list, i) => {
    const j = i + delta;
    if (j < 0 || j >= list.length) return list;
    [list[i], list[j]] = [list[j], list[i]];
    return list;
  });
}

/** Whether the tab can become a child of the sibling before it. */
export function canIndent(path: TabPath): boolean {
  return path[path.length - 1] > 0;
}

/** Makes the tab the last child of the sibling before it. */
export function indentTab(tabs: readonly LayoutTab[], path: TabPath): LayoutTab[] {
  if (!canIndent(path)) return [...tabs];
  const tab = tabAt(tabs, path);
  const before = tabAt(tabs, [...path.slice(0, -1), path[path.length - 1] - 1]);
  if (!tab || !before) return [...tabs];
  const without = removeTab(tabs, path);
  return replaceAt(without, [...path.slice(0, -1), path[path.length - 1] - 1], (list, i) => {
    list[i] = { ...before, children: [...before.children, tab] };
    return list;
  });
}

/** Whether the tab has a parent to leave. */
export function canOutdent(path: TabPath): boolean {
  return path.length > 1;
}

/** Makes the tab the sibling right after its parent. */
export function outdentTab(tabs: readonly LayoutTab[], path: TabPath): LayoutTab[] {
  if (!canOutdent(path)) return [...tabs];
  const tab = tabAt(tabs, path);
  if (!tab) return [...tabs];
  const without = removeTab(tabs, path);
  const parent = path.slice(0, -1);
  const grand = parent.slice(0, -1);
  if (grand.length === 0) {
    const out = [...without];
    out.splice(parent[0] + 1, 0, tab);
    return out;
  }
  return replaceAt(without, grand, (list, i) => {
    const g = list[i];
    const kids = [...g.children];
    kids.splice(parent[parent.length - 1] + 1, 0, tab);
    list[i] = { ...g, children: kids };
    return list;
  });
}

function nodeUpdate(
  n: LayoutNode,
  path: NodePath,
  edit: (n: LayoutNode) => LayoutNode | null,
): LayoutNode {
  if (path.length === 0) return edit(n) ?? n;
  if (n.kind !== "split") return n;
  const [head, ...rest] = path;
  const kids: [LayoutNode, LayoutNode] = [n.children[0], n.children[1]];
  kids[head] = nodeUpdate(kids[head], rest, edit);
  return { ...n, children: kids };
}

/** Edits the node at `path` of a split tree. */
export function editNode(
  root: LayoutNode,
  path: NodePath,
  patch: Partial<{
    name: string | null;
    command: string | null;
    cwd: string | null;
    ratio: number;
    direction: SplitDirection;
  }>,
): LayoutNode {
  return nodeUpdate(root, path, (n) => ({ ...n, ...patch }) as LayoutNode);
}

/** Splits the leaf at `path` into itself and a new leaf. */
export function splitNode(root: LayoutNode, path: NodePath, direction: SplitDirection): LayoutNode {
  return nodeUpdate(root, path, (n) =>
    n.kind === "terminal"
      ? {
          kind: "split",
          direction,
          ratio: 0.5,
          children: [n, { kind: "terminal", name: null, command: null, cwd: null }],
        }
      : null,
  );
}

/** Removes the leaf at `path`; its sibling takes the place of the split. The only leaf stays. */
export function removeNode(root: LayoutNode, path: NodePath): LayoutNode {
  if (path.length === 0) return root;
  const parent = path.slice(0, -1);
  const which = path[path.length - 1];
  return nodeUpdate(root, parent, (n) => (n.kind === "split" ? n.children[1 - which] : null));
}

/** A row of the spaces tree preview. */
export interface PreviewRow {
  name: string;
  depth: number;
  /** The terminals of the tab, "2 terminals" or the leaf's name. */
  detail: string;
}

/** The tabs of the layout as the rows the sidebar shows for a space. */
export function spacePreview(tabs: readonly LayoutTab[]): PreviewRow[] {
  return flatTabs(tabs).map(({ tab, depth }) => {
    const ls = leaves(tab.root);
    return {
      name: tab.name || "(unnamed)",
      depth,
      detail: ls.length === 1 ? (ls[0].command ?? ls[0].name ?? "shell") : `${ls.length} terminals`,
    };
  });
}

/** An editor-group arrangement: a group of tabs, or two arrangements split by a direction. */
export type GroupNode =
  | { kind: "group"; tabs: string[] }
  | { kind: "split"; direction: SplitDirection; ratio: number; children: [GroupNode, GroupNode] };

/**
 * How the app arranges the tabs in editor groups: the first tab opens the first group; a tab with
 * a `place` splits the group of the tab it is placed of (`of`, else its parent, else the first
 * tab) and gets the new group; a tab without one joins the group of that tab.
 */
export function editorGroups(tabs: readonly LayoutTab[]): GroupNode | null {
  const all = flatTabs(tabs);
  if (all.length === 0) return null;
  const parentName = (path: TabPath): string | null => tabAt(tabs, path.slice(0, -1))?.name ?? null;
  let root: GroupNode = { kind: "group", tabs: [all[0].tab.name] };
  const groupOf = new Map<string, { kind: "group"; tabs: string[] }>();
  groupOf.set(all[0].tab.name, root);
  for (const { tab, path } of all.slice(1)) {
    const target = tab.place?.of ?? parentName(path) ?? all[0].tab.name;
    const host = groupOf.get(target) ?? groupOf.get(all[0].tab.name)!;
    if (!tab.place) {
      host.tabs.push(tab.name);
      groupOf.set(tab.name, host);
      continue;
    }
    const fresh: { kind: "group"; tabs: string[] } = { kind: "group", tabs: [tab.name] };
    const split: GroupNode = {
      kind: "split",
      direction: tab.place.direction,
      ratio: tab.place.ratio,
      children: [host, fresh],
    };
    root = replaceGroup(root, host, split);
    groupOf.set(tab.name, fresh);
  }
  return root;
}

function replaceGroup(node: GroupNode, from: GroupNode, to: GroupNode): GroupNode {
  if (node === from) return to;
  if (node.kind === "group") return node;
  return {
    ...node,
    children: [replaceGroup(node.children[0], from, to), replaceGroup(node.children[1], from, to)],
  };
}

/** The layout of one tab of the snapshot with the tabs below it (`Tab.parent`): "Save current tab as layout…". */
export function layoutFromTab(snapshot: Snapshot, tabId: string, name: string): NamedLayout | null {
  const tab = snapshot.tabs.find((t) => t.tab_id === tabId);
  if (!tab) return null;
  const names = new Set<string>();
  const build = (id: string): LayoutTab | null => {
    const t = snapshot.tabs.find((x) => x.tab_id === id);
    if (!t) return null;
    const tabName = uniqueName(t.label.trim() || "tab", [...names]);
    names.add(tabName);
    const terminal = (terminalId: string): LayoutNode => {
      const term = snapshot.terminals.find((x) => x.terminal_id === terminalId);
      return {
        kind: "terminal",
        name: text(term?.label),
        command: text(term?.agent),
        cwd: null,
      };
    };
    const node = (tree: TabTree): LayoutNode =>
      tree.kind === "terminal"
        ? terminal(tree.terminal_id)
        : {
            kind: "split",
            direction: tree.direction,
            ratio: tree.ratio,
            children: [node(tree.children[0]), node(tree.children[1])],
          };
    const root: LayoutNode = t.root
      ? node(t.root)
      : { kind: "terminal", name: null, command: null, cwd: null };
    const children = snapshot.tabs
      .filter((x) => x.parent === id && x.space_id === t.space_id)
      .sort((a, b) => a.number - b.number)
      .flatMap((x) => build(x.tab_id) ?? []);
    // A lone leaf takes the tab's name, so the tab is written flat.
    const single = root.kind === "terminal" && root.name === null ? root : null;
    return {
      name: tabName,
      root: single ? { ...single } : root,
      children,
      // The server keeps no place: it is the app's arrangement, set in the editor.
      place: null,
    };
  };
  const top = build(tabId);
  return top ? { name, tabs: [top] } : null;
}
