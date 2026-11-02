/** How a split lays out its children: side by side (`row`) or stacked (`column`). */
export type SplitDirection = "row" | "column";

/** The layout of the editor area: a tree of splits whose leaves are editor groups. */
export type LayoutNode =
  | { kind: "group"; id: number }
  | {
      kind: "split";
      id: number;
      dir: SplitDirection;
      children: readonly LayoutNode[];
      sizes: readonly number[];
    };

/** Ids of the groups in reading order (left to right, top to bottom). */
export function groupIds(node: LayoutNode): number[] {
  return node.kind === "group" ? [node.id] : node.children.flatMap(groupIds);
}

function normalize(sizes: readonly number[]): number[] {
  const total = sizes.reduce((a, b) => a + b, 0);
  return total > 0 ? sizes.map((s) => s / total) : sizes.map(() => 1 / sizes.length);
}

/**
 * Puts a new group `newId` after group `groupId` in direction `dir`. The group is halved; inside a
 * split of the same direction the new group becomes a sibling, else a nested split (`splitId`) is made.
 */
export function splitGroup(
  node: LayoutNode,
  groupId: number,
  newId: number,
  dir: SplitDirection,
  splitId: number,
): LayoutNode {
  if (node.kind === "group") {
    if (node.id !== groupId) return node;
    return {
      kind: "split",
      id: splitId,
      dir,
      children: [node, { kind: "group", id: newId }],
      sizes: [0.5, 0.5],
    };
  }
  const at = node.children.findIndex((c) => c.kind === "group" && c.id === groupId);
  if (at >= 0 && node.dir === dir) {
    const children = [...node.children];
    children.splice(at + 1, 0, { kind: "group", id: newId });
    const sizes = [...node.sizes];
    sizes.splice(at, 1, node.sizes[at] / 2, node.sizes[at] / 2);
    return { ...node, children, sizes };
  }
  return {
    ...node,
    children: node.children.map((c) => splitGroup(c, groupId, newId, dir, splitId)),
  };
}

/** Removes a group; its space goes to the siblings and a split left with one child collapses. */
export function removeGroup(node: LayoutNode, groupId: number): LayoutNode | null {
  if (node.kind === "group") return node.id === groupId ? null : node;
  const kept: { child: LayoutNode; size: number }[] = [];
  node.children.forEach((child, i) => {
    const next = removeGroup(child, groupId);
    if (next) kept.push({ child: next, size: node.sizes[i] });
  });
  if (kept.length === 0) return null;
  if (kept.length === 1) return kept[0].child;
  return {
    ...node,
    children: kept.map((k) => k.child),
    sizes: normalize(kept.map((k) => k.size)),
  };
}

/** Replaces the sizes of the split `splitId`; a list of the wrong length is ignored. */
export function setSplitSizes(
  node: LayoutNode,
  splitId: number,
  sizes: readonly number[],
): LayoutNode {
  if (node.kind === "group") return node;
  if (node.id === splitId) {
    return sizes.length === node.children.length ? { ...node, sizes: normalize(sizes) } : node;
  }
  return { ...node, children: node.children.map((c) => setSplitSizes(c, splitId, sizes)) };
}

/**
 * Moves the divider after pane `index` by `delta` (a fraction of the whole), taking from and giving
 * to its two neighbors, neither below `min`.
 */
export function resizeSizes(
  sizes: readonly number[],
  index: number,
  delta: number,
  min = 0.1,
): number[] {
  if (index < 0 || index >= sizes.length - 1) return [...sizes];
  const pair = sizes[index] + sizes[index + 1];
  const floor = Math.min(min, pair / 2);
  const first = Math.min(Math.max(sizes[index] + delta, floor), pair - floor);
  const next = [...sizes];
  next[index] = first;
  next[index + 1] = pair - first;
  return next;
}
