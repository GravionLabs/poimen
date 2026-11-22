import { Injectable, inject, signal } from "@angular/core";
import { joinPath } from "./path";
import { splitId } from "./snapshot";
import { SpaceActions } from "./space-actions";
import { SpacesStore } from "./spaces-store";
import { TauriBridge } from "./tauri";

/** Where in a file the editor opens; both are 1-based. */
export interface EditorPosition {
  line?: number;
  column?: number;
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

  private readonly _remoteBlock = signal<string | null>(null);
  /** Why a space of a machine cannot be opened (the editor has no remote support); `null` when it can. */
  readonly remoteBlock = this._remoteBlock.asReadonly();

  /** Reads whether the editor of the setting can open a remote space (#1005); the setting is a file. */
  async refreshRemote(): Promise<void> {
    try {
      await this.bridge.invoke("open_in_editor", {
        path: "/",
        line: null,
        column: null,
        machine: null,
        probe: true,
      });
      this._remoteBlock.set(null);
    } catch (e) {
      this._remoteBlock.set(e instanceof Error ? e.message : String(e));
    }
  }

  async open(path: string, position: EditorPosition = {}): Promise<boolean> {
    try {
      await this.bridge.invoke("open_in_editor", {
        path,
        line: position.line ?? null,
        column: position.column ?? null,
      });
      return true;
    } catch (e) {
      this.actions.notify(`Open in editor: ${e instanceof Error ? e.message : String(e)}`, "error");
      return false;
    }
  }

  /** The palette entry: `dir` is the directory of the active tab. */
  openDir(dir: string | undefined): void {
    if (dir) void this.open(dir);
    else this.actions.notify("Open in editor: there is no active tab");
  }

  /**
   * The entry of a space's menu: the directory of the space. A space of a machine (qualified id)
   * opens over ssh in VS Code Remote-SSH or Zed (#1005); the menu disables the entry while the
   * machine is not connected.
   */
  openSpace(spaceId: string): void {
    const { machine, id } = splitId(spaceId);
    if (machine !== undefined) {
      const cwd = this.store.all().spaces.find((s) => s.space_id === spaceId)?.cwd;
      if (cwd) void this.openRemote(machine, cwd);
      return;
    }
    const cwd = this.store.snapshot().spaces.find((s) => s.space_id === id)?.cwd;
    if (cwd) void this.open(cwd);
  }

  /** Opens `path` on `machine` (VS Code Remote-SSH or Zed); a failure is a notice. */
  async openRemote(machine: string, path: string): Promise<boolean> {
    try {
      await this.bridge.invoke("open_in_editor", { path, line: null, column: null, machine });
      return true;
    } catch (e) {
      this.actions.notify(`Open in editor: ${e instanceof Error ? e.message : String(e)}`, "error");
      return false;
    }
  }
}

export { joinPath };
