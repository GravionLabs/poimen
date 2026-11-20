import { Injectable, inject } from "@angular/core";
import { SpaceActions } from "./space-actions";
import { SpacesStore } from "./spaces-store";
import { TauriBridge } from "./tauri";

/** Where in a file the editor opens; both are 1-based. */
export interface EditorPosition {
  line?: number;
  column?: number;
}

/** The path `file` (relative to `dir`, or absolute as it is) as one path, with `/` between the parts. */
export function joinPath(dir: string, file: string): string {
  if (/^([\\/]|[A-Za-z]:[\\/])/.test(file)) return file;
  return `${dir.replace(/[\\/]+$/, "")}/${file.replace(/^[\\/]+/, "")}`;
}

/**
 * "Open in editor" (#1002): the `editor` setting of `config.json` (a preset id or an argv template)
 * decides, else the first editor found on the login-shell `PATH`; the backend runs it without a
 * shell. A failure is a notice that names the setting.
 */
@Injectable({ providedIn: "root" })
export class ExternalEditor {
  private readonly bridge = inject(TauriBridge);
  private readonly actions = inject(SpaceActions);
  private readonly store = inject(SpacesStore);

  async open(path: string, position: EditorPosition = {}): Promise<boolean> {
    try {
      await this.bridge.invoke("open_in_editor", {
        path,
        line: position.line ?? null,
        column: position.column ?? null,
      });
      return true;
    } catch (e) {
      this.actions.notify(`Open in editor: ${e instanceof Error ? e.message : String(e)}`);
      return false;
    }
  }

  /** The palette entry: `dir` is the directory of the active tab. */
  openDir(dir: string | undefined): void {
    if (dir) void this.open(dir);
    else this.actions.notify("Open in editor: there is no active tab");
  }

  /** The entry of a space's menu: the directory of the (local) space. */
  openSpace(spaceId: string): void {
    const cwd = this.store.snapshot().spaces.find((s) => s.space_id === spaceId)?.cwd;
    if (cwd) void this.open(cwd);
  }
}
