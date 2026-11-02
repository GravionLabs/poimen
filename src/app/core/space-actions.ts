import { Injectable, inject, signal } from "@angular/core";
import { HerdrStore } from "./herdr-store";
import { TauriBridge } from "./tauri";

/** Answer of the `create_space` command. */
export interface SpaceOutcome {
  workspace_id: string;
  /** False when an existing space for the directory was focused instead. */
  created: boolean;
}

/** Last path segment: the default label of a space for `path`. */
export function dirName(path: string): string {
  return path.trim().replace(/\/+$/, "").split("/").pop() ?? "";
}

/**
 * Creates a space (herdr workspace) for a directory. herdr owns the result: the backend creates
 * or focuses the space, the sidebar follows through herdr's events and the snapshot re-read.
 */
@Injectable({ providedIn: "root" })
export class SpaceActions {
  private readonly bridge = inject(TauriBridge);
  private readonly store = inject(HerdrStore);

  private readonly _open = signal(false);
  private readonly _error = signal<string | null>(null);
  private readonly _busy = signal(false);
  private readonly _dirs = signal<string[]>([]);
  readonly open = this._open.asReadonly();
  /** Why the last attempt failed; shown in the dialog. */
  readonly error = this._error.asReadonly();
  readonly busy = this._busy.asReadonly();
  /** Directories matching the typed path. */
  readonly dirs = this._dirs.asReadonly();

  ask(): void {
    this._error.set(null);
    this._dirs.set([]);
    this._open.set(true);
  }

  cancel(): void {
    this._open.set(false);
    this._error.set(null);
  }

  /** Looks up directories for the typed path (completion). */
  async complete(input: string): Promise<void> {
    try {
      this._dirs.set(await this.bridge.invoke<string[]>("list_dirs", { input }));
    } catch {
      this._dirs.set([]);
    }
  }

  /** The directory chosen in the native folder dialog, or `null` when cancelled. */
  async pick(): Promise<string | null> {
    try {
      return await this.bridge.pickDirectory();
    } catch (err) {
      this._error.set(String(err));
      return null;
    }
  }

  /** Creates (or focuses) the space of `cwd`; closes the dialog on success. */
  async create(cwd: string, label: string): Promise<SpaceOutcome | undefined> {
    this._error.set(null);
    this._busy.set(true);
    try {
      const outcome = await this.bridge.invoke<SpaceOutcome>("create_space", {
        cwd: cwd.trim(),
        label: label.trim() || null,
      });
      this._open.set(false);
      await this.store.refresh();
      return outcome;
    } catch (err) {
      this._error.set(String(err));
      return undefined;
    } finally {
      this._busy.set(false);
    }
  }
}
