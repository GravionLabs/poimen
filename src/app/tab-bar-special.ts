import { changesDir, previewPath, spaceOfTab, tabKind } from "./core/editor-tabs";
import type { Space } from "./core/snapshot";
import { fileName } from "./file-preview/preview-model";
import type { TabBarItem } from "./ui/helix-candidates/tab-bar/tab-bar";

/**
 * The tab bar item of a tab that is no terminal (settings, GitHub, Runs, space overview, changes,
 * file preview); `null` for a pane or a flow tab.
 */
export function specialTabItem(id: string, spaces: readonly Space[]): TabBarItem | null {
  const kind = tabKind(id);
  if (kind === "settings") return { id, label: "Settings", title: "Settings" };
  if (kind === "github") {
    return { id, label: "Pull requests", group: "GitHub", title: "GitHub pull requests" };
  }
  if (kind === "deliver") return { id, label: "Runs", title: "Agent runs" };
  const spaceOf = spaceOfTab(id);
  if (spaceOf !== null) {
    const name = spaces.find((w) => w.space_id === spaceOf)?.label ?? spaceOf;
    return { id, label: `Space: ${name}`, title: `Overview of space ${name}` };
  }
  const changesOf = changesDir(id);
  if (changesOf !== null) {
    const name = changesOf.split("/").filter(Boolean).pop() ?? changesOf;
    return { id, label: `Changes: ${name}`, title: `Changes of ${changesOf}` };
  }
  const previewOf = previewPath(id);
  if (previewOf !== null) {
    return { id, label: `Preview: ${fileName(previewOf)}`, title: `Preview of ${previewOf}` };
  }
  return null;
}
