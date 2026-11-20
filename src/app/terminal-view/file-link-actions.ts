import { Injectable, inject } from "@angular/core";
import { EditorTabs } from "../core/editor-tabs";
import { ExternalEditor } from "../core/external-editor";
import { TauriBridge } from "../core/tauri";
import { plainGroups } from "../ui/context-menu/menu-model";
import type { FileLinkTarget, ResolvedFile } from "./file-links";

/** The entries of the menu a link to a changed file opens: the editor first, then the Changes tab. */
export const FILE_LINK_MENU = plainGroups([
  { id: "open", label: "Open in editor" },
  { id: "changes", label: "Show in Changes" },
]);

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

  showInChanges(target: FileLinkTarget): void {
    const repo = target.file.repo;
    if (repo) this.tabs.openChanges(repo.root, repo.rel);
  }
}
