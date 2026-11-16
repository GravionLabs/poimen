/**
 * The layout of the project file (`layout` and each of `presets[]`) as the layout editor of #610
 * edits it. The JSON text of the form stays the source of truth: these pure functions read a layout
 * object into tabs (`tabs[]` as written, `panes[]` converted like `Layout::to_tabs`) and write the
 * tabs back in the shape the loader reads, so nested `children[]` and `place` survive a round trip.
 */
import {
  layoutConfig,
  parseNode,
  parsePlace,
  text,
  type LayoutNode,
  type LayoutTab,
} from "../core/named-layouts";
import type { SplitDirection } from "../core/space-layout";

type Json = Record<string, unknown>;

function record(raw: unknown): Json | null {
  return typeof raw === "object" && raw !== null && !Array.isArray(raw) ? (raw as Json) : null;
}

function tabOf(raw: unknown): LayoutTab | null {
  const r = record(raw);
  if (!r) return null;
  const name = text(r["name"]) ?? "";
  let root: LayoutNode | null;
  if (r["root"] !== undefined && r["root"] !== null) {
    root = parseNode(r["root"]);
  } else {
    root = { kind: "terminal", name: null, command: text(r["command"]), cwd: text(r["cwd"]) };
  }
  let children: LayoutTab[] | null = [];
  if (r["children"] !== undefined && r["children"] !== null) children = tabsOf(r["children"]);
  if (!root || !children) return null;
  return { name, root, children, place: parsePlace(r["place"]) };
}

function tabsOf(raw: unknown): LayoutTab[] | null {
  if (!Array.isArray(raw)) return null;
  const out: LayoutTab[] = [];
  for (const t of raw) {
    const tab = tabOf(t);
    if (!tab) return null;
    out.push(tab);
  }
  return out;
}

/** Replaces the leaf named `from` by a split of it and `added`. */
function splitLeaf(
  node: LayoutNode,
  from: string,
  direction: SplitDirection,
  ratio: number,
  added: LayoutNode,
): { node: LayoutNode; done: boolean } {
  if (node.kind === "terminal") {
    if (node.name !== from) return { node, done: false };
    return { node: { kind: "split", direction, ratio, children: [node, added] }, done: true };
  }
  const a = splitLeaf(node.children[0], from, direction, ratio, added);
  if (a.done) return { node: { ...node, children: [a.node, node.children[1]] }, done: true };
  const b = splitLeaf(node.children[1], from, direction, ratio, added);
  return { node: { ...node, children: [node.children[0], b.node] }, done: b.done };
}

/** `panes[]` as one tab: the first pane is the root leaf, each later pane splits the leaf `from`. */
function tabsOfPanes(panes: unknown[]): LayoutTab[] | null {
  const specs = panes.map(record);
  if (specs.length === 0 || specs.some((p) => p === null || text(p["name"]) === null)) return null;
  const leaf = (p: Json): LayoutNode => ({
    kind: "terminal",
    name: text(p["name"]),
    command: text(p["command"]),
    cwd: null,
  });
  const first = specs[0]!;
  let root = leaf(first);
  for (const [i, p] of specs.entries()) {
    if (i === 0) continue;
    const split = record(p!["split"]);
    if (!split) return null;
    const from = text(split["from"]) ?? text(specs[i - 1]!["name"])!;
    const ratio = typeof split["ratio"] === "number" ? split["ratio"] : 0.5;
    const direction: SplitDirection = split["direction"] === "down" ? "down" : "right";
    const next = splitLeaf(root, from, direction, ratio, leaf(p!));
    if (!next.done) return null;
    root = next.node;
  }
  return [{ name: text(first["name"])!, root, children: [], place: null }];
}

/**
 * The tabs of a layout object (a project `layout` or a preset), or `null` when the editor cannot
 * show it (not an object, neither `tabs[]` nor `panes[]`, a malformed node); "Advanced" then keeps
 * the JSON editable.
 */
