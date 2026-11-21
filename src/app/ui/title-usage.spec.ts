import { readFileSync, readdirSync, statSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";

/**
 * A hint on a control is `hx-tooltip` (AGENTS.md "Tooltips"), not the native `title` attribute: it
 * appears late, cannot be styled and shows no shortcut. `LEGACY` is the number of `title` attributes
 * (`title=`, `[title]`, `[attr.title]`) each file still has; it only goes down. When you convert one,
 * lower the number (delete the line at 0).
 */
const LEGACY: Record<string, number> = {
  "src/app/bottom-panel/bottom-panel.html": 6,
  "src/app/bottom-panel/flow-view.html": 7,
  "src/app/changes/changes-view.html": 7,
  "src/app/changes/comments-panel.html": 1,
  "src/app/deliver/deliver-view.html": 3,
  "src/app/explorer/explorer.html": 2,
  "src/app/github/chip-picker/chip-picker.html": 1,
  "src/app/github/github-view.html": 6,
  "src/app/github/inbox-section.html": 1,
  "src/app/github/issue-detail/issue-detail.html": 5,
  "src/app/github/issues-section.html": 8,
  "src/app/github/milestones-section.html": 4,
  "src/app/github/pull-detail/pull-detail.html": 5,
  "src/app/jump-palette/jump-palette.html": 2,
  "src/app/machines-page/machine-form.html": 1,
  "src/app/machines-page/machines-page.html": 1,
  "src/app/notifications/notifications.html": 1,
  "src/app/pane-dialog/pane-dialog.html": 1,
  "src/app/settings-view/layout-tabs-editor.html": 9,
  "src/app/settings-view/layouts-editor.html": 3,
  "src/app/settings-view/settings-view.html": 5,
  "src/app/settings-view/space-templates-editor.html": 6,
  "src/app/sidebar/machines-section/machines-section.html": 2,
  "src/app/sidebar/space-card.html": 18,
  "src/app/space-dialog/space-dialog.html": 1,
  "src/app/space-overview/space-overview.html": 2,
  "src/app/status-bar/status-bar.html": 12,
  "src/app/task-dialog/task-dialog.html": 2,
  "src/app/terminal-view/terminal-view.html": 2,
  "src/app/ui/context-menu/context-menu.html": 1,
  "src/app/ui/dialog-shell/dialog-shell.html": 1,
  "src/app/ui/helix-candidates/tab-bar/tab-bar.html": 1,
};

function sources(dir: string): string[] {
  return readdirSync(dir).flatMap((name) => {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) return sources(path);
    return /\.(html|ts)$/.test(name) && !name.endsWith(".spec.ts") ? [path] : [];
  });
}

/** `title=` as an attribute, not as an object key or a property (`title: "x"`, `.title`). */
const titles = (text: string): number =>
  (text.match(/(?<![\w.-])(?:\[title\]|\[attr\.title\]|title)=["{]/g) ?? []).length;

describe("title attributes", () => {
  const counts = new Map(
    sources("src/app")
      .map((file) => [file, titles(readFileSync(file, "utf8"))] as const)
      .filter(([, n]) => n > 0),
  );

  it.each([...counts])("%s has no new title attribute", (file, n) => {
    expect(n, `${file}: use hx-tooltip (AGENTS.md "Tooltips")`).toBeLessThanOrEqual(
      LEGACY[file] ?? 0,
    );
  });

  it("lists no file that is clean now", () => {
    expect(Object.keys(LEGACY).filter((file) => !counts.has(file))).toEqual([]);
  });
});
