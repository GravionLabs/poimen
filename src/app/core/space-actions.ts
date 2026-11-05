import { Injectable, inject, signal } from "@angular/core";
import { agentKind, pathSegments, type Snapshot } from "./herdr";
import { HerdrStore } from "./herdr-store";
import { TauriBridge } from "./tauri";

/** Answer of the `create_space` command. */
export interface SpaceOutcome {
  workspace_id: string;
  /** False when an existing space for the directory was focused instead. */
  created: boolean;
  /** Why the pane layout of a new space was not (fully) applied, or the setup not started. */
  warnings?: string[];
  /** The `setup` command of the project file runs in a `setup` tab of the new space. */
  setup_started?: boolean;
  /** Set instead of creating anything: the project file is not trusted yet. */
  trust?: TrustRequest;
}

/** A command of a project file (wire shape of `trust.rs`). */
export interface ListedCommand {
  what: string;
  command: string;
}

/** The project commands waiting for the user's trust (wire shape of `trust.rs`). */
export interface TrustRequest {
  root: string;
  files: string[];
  hash: string;
  commands: ListedCommand[];
}

/** The answer sent back to `create_space`. */
export type TrustDecision = { kind: "trust"; hash: string } | { kind: "without" };

/** A space waiting for the trust decision of its repository. */
export interface TrustPending {
  cwd: string;
  label: string;
  /** The command a task runs in the root pane of the new space. */
  command: string | null;
  /** Runs once the space exists, also after the trust answer (a task tab in the space). */
  after?: (outcome: SpaceOutcome) => Promise<void>;
  request: TrustRequest;
}

/** Last path segment: the default label of a space for `path`. */
export function dirName(path: string): string {
  return pathSegments(path.trim()).pop() ?? "";
}

/** A space waiting for the user to confirm its close. */
export interface SpaceClosing {
  workspaceId: string;
  label: string;
  panes: number;
  /** What the close ends: open panes and running agents. */
  warning: string;
}

/** The space to focus after `workspaceId` closed: the next one, else the previous; `null` if none. */
export function neighbourSpace(snapshot: Snapshot, workspaceId: string): string | null {
  const ids = [...snapshot.spaces].sort((a, b) => a.number - b.number).map((w) => w.space_id);
  const i = ids.indexOf(workspaceId);
  if (i < 0) return null;
  return ids[i + 1] ?? ids[i - 1] ?? null;
}

