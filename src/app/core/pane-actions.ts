import { Injectable, inject, signal } from "@angular/core";
import {
  agentKind,
  closeWarning,
  olderHost,
  paneDir,
  paneSuccessor,
  rowName,
  spaceContaining,
  spaceRoot,
  workspaceDirs,
  type Terminal,
  type SplitDirection,
} from "./herdr";
import { HerdrStore } from "./herdr-store";
import { SpaceActions } from "./space-actions";
import { TauriBridge } from "./tauri";

/** A dialog the user has to answer before a pane is created or closed. */
export type PaneDialogState =
  | { kind: "new"; workspaceId: string; dirs: string[]; cwd: string }
  | { kind: "close"; paneId: string; name: string; warning: string }
  | { kind: "restart"; paneId: string; name: string; have: string; bundled: string }
  | { kind: "agent"; paneId: string; name: string }
  | { kind: "rename"; paneId: string; agent: boolean; current: string };

/**
 * A pane that was closed, and the leaf of its split tab that takes over its editor tab (resolved
 * before the close, since the snapshot read after it no longer has the pane); none for the last leaf.
 */
export interface PaneClosed {
  paneId: string;
  successor?: string;
}

/**
 * Creates, splits and closes herdr panes. herdr owns the result: each call only asks herdr, then
 * re-reads the snapshot (the events do the same, debounced). Closing always asks first.
 */
@Injectable({ providedIn: "root" })
export class PaneActions {
  private readonly bridge = inject(TauriBridge);
  private readonly store = inject(HerdrStore);
  private readonly spaces = inject(SpaceActions);

  /** Ratios waiting behind a request in flight, per `tab/split`; `null` = nothing waiting. */
  private readonly resizing = new Map<string, number | null>();
  private readonly _dialog = signal<PaneDialogState | null>(null);
  private readonly _error = signal<string | null>(null);
  /** The question waiting for an answer; `null` when none. */
  readonly dialog = this._dialog.asReadonly();
  /** Why the last action failed; cleared by the next one. */
  readonly error = this._error.asReadonly();

