/**
 * Named layouts (`layouts[]` of the user config, ADR 0011): tabs with a split tree each, nested
 * `children[]` and an app-only `place`. The wire shape the backend sends (`NamedLayout` of
 * `poimen-core`), the JSON written back, the pure edits of the Layouts page, the delete check and
 * the preview. Validation mirrors `layout::parse_tabs_value`; the backend validates again.
 */
import type { SplitDirection } from "./space-layout";

export type LayoutNode =
  | { kind: "terminal"; name: string | null; command: string | null; cwd: string | null }
  | {
      kind: "split";
      direction: SplitDirection;
      /** The share of the first child. */
      ratio: number;
      children: [LayoutNode, LayoutNode];
    };

/** Where a tab goes in the editor groups of the app. `of: null` is the parent tab (the first tab at top level). */
export interface LayoutPlace {
  direction: SplitDirection;
  of: string | null;
  ratio: number;
}

export interface LayoutTab {
  name: string;
  /** A lone terminal leaf is a flat tab. */
  root: LayoutNode;
  children: LayoutTab[];
  place: LayoutPlace | null;
}

export interface NamedLayout {
  name: string;
  tabs: LayoutTab[];
}

/** Position of a tab: the index at each level, from the top. */
export type TabPath = readonly number[];
/** Position of a node of a split tree: 0 or 1 at each split, from the root. */
export type NodePath = readonly number[];

export const MIN_RATIO = 0.1;
export const MAX_RATIO = 0.9;

export function text(v: unknown): string | null {
  return typeof v === "string" && v.trim() !== "" ? v : null;
}

export function parseNode(raw: unknown): LayoutNode | null {
  if (typeof raw !== "object" || raw === null) return null;
  const r = raw as Record<string, unknown>;
  if (r["kind"] === "terminal") {
    return {
      kind: "terminal",
      name: text(r["name"]),
      command: text(r["command"]),
      cwd: text(r["cwd"]),
    };
  }
  if (r["kind"] !== "split" || !Array.isArray(r["children"]) || r["children"].length !== 2) {
    return null;
  }
  const a = parseNode(r["children"][0]);
  const b = parseNode(r["children"][1]);
  if (!a || !b) return null;
  return {
    kind: "split",
    direction: r["direction"] === "down" ? "down" : "right",
    ratio: typeof r["ratio"] === "number" ? r["ratio"] : 0.5,
    children: [a, b],
  };
}

export function parsePlace(raw: unknown): LayoutPlace | null {
  if (typeof raw !== "object" || raw === null) return null;
  const r = raw as Record<string, unknown>;
  return {
    direction: r["direction"] === "down" ? "down" : "right",
    of: text(r["of"]),
    ratio: typeof r["ratio"] === "number" ? r["ratio"] : 0.5,
  };
}

function parseTabs(raw: unknown): LayoutTab[] | null {
  if (!Array.isArray(raw)) return null;
  const out: LayoutTab[] = [];
  for (const t of raw) {
    if (typeof t !== "object" || t === null) return null;
    const r = t as Record<string, unknown>;
    const root = parseNode(r["root"]);
    const children = r["children"] === null ? [] : parseTabs(r["children"] ?? []);
    if (!root || !children) return null;
    out.push({ name: text(r["name"]) ?? "", root, children, place: parsePlace(r["place"]) });
  }
  return out;
}

/** The layouts of a settings payload (`layouts`); malformed entries are skipped. */
export function parseNamedLayouts(raw: unknown): NamedLayout[] {
  if (!Array.isArray(raw)) return [];
  return raw.flatMap((item): NamedLayout[] => {
    if (typeof item !== "object" || item === null) return [];
    const r = item as Record<string, unknown>;
    const layout = r["layout"] as Record<string, unknown> | undefined;
    const tabs = parseTabs(layout?.["tabs"]);
    if (typeof r["name"] !== "string" || !tabs || tabs.length === 0) return [];
    return [{ name: r["name"], tabs }];
  });
}

function nodeConfig(n: LayoutNode): Record<string, unknown> {
  if (n.kind === "terminal") {
    const out: Record<string, unknown> = { kind: "terminal" };
    if (n.name?.trim()) out["name"] = n.name.trim();
    if (n.command?.trim()) out["command"] = n.command.trim();
    if (n.cwd?.trim()) out["cwd"] = n.cwd.trim();
    return out;
  }
  return {
    kind: "split",
    direction: n.direction,
    ratio: n.ratio,
    children: n.children.map(nodeConfig),
  };
}

function tabConfig(t: LayoutTab): Record<string, unknown> {
  const out: Record<string, unknown> = { name: t.name.trim() };
  const leaf = t.root;
  const flat =
    leaf.kind === "terminal" && (leaf.name === null || leaf.name.trim() === t.name.trim());
  if (flat) {
    if (leaf.command?.trim()) out["command"] = leaf.command.trim();
    if (leaf.cwd?.trim()) out["cwd"] = leaf.cwd.trim();
  } else {
    out["root"] = nodeConfig(leaf);
  }
  if (t.place) {
    const place: Record<string, unknown> = { direction: t.place.direction, ratio: t.place.ratio };
    if (t.place.of) place["of"] = t.place.of;
    out["place"] = place;
  }
  if (t.children.length > 0) out["children"] = t.children.map(tabConfig);
  return out;
}

/** The JSON of one entry in `layouts[]`: a flat tab where the tree is one unnamed terminal. */
export function layoutConfig(l: NamedLayout): Record<string, unknown> {
  return { name: l.name.trim(), tabs: l.tabs.map(tabConfig) };
}

/** The terminal leaves of a split tree, first child first. */
export function leaves(n: LayoutNode): Extract<LayoutNode, { kind: "terminal" }>[] {
  return n.kind === "terminal" ? [n] : n.children.flatMap(leaves);
}

/** `base`, or `base 2`, `base 3` … the first that is not in `taken`. */
export function uniqueName(base: string, taken: readonly string[]): string {
  if (!taken.includes(base)) return base;
  for (let i = 2; ; i++) {
    const candidate = `${base} ${i}`;
    if (!taken.includes(candidate)) return candidate;
  }
}
