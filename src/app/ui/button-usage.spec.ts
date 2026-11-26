import { readFileSync, readdirSync, statSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";

/**
 * Buttons are `button[hx-button]` (AGENTS.md "Buttons, tags and fields"); a `button[hx-list-row]` is a
 * row, not an action. Any other `<button>` (in a template or built with `createElement`) fails here
 * unless `ALLOWED` has it, with the number per file and the reason. The numbers only go down: when you
 * convert one, lower it (delete the entry at 0).
 */
const ALLOWED: Record<string, { count: number; reason: string }> = {
  "src/app/agent-picker/agent-picker.html": {
    count: 1,
    reason: "the backdrop: a full-window dismiss surface, not a visible action",
  },
  "src/app/changes/comment-extension.ts": {
    count: 3,
    reason:
      "built with createElement inside CodeMirror widgets, where the hx-button directive cannot run",
  },
  "src/app/command-palette/command-palette.html": {
    count: 2,
    reason: "the backdrop (a dismiss surface) and the source tabs (`role=tab`, not actions)",
  },
  "src/app/jump-palette/jump-palette.html": {
    count: 1,
    reason: "the backdrop: a full-window dismiss surface, not a visible action",
  },
  "src/app/notifications/notifications.html": {
    count: 1,
    reason:
      "a notification entry is a multi-line row (status dot, message, label, time), not an action; " +
      "hx-button would center it and impose the button look",
  },
  "src/app/ui/context-menu/context-menu.html": {
    count: 2,
    reason: "the backdrop and the menu items (`role=menuitem`), which the menu styles itself",
  },
  "src/app/ui/dialog-shell/dialog-contract.ts": {
    count: 1,
    reason: "test helper: the plain opener button of the dialog contract, not part of the app",
  },
  "src/app/ui/helix-candidates/tab-bar/tab-bar.html": {
    count: 4,
    reason:
      "tab strip chrome (scroll, close, all tabs) with the tab bar's own styles, to move to Helix " +
      "with the component",
  },
};

function sources(dir: string): string[] {
  return readdirSync(dir).flatMap((name) => {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) return sources(path);
    return /\.(html|ts)$/.test(name) && !name.endsWith(".spec.ts") ? [path] : [];
  });
}

const plain = (text: string): number =>
  (text.match(/<button\b[^>]*>/g) ?? []).filter((tag) => !/\bhx-(button|list-row)\b/.test(tag))
    .length + (text.match(/createElement\(["']button["']\)/g) ?? []).length;

describe("buttons", () => {
  const counts = new Map(
    sources("src/app")
      .map((file) => [file, plain(readFileSync(file, "utf8"))] as const)
      .filter(([, n]) => n > 0),
  );

  it.each([...counts])("%s has no new button without hx-button", (file, n) => {
    expect(
      n,
      `${file}: use <button hx-button> (AGENTS.md "Buttons, tags and fields")`,
    ).toBeLessThanOrEqual(ALLOWED[file]?.count ?? 0);
  });

  it("has an exact allowlist entry with a reason for every plain button", () => {
    expect(Object.fromEntries(counts)).toEqual(
      Object.fromEntries(Object.entries(ALLOWED).map(([file, { count }]) => [file, count])),
    );
    for (const { reason } of Object.values(ALLOWED)) expect(reason).not.toBe("");
  });
});
