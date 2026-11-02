/**
 * The pane layout of a new space (`spaces.layout`, the layout-bootstrap schema of
 * `src-tauri/src/layout.rs`) and the pure edits of the settings view. Validation mirrors
 * `parse_layout_value`; the backend validates again before it writes.
 */

export type SplitDirection = "right" | "down";

export interface PaneSplit {
  /** An earlier pane. */
  from: string;
  direction: SplitDirection;
  /** The share the split pane keeps; the new pane gets the rest. */
  ratio: number;
}

export interface PaneSpec {
  name: string;
  /** Typed into the pane, then Enter; `null` is a plain shell. */
  command: string | null;
  /** `null` for the first pane, the root pane of the space. */
  split: PaneSplit | null;
}

export interface SpaceLayout {
  panes: PaneSpec[];
}

export const MIN_RATIO = 0.1;
export const MAX_RATIO = 0.9;

/** A layout from the backend (`spacesLayout`, `default_spaces_layout`), or `null` when malformed. */
export function parseSpaceLayout(raw: unknown): SpaceLayout | null {
  if (typeof raw !== "object" || raw === null) return null;
  const list = (raw as Record<string, unknown>)["panes"];
  if (!Array.isArray(list) || list.length === 0) return null;
  const panes: PaneSpec[] = [];
  for (const p of list) {
    if (typeof p !== "object" || p === null) return null;
    const o = p as Record<string, unknown>;
    if (typeof o["name"] !== "string") return null;
    const s = o["split"] as Record<string, unknown> | null | undefined;
    panes.push({
      name: o["name"],
      command: typeof o["command"] === "string" && o["command"] !== "" ? o["command"] : null,
      split:
        s && typeof s === "object"
          ? {
              from: typeof s["from"] === "string" ? s["from"] : "",
              direction: s["direction"] === "down" ? "down" : "right",
              ratio: typeof s["ratio"] === "number" ? s["ratio"] : 0.5,
            }
          : null,
    });
  }
  return { panes };
}

/** Problem per pane (same index), `null` where the pane is fine. */
export function layoutErrors(panes: readonly PaneSpec[]): (string | null)[] {
  const seen = new Set<string>();
  return panes.map((pane, i) => {
    const name = pane.name.trim();
    let error: string | null = null;
    if (!name) error = "A name is required";
    else if (seen.has(name)) error = `Duplicate name "${name}"`;
    else if (i > 0 && !pane.split) error = "Needs a split";
    else if (i > 0 && pane.split && !seen.has(pane.split.from.trim()))
      error = "Split from must be an earlier pane";
    else if (
      pane.split &&
      !(
        Number.isFinite(pane.split.ratio) &&
        pane.split.ratio >= MIN_RATIO &&
        pane.split.ratio <= MAX_RATIO
      )
    )
      error = `Ratio must be from ${MIN_RATIO} to ${MAX_RATIO}`;
    if (name) seen.add(name);
    return error;
  });
}

export function isValidLayout(panes: readonly PaneSpec[]): boolean {
  return panes.length > 0 && layoutErrors(panes).every((e) => e === null);
}

/** The first pane has no split; every later pane has one (from the previous pane by default). */
function normalized(panes: PaneSpec[]): PaneSpec[] {
  return panes.map((p, i) =>
    i === 0
      ? { ...p, split: null }
      : p.split
        ? p
        : { ...p, split: { from: panes[i - 1].name, direction: "right", ratio: 0.5 } },
  );
}

/** A free name `pane`, `pane-2`, … */
function freeName(panes: readonly PaneSpec[]): string {
  const names = new Set(panes.map((p) => p.name.trim()));
  if (!names.has("pane")) return "pane";
  let n = 2;
  while (names.has(`pane-${n}`)) n++;
  return `pane-${n}`;
}

/** Appends a shell pane split down from the last one. */
export function addPane(panes: readonly PaneSpec[]): PaneSpec[] {
  const last = panes.at(-1);
  const pane: PaneSpec = {
    name: freeName(panes),
    command: null,
    split: last ? { from: last.name, direction: "down", ratio: 0.5 } : null,
  };
  return [...panes, pane];
}

/** Removes a pane (never the only one); splits from it now split from the pane before it. */
export function removePane(panes: readonly PaneSpec[], index: number): PaneSpec[] {
  if (panes.length <= 1) return [...panes];
  const gone = panes[index].name;
  const rest = panes.filter((_, i) => i !== index);
  const fallback = rest[Math.max(0, index - 1)].name;
  return normalized(
    rest.map((p) =>
      p.split && p.split.from === gone ? { ...p, split: { ...p.split, from: fallback } } : p,
    ),
  );
}

/** Moves a pane by `delta` places; a split that now points at a later pane is left for validation. */
export function movePane(panes: readonly PaneSpec[], index: number, delta: number): PaneSpec[] {
  const target = Math.min(Math.max(index + delta, 0), panes.length - 1);
  if (target === index) return [...panes];
  const next = [...panes];
  const [pane] = next.splice(index, 1);
  next.splice(target, 0, pane);
  return normalized(next);
}

/** Changes one pane; a rename also renames the splits that start from it. */
export function updatePane(
  panes: readonly PaneSpec[],
  index: number,
  patch: Partial<Omit<PaneSpec, "split">> & { split?: Partial<PaneSplit> },
): PaneSpec[] {
  const old = panes[index];
  const { split, ...rest } = patch;
  const pane: PaneSpec = {
    ...old,
    ...rest,
    split: old.split && split ? { ...old.split, ...split } : old.split,
  };
  // A name another pane shares (a duplicate being fixed) is not the one the splits refer to.
  const unique = !panes.some((p, i) => i !== index && p.name === old.name);
  return panes.map((p, i) => {
    if (i === index) return pane;
    if (unique && patch.name !== undefined && p.split && p.split.from === old.name) {
      return { ...p, split: { ...p.split, from: patch.name } };
    }
    return p;
  });
}

/** The JSON written to `spaces.layout`: trimmed names, no empty command, no split on the first. */
export function toConfig(panes: readonly PaneSpec[]): Record<string, unknown> {
  return {
    panes: panes.map((p, i) => {
      const out: Record<string, unknown> = { name: p.name.trim() };
      const command = p.command?.trim();
      if (command) out["command"] = command;
      if (i > 0 && p.split) {
        out["split"] = {
          from: p.split.from.trim(),
          direction: p.split.direction,
          ratio: Math.round(p.split.ratio * 100) / 100,
        };
      }
      return out;
    }),
  };
}

export function sameLayout(a: readonly PaneSpec[], b: readonly PaneSpec[]): boolean {
  return JSON.stringify(toConfig(a)) === JSON.stringify(toConfig(b));
}
