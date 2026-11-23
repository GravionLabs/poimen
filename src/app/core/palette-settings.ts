import { isSkill, type DetectedEntry } from "./project-entries";

/** A detected entry the user hid for one repository (`palette.hidden[<root>]`); `source` is the entry's own (`npm`, `skills`). */
export interface HiddenEntry {
  source: string;
  name: string;
}

/** `palette` of the user config (#1218): the detected sources shown and the hidden entries. */
export interface PaletteSettings {
  scripts: boolean;
  skills: boolean;
  /** Repository root to its hidden entries; never stored in the repository. */
  hidden: Record<string, HiddenEntry[]>;
}

export const DEFAULT_PALETTE: PaletteSettings = { scripts: true, skills: true, hidden: {} };

/** The detected sources a user can turn off, with the label of the settings page. */
export const PALETTE_SOURCES = [
  { id: "scripts", label: "Scripts" },
  { id: "skills", label: "Skills" },
] as const;

/** Parses the `palette` of the `get_settings` payload; anything unexpected keeps the default. */
export function parsePalette(raw: unknown): PaletteSettings {
  if (typeof raw !== "object" || raw === null) return DEFAULT_PALETTE;
  const r = raw as Record<string, unknown>;
  const hidden: Record<string, HiddenEntry[]> = {};
  if (typeof r["hidden"] === "object" && r["hidden"] !== null) {
    for (const [root, list] of Object.entries(r["hidden"])) {
      const entries = Array.isArray(list) ? list.filter(isHiddenEntry) : [];
      if (entries.length > 0) hidden[root] = entries;
    }
  }
  return {
    scripts: r["scripts"] !== false,
    skills: r["skills"] !== false,
    hidden,
  };
}

function isHiddenEntry(v: unknown): v is HiddenEntry {
  const e = v as Partial<HiddenEntry> | null;
  return typeof e?.source === "string" && typeof e.name === "string";
}

/** Whether the palette shows `entry`: a skill follows `skills`, every other entry `scripts`. */
export function sourceShown(palette: PaletteSettings, entry: DetectedEntry): boolean {
  return isSkill(entry) ? palette.skills : palette.scripts;
}

/** Whether `entry` is hidden for the repository `root`. */
export function isHidden(palette: PaletteSettings, root: string, entry: DetectedEntry): boolean {
  return (palette.hidden[root] ?? []).some(
    (h) => h.source === entry.source && h.name === entry.name,
  );
}

/** The settings with `entry` hidden (or shown again) for `root`; the same object when unchanged. */
export function withHidden(
  palette: PaletteSettings,
  root: string,
  entry: HiddenEntry,
  hide: boolean,
): PaletteSettings {
  const rest = (palette.hidden[root] ?? []).filter(
    (h) => !(h.source === entry.source && h.name === entry.name),
  );
  const list = hide ? [...rest, { source: entry.source, name: entry.name }] : rest;
  const hidden = { ...palette.hidden };
  if (list.length > 0) hidden[root] = list;
  else delete hidden[root];
  return { ...palette, hidden };
}

/** The wire shape of `palette` the backend validates and writes. */
export function paletteWire(p: PaletteSettings): unknown {
  return { sources: { scripts: p.scripts, skills: p.skills }, hidden: p.hidden };
}
