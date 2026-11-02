import { Injectable, inject, signal } from "@angular/core";
import {
  closeWarning,
  paneDir,
  rowName,
  workspaceDirs,
  type Pane,
  type SplitDirection,
} from "./herdr";
import { HerdrStore } from "./herdr-store";
import { TauriBridge } from "./tauri";

/** A dialog the user has to answer before a pane is created or closed. */
export type PaneDialogState =
  | { kind: "new"; workspaceId: string; dirs: string[]; cwd: string }
  | { kind: "close"; paneId: string; name: string; warning: string };

/**
 * Creates, splits and closes herdr panes. herdr owns the result: each call only asks herdr, then
 * re-reads the snapshot (the events do the same, debounced). Closing always asks first.
 */
@Injectable({ providedIn: "root" })
export class PaneActions {
  private readonly bridge = inject(TauriBridge);
  private readonly store = inject(HerdrStore);

  private readonly _dialog = signal<PaneDialogState | null>(null);
  private readonly _error = signal<string | null>(null);
  /** The question waiting for an answer; `null` when none. */
  readonly dialog = this._dialog.asReadonly();
  /** Why the last action failed; cleared by the next one. */
  readonly error = this._error.asReadonly();

  /** Asks where to start a new terminal in `workspaceId` (default: the first workspace). */
  askNew(workspaceId?: string): void {
    const id = workspaceId ?? this.store.snapshot().workspaces[0]?.workspace_id;
    if (!id) return;
    const dirs = workspaceDirs(this.store.snapshot(), id);
    this._error.set(null);
    this._dialog.set({ kind: "new", workspaceId: id, dirs, cwd: dirs[0] ?? "" });
  }

  /** Asks for confirmation to close a pane. */
  askClose(paneId: string): void {
    const pane = this.store.pane(paneId);
    const tab = this.store.snapshot().tabs.find((t) => t.tab_id === pane?.tab_id);
    this._error.set(null);
    this._dialog.set({
      kind: "close",
      paneId,
      name: pane ? rowName(pane, tab?.label) : paneId,
      warning: closeWarning(pane),
    });
  }

  cancel(): void {
    this._dialog.set(null);
  }

  dismissError(): void {
    this._error.set(null);
  }

  /** Creates a terminal in the workspace of the open dialog; resolves to the new pane. */
  async confirmNew(cwd: string): Promise<Pane | undefined> {
    const dialog = this._dialog();
    if (dialog?.kind !== "new") return undefined;
    this._dialog.set(null);
    return this.create(() =>
      this.bridge.invoke<string | null>("terminal_create", {
        workspaceId: dialog.workspaceId,
        cwd: cwd.trim() || null,
      }),
    );
  }

  /** Splits `paneId`; the new shell starts in the pane's directory. Resolves to the new pane. */
  async split(paneId: string, direction: SplitDirection): Promise<Pane | undefined> {
    const pane = this.store.pane(paneId);
    return this.create(() =>
      this.bridge.invoke<string | null>("pane_split", {
        target: paneId,
        direction,
        cwd: (pane && paneDir(pane)) ?? null,
      }),
    );
  }

  /** Closes the pane of the open dialog; resolves to its id once herdr closed it. */
  async confirmClose(): Promise<string | undefined> {
    const dialog = this._dialog();
    if (dialog?.kind !== "close") return undefined;
    this._dialog.set(null);
    this._error.set(null);
    try {
      await this.bridge.invoke("pane_close", { paneId: dialog.paneId });
    } catch (err) {
      this._error.set(String(err));
      return undefined;
    }
    await this.store.refresh();
    return dialog.paneId;
  }

  private async create(call: () => Promise<string | null>): Promise<Pane | undefined> {
    this._error.set(null);
    try {
      const id = await call();
      await this.store.refresh();
      return id ? this.store.pane(id) : undefined;
    } catch (err) {
      this._error.set(String(err));
      return undefined;
    }
  }
}
