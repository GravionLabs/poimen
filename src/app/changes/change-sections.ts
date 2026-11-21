import type { ChangedFile } from "./changes";

export type SectionId = "conflicts" | "staged" | "changes";

export interface Section {
  id: SectionId;
  title: string;
  files: ChangedFile[];
}

const TITLES: Record<SectionId, string> = {
  conflicts: "Conflicts",
  staged: "Staged",
  changes: "Changes",
};

/** The sections of the uncommitted scope: Conflicts, Staged and Changes; empty ones are left out. */
export function sectionsOf(files: readonly ChangedFile[]): Section[] {
  const pick = (id: SectionId, keep: (f: ChangedFile) => boolean): Section => ({
    id,
    title: TITLES[id],
    files: files.filter(keep),
  });
  return [
    pick("conflicts", (f) => f.conflicted),
    pick("staged", (f) => !f.conflicted && f.staged),
    pick("changes", (f) => !f.conflicted && !f.staged),
  ].filter((s) => s.files.length > 0);
}

/** The file name and its directory (with the trailing slash, empty at the root). */
export function splitPath(path: string): { name: string; dir: string } {
  const at = path.lastIndexOf("/");
  return at < 0
    ? { name: path, dir: "" }
    : { name: path.slice(at + 1), dir: path.slice(0, at + 1) };
}

/** The paths from `from` to `to` (both included, either order) in the listed order. */
export function rangeBetween(order: readonly string[], from: string, to: string): string[] {
  const a = order.indexOf(from);
  const b = order.indexOf(to);
  if (a < 0 || b < 0) return [to];
  return order.slice(Math.min(a, b), Math.max(a, b) + 1);
}