export function tabsOfLayout(raw: unknown): LayoutTab[] | null {
  const r = record(raw);
  if (!r) return null;
  if (r["tabs"] !== undefined) return tabsOf(r["tabs"]);
  if (Array.isArray(r["panes"])) return tabsOfPanes(r["panes"]);
  return null;
}

/** The layout object the loader reads for `tabs`: `{tabs: [...]}`, a flat tab where possible. */
export function layoutOfTabs(tabs: LayoutTab[]): { tabs: unknown[] } {
  return { tabs: layoutConfig({ name: "layout", tabs })["tabs"] as unknown[] };
}

/** A preset as the editor shows it; `tabs` is `null` for a layout it cannot show. */
export interface PresetView {
  name: string;
  tabs: LayoutTab[] | null;
}

function presetList(source: string): Json[] | null {
  if (source.trim() === "") return [];
  let raw: unknown;
  try {
    raw = JSON.parse(source);
  } catch {
    return null;
  }
  if (!Array.isArray(raw)) return null;
  const out: Json[] = [];
  for (const p of raw) {
    const r = record(p);
    if (!r) return null;
    out.push(r);
  }
  return out;
}

/** The presets of the JSON text of `presets`; `null` for text the editor cannot show. */
export function presetsOfText(source: string): PresetView[] | null {
  return (
    presetList(source)?.map((r) => ({
      name: typeof r["name"] === "string" ? r["name"] : "",
      tabs: tabsOfLayout(r),
    })) ?? null
  );
}

function presetsJson(list: Json[]): string {
  return list.length === 0 ? "" : JSON.stringify(list, null, 2);
}

/**
 * The text of `presets` with preset `index` changed: its name and/or its tabs (written as `tabs[]`
 * and replacing `panes[]`); the other presets and the other keys of the entry stay as they are.
 */
export function changePreset(
  source: string,
  index: number,
  change: { name?: string; tabs?: LayoutTab[] },
): string {
  const list = presetList(source);
  if (!list?.[index]) return source;
  const entry: Json = { ...list[index] };
  if (change.name !== undefined) entry["name"] = change.name;
  if (change.tabs) {
    delete entry["panes"];
    entry["tabs"] = layoutOfTabs(change.tabs).tabs;
  }
  return presetsJson(list.map((p, i) => (i === index ? entry : p)));
}

/** The text of `presets` with a new preset called `name` (one empty tab) appended. */
export function addPreset(source: string, name: string): string {
  const list = presetList(source) ?? [];
  const tab: LayoutTab = {
    name: "shell",
    root: { kind: "terminal", name: null, command: null, cwd: null },
    children: [],
    place: null,
  };
  return presetsJson([...list, { name, ...layoutOfTabs([tab]) }]);
}

/** The text of `presets` without preset `index`. */
export function removePreset(source: string, index: number): string {
  const list = presetList(source);
  return list ? presetsJson(list.filter((_, i) => i !== index)) : source;
}

/** The first problem of preset names: blank or repeated. */
export function presetNameError(names: readonly string[]): string | null {
  const seen = new Set<string>();
  for (const raw of names) {
    const name = raw.trim();
    if (name === "") return "A preset needs a name";
    if (seen.has(name)) return `Two presets are called "${name}"`;
    seen.add(name);
  }
  return null;
}

/** The layout object of `presets[index]` as a layout file (name left out), for export. */
export function presetLayout(source: string, index: number): Json | null {
  const entry = presetList(source)?.[index];
  if (!entry) return null;
  return Object.fromEntries(Object.entries(entry).filter(([key]) => key !== "name"));
}

/** The text of `presets` with the layout of preset `index` replaced by an imported layout object. */
export function replacePresetLayout(source: string, index: number, layout: Json): string {
  const list = presetList(source);
  if (!list?.[index]) return source;
  const name = list[index]["name"];
  return presetsJson(list.map((p, i) => (i === index ? { name, ...layout } : p)));
}