/** What closing the space ends, for the confirmation. */
export function closeSpaceWarning(snapshot: Snapshot, workspaceId: string): string {
  const panes = snapshot.terminals.filter((p) => p.space_id === workspaceId);
  const agents = panes.filter((p) => agentKind(p)).length;
  const noun = (n: number, word: string) => `${n} ${word}${n === 1 ? "" : "s"}`;
  return agents > 0
    ? `Closing the space ends ${noun(panes.length, "pane")}, ${noun(agents, "agent")} among them.`
    : `Closing the space ends ${noun(panes.length, "pane")}.`;
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
  private readonly _warning = signal<string | null>(null);
  private readonly _closing = signal<SpaceClosing | null>(null);
  private readonly _closeError = signal<string | null>(null);
  /** The space waiting for a close confirmation; `null` when none. */
  readonly closing = this._closing.asReadonly();
  private readonly _trust = signal<TrustPending | null>(null);
  /** The project file waiting for a trust decision; nothing was created yet. */
  readonly trust = this._trust.asReadonly();
  /** herdr's error of the last failed close; the space is untouched. */
  readonly closeError = this._closeError.asReadonly();
  readonly open = this._open.asReadonly();
  /** Layout problem of the last created space (status bar); `null` when there was none. */
  readonly warning = this._warning.asReadonly();
  /** Why the last attempt failed; shown in the dialog. */
  readonly error = this._error.asReadonly();
  readonly busy = this._busy.asReadonly();
  /** Directories matching the typed path. */
  readonly dirs = this._dirs.asReadonly();

  /** Shows `message` in the status bar like a layout warning (a failed step of a task). */
  notify(message: string): void {
    this._warning.set(message);
  }

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
  async create(
    cwd: string,
    label: string,
    decision: TrustDecision | null = null,
    command: string | null = null,
    after?: (outcome: SpaceOutcome) => Promise<void>,
  ): Promise<SpaceOutcome | undefined> {
    this._error.set(null);
    this._busy.set(true);
    try {
      const outcome = await this.bridge.invoke<SpaceOutcome>("create_space", {
        cwd: cwd.trim(),
        label: label.trim() || null,
        decision,
        command,
      });
      this._open.set(false);
      if (outcome.trust) {
        // Nothing exists yet: ask, then call again with the answer.
        this._trust.set({
          cwd: cwd.trim(),
          label: label.trim(),
          command,
          after,
          request: outcome.trust,
        });
        return undefined;
      }
      this._trust.set(null);
      const warnings = outcome.warnings ?? [];
      this._warning.set(warnings.length > 0 ? warnings.join("; ") : null);
      for (const w of warnings) console.warn(`poimen space layout: ${w}`);
      await this.store.refresh();
      if (after) await after(outcome);
      return outcome;
    } catch (err) {
      this._error.set(String(err));
      return undefined;
    } finally {
      this._busy.set(false);
    }
  }

  /**
   * The answer to the trust dialog. "trust" and "without" create the space (the first stores the
   * hash of the listed file, valid only if the file is unchanged); "cancel" creates nothing.
   * Resolves to the outcome when a space was created or focused.
   */
  async answerTrust(answer: "trust" | "without" | "cancel"): Promise<SpaceOutcome | undefined> {
    const pending = this._trust();
    if (!pending) return undefined;
    this._trust.set(null);
    if (answer === "cancel") return undefined;
    const decision: TrustDecision =
      answer === "trust" ? { kind: "trust", hash: pending.request.hash } : { kind: "without" };
    return this.create(pending.cwd, pending.label, decision, pending.command, pending.after);
  }

  /**
   * Closes a space (`workspace.close`). A space with panes asks first (see {@link closing}) and
   * resolves to `undefined`; an empty one closes right away. Resolves to the ids of the panes that
   * were closed once herdr closed the space, else `undefined` (a failure sets {@link closeError}).
   */
  async requestClose(workspaceId: string): Promise<string[] | undefined> {
    const snapshot = this.store.snapshot();
    const space = snapshot.spaces.find((w) => w.space_id === workspaceId);
    if (!space) return undefined;
    this._closeError.set(null);
    const panes = snapshot.terminals.filter((p) => p.space_id === workspaceId);
    if (panes.length === 0) return this.close(workspaceId);
    this._closing.set({
      workspaceId,
      label: space.label,
      panes: panes.length,
      warning: closeSpaceWarning(snapshot, workspaceId),
    });
    return undefined;
  }

  /** Closes the space of the open confirmation. */
  async confirmClose(): Promise<string[] | undefined> {
    const closing = this._closing();
    if (!closing) return undefined;
    this._closing.set(null);
    return this.close(closing.workspaceId);
  }

  cancelClose(): void {
    this._closing.set(null);
  }

  dismissCloseError(): void {
    this._closeError.set(null);
  }

  private async close(workspaceId: string): Promise<string[] | undefined> {
    const snapshot = this.store.snapshot();
    const paneIds = snapshot.terminals
      .filter((p) => p.space_id === workspaceId)
      .map((p) => p.terminal_id);
    try {
      await this.bridge.invoke("space_close", {
        workspaceId,
        focus: neighbourSpace(snapshot, workspaceId),
      });
    } catch (err) {
      this._closeError.set(String(err));
      return undefined;
    }
    await this.store.refresh();
    return paneIds;
  }
}