  /** Asks where to start a new terminal in `workspaceId` (default: the first workspace). */
  askNew(workspaceId?: string): void {
    const id = workspaceId ?? this.store.snapshot().spaces[0]?.space_id;
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

  /** Asks for confirmation to restart a terminal on an older PTY host on the bundled one (#606). */
  askRestart(paneId: string): void {
    const pane = this.store.pane(paneId);
    const host = olderHost(pane);
    if (!pane || !host) return;
    const tab = this.store.snapshot().tabs.find((t) => t.tab_id === pane.tab_id);
    this._error.set(null);
    this._dialog.set({
      kind: "restart",
      paneId,
      name: rowName(pane, tab?.label),
      have: host.have,
      bundled: host.bundled,
    });
  }

  /** Asks which agent to start (and what to call it) in the shell of `paneId`. */
  askStartAgent(paneId: string): void {
    const pane = this.store.pane(paneId);
    const tab = this.store.snapshot().tabs.find((t) => t.tab_id === pane?.tab_id);
    this._error.set(null);
    this._dialog.set({
      kind: "agent",
      paneId,
      name: pane ? rowName(pane, tab?.label) : paneId,
    });
  }

  /** Asks for a new name: the agent's name for an agent pane, else the pane label. */
  askRename(paneId: string): void {
    const pane = this.store.pane(paneId);
    const tab = this.store.snapshot().tabs.find((t) => t.tab_id === pane?.tab_id);
    this._error.set(null);
    this._dialog.set({
      kind: "rename",
      paneId,
      agent: !!pane && !!agentKind(pane),
      current: pane ? rowName(pane, tab?.label) : "",
    });
  }

  cancel(): void {
    this._dialog.set(null);
  }

  dismissError(): void {
    this._error.set(null);
  }

  /** Creates a terminal in the workspace of the open dialog; resolves to the new pane. */
  async confirmNew(cwd: string): Promise<Terminal | undefined> {
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

  /**
   * Opens a terminal whose working directory is `dir`: a new tab in the space whose root contains
   * it, else in a new space for `dir` (its first terminal). Resolves to the terminal, if any.
   */
  async openTerminalIn(dir: string): Promise<Terminal | undefined> {
    const cwd = dir.trim();
    if (!cwd) return undefined;
    const spaceId = spaceContaining(this.store.snapshot(), cwd);
    if (spaceId) {
      return this.create(() =>
        this.bridge.invoke<string | null>("terminal_create", { workspaceId: spaceId, cwd }),
      );
    }
    const outcome = await this.spaces.create(cwd, "");
    if (!outcome) return undefined;
    return spaceRoot(this.store.snapshot(), outcome.workspace_id);
  }

  /** Splits `paneId`; the new shell starts in the pane's directory. Resolves to the new pane. */
  async split(paneId: string, direction: SplitDirection): Promise<Terminal | undefined> {
    const pane = this.store.pane(paneId);
    return this.create(() =>
      this.bridge.invoke<string | null>("pane_split", {
        target: paneId,
        direction,
        cwd: (pane && paneDir(pane)) ?? null,
      }),
    );
  }

  /**
   * Sets the ratio of a split of a tab. A drag calls this on every move: while a request is in
   * flight only the latest ratio per split is kept and sent after it, so the server sees the last
   * position. Failures set `error`; the snapshot is re-read afterwards either way.
   */
  async resizeSplit(tabId: string, splitId: string, ratio: number): Promise<void> {
    const key = `${tabId}/${splitId}`;
    if (this.resizing.has(key)) {
      this.resizing.set(key, ratio);
      return;
    }
    this.resizing.set(key, null);
    let next: number | null = ratio;
    while (next !== null) {
      try {
        await this.bridge.invoke("tab_resize_split", { tabId, splitId, ratio: next });
      } catch (err) {
        this._error.set(String(err));
      }
      next = this.resizing.get(key) ?? null;
      this.resizing.set(key, null);
    }
    this.resizing.delete(key);
    await this.store.refresh();
  }

  /** Focuses a pane of a tab on the server (shared by every client). Fire and forget. */
  async focusPane(tabId: string, terminalId: string): Promise<void> {
    try {
      await this.bridge.invoke("tab_focus", { tabId, terminalId });
    } catch (err) {
      this._error.set(String(err));
    }
  }

  /** Closes the pane of the open dialog; resolves to it and its successor once herdr closed it. */
  async confirmClose(): Promise<PaneClosed | undefined> {
    const dialog = this._dialog();
    if (dialog?.kind !== "close") return undefined;
    this._dialog.set(null);
    this._error.set(null);
    const successor = paneSuccessor(this.store.snapshot(), dialog.paneId);
    try {
      await this.bridge.invoke("pane_close", { paneId: dialog.paneId });
    } catch (err) {
      this._error.set(String(err));
      return undefined;
    }
    await this.store.refresh();
    return successor ? { paneId: dialog.paneId, successor } : { paneId: dialog.paneId };
  }

  /**
   * Restarts the pane of the open restart dialog: a new terminal with the same directory, program
   * and place replaces it. Resolves to the old pane and the new one as its successor, so the
   * host hands the editor tab over.
   */
  async confirmRestart(): Promise<PaneClosed | undefined> {
    const dialog = this._dialog();
    if (dialog?.kind !== "restart") return undefined;
    this._dialog.set(null);
    this._error.set(null);
    try {
      const created = await this.bridge.invoke<string | null>("pane_restart", {
        paneId: dialog.paneId,
      });
      await this.store.refresh();
      return created ? { paneId: dialog.paneId, successor: created } : { paneId: dialog.paneId };
    } catch (err) {
      this._error.set(String(err));
      return undefined;
    }
  }

  /** Starts `kind` in the pane of the open agent dialog; the `args` field is split on whitespace into argv elements. */
  async confirmStartAgent(kind: string, name: string, args: string): Promise<boolean> {
    const dialog = this._dialog();
    if (dialog?.kind !== "agent" || !kind.trim()) return false;
    this._dialog.set(null);
    return this.run(() =>
      this.bridge.invoke("agent_start", {
        paneId: dialog.paneId,
        kind: kind.trim(),
        name: name.trim() || null,
        args: args.split(/\s+/).filter(Boolean),
      }),
    );
  }

  /** Renames the pane of the open rename dialog; an empty name clears it. */
  async confirmRename(name: string): Promise<boolean> {
    const dialog = this._dialog();
    if (dialog?.kind !== "rename") return false;
    this._dialog.set(null);
    return this.run(() =>
      dialog.agent
        ? this.bridge.invoke("agent_rename", { target: dialog.paneId, name: name.trim() || null })
        : this.bridge.invoke("pane_rename", { paneId: dialog.paneId, label: name.trim() || null }),
    );
  }

  private async run(call: () => Promise<unknown>): Promise<boolean> {
    this._error.set(null);
    try {
      await call();
    } catch (err) {
      this._error.set(String(err));
      return false;
    }
    await this.store.refresh();
    return true;
  }

  private async create(call: () => Promise<string | null>): Promise<Terminal | undefined> {
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
