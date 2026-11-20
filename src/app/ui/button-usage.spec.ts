import { readFileSync, readdirSync, statSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";

/**
 * Buttons are `button[hx-button]` (AGENTS.md "Buttons, tags and fields"). A new plain `<button>` fails
 * here. Not counted: the files whose buttons are rows or tabs, not actions (tree rows, tab bar, palette
 * items). `LEGACY` is the number of plain buttons each other file still has; it only goes down. When you
 * convert buttons, lower the number (delete the line at 0).
 */
const ROW_FILES = [
  "src/app/sidebar/sidebar.html",
  "src/app/ui/helix-candidates/tab-bar/tab-bar.html",
  "src/app/ui/context-menu/context-menu.html",
  "src/app/command-palette/command-palette.html",
  "src/app/jump-palette/jump-palette.html",
];

const LEGACY: Record<string, number> = {
  "src/app/agent-picker/agent-picker.html": 3,
  "src/app/app.html": 1,
  "src/app/explorer/explorer.html": 1,
  "src/app/notifications/notifications.html": 1,
  "src/app/sidebar/machines-section/machines-section.html": 2,
  "src/app/terminal-view/terminal-view.html": 5,
  "src/app/toolbar/toolbar.html": 2,
};

/** Reference surfaces (#844): only buttons that are not actions stay plain. */
const REFERENCE_MAX: Record<string, number> = {};

function sources(dir: string): string[] {
  return readdirSync(dir).flatMap((name) => {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) return sources(path);
    return /\.(html|ts)$/.test(name) && !name.endsWith(".spec.ts") ? [path] : [];
  });
}

const plain = (text: string): number =>
  (text.match(/<button\b[^>]*>/g) ?? []).filter((tag) => !/\bhx-button\b/.test(tag)).length;

describe("buttons", () => {
  const counts = new Map(
    sources("src/app")
      .filter((file) => !ROW_FILES.includes(file))
      .map((file) => [file, plain(readFileSync(file, "utf8"))] as const)
      .filter(([, n]) => n > 0),
  );

  it.each([...counts])("%s has no new button without hx-button", (file, n) => {
    expect(
      n,
      `${file}: use <button hx-button> (AGENTS.md "Buttons, tags and fields")`,
    ).toBeLessThanOrEqual(LEGACY[file] ?? 0);
  });

  it("lists no file that is clean now", () => {
    const stale = Object.keys(LEGACY).filter((file) => !counts.has(file));
    expect(stale).toEqual([]);
  });

  it("keeps the reference surfaces converted", () => {
    for (const [file, max] of Object.entries(REFERENCE_MAX)) {
      expect(counts.get(file) ?? 0).toBeLessThanOrEqual(max);
    }
  });

  it("keeps the row files in the allow list", () => {
    for (const file of ROW_FILES) expect(statSync(file).isFile()).toBe(true);
  });
});
