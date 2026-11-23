import { Injectable, inject } from "@angular/core";
import { EditorTabs } from "../core/editor-tabs";
import { ExternalEditor } from "../core/external-editor";
import { TauriBridge } from "../core/tauri";
import { plainGroups } from "../ui/context-menu/menu-model";
import type { FileLinkTarget, ResolvedFile } from "./file-links";

/**
 * The entries of the menu a link to a file opens: the editor, the preview tab (#1004), and for a
 * changed file the Changes tab. Two constants: the menu takes the same groups on every check.
 */
const PLAIN_ENTRIES = [
  { id: "open", label: "Open in editor" },
  { id: "preview", label: "Preview" },
];
const FILE_LINK_MENUS = {
  plain: plainGroups(PLAIN_ENTRIES),
  changed: plainGroups([...PLAIN_ENTRIES, { id: "changes", label: "Show in Changes" }]),
};

export function fileLinkMenu(changed: boolean) {
  return changed ? FILE_LINK_MENUS.changed : FILE_LINK_MENUS.plain;
}

/** What clicking a file path in a terminal does (#1003). */
@Injectable({ providedIn: "root" })
export class FileLinkActions {
  private readonly bridge = inject(TauriBridge);
  private readonly editor = inject(ExternalEditor);
  private readonly tabs = inject(EditorTabs);

  /** The files the `paths` of a line name, `null` for what is not an existing file. */
  resolve(dir: string, paths: string[]): Promise<(ResolvedFile | null)[]> {
    return this.bridge.invoke<(ResolvedFile | null)[]>("resolve_file_links", { dir, paths });
  }

  /** Whether the file is one of the changes of its worktree (against the base). */
  async isChanged(target: FileLinkTarget): Promise<boolean> {
    const repo = target.file.repo;
    if (!repo) return false;
    try {
      const list = await this.bridge.invoke<{ files: { path: string }[] }>("changes_list", {
        dir: repo.root,
        scope: "base",
      });
      return list.files.some((f) => f.path === repo.rel);
    } catch {
      return false;
    }
  }

  open(target: FileLinkTarget): void {
    void this.editor.open(target.file.path, { line: target.line, column: target.column });
  }

  /** The entry `id` of the menu (`fileLinkMenu`) for `target`. */
  pick(id: string, target: FileLinkTarget): void {
    if (id === "changes") this.showInChanges(target);
    else if (id === "preview") this.preview(target);
    else this.open(target);
  }

  /** Opens the read-only preview tab of the file. */
  preview(target: FileLinkTarget): void {
    this.tabs.openPreview(target.file.path);
  }

  showInChanges(target: FileLinkTarget): void {
    const repo = target.file.repo;
    if (repo) this.tabs.openChanges(repo.root, repo.rel);
  }
}
