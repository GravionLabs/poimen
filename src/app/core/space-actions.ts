import { Injectable, inject, signal } from "@angular/core";
import {
  agentKind,
  orderSpaces,
  orderTabs,
  pathSegments,
  reorderIds,
  type Snapshot,
} from "./snapshot";
import { SpacesStore } from "./spaces-store";
import { Machines } from "./machines";
import type { TemplateHint } from "./space-templates";
import { TauriBridge } from "./tauri";
import { UiState } from "./ui-state";

/** Answer of the `create_space` command. */
export interface SpaceOutcome {
  space_id: string;
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
  /** Set when no space is created: "trust" calls it with the answer instead of `create_space`. */
  resume?: (decision: TrustDecision) => Promise<void>;
  /** "without" calls `resume` too (with the `without` decision). */
  without?: boolean;
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

/** A space waiting for its new name. */
export interface SpaceRenaming {
  workspaceId: string;
  /** The label when the dialog opened. */
  current: string;
}

/** The space to focus after `workspaceId` closed: the next one, else the previous; `null` if none. */
export function neighbourSpace(
  snapshot: Snapshot,
  workspaceId: string,
  order: readonly string[] = [],
): string | null {
  const ids = orderSpaces(snapshot.spaces, order).map((w) => w.space_id);
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
  private readonly store = inject(SpacesStore);
  private readonly ui = inject(UiState);
  private readonly machines = inject(Machines, { optional: true });

  private readonly _open = signal(false);
  private readonly _error = signal<string | null>(null);
  private readonly _busy = signal(false);
  private readonly _dirs = signal<string[]>([]);
  private readonly _warning = signal<string | null>(null);
  private readonly _closing = signal<SpaceClosing | null>(null);
  private readonly _closeError = signal<string | null>(null);
  private readonly _renaming = signal<SpaceRenaming | null>(null);
  private readonly _renameError = signal<string | null>(null);
  /** The space of the open rename dialog; `null` when none. */
  readonly renaming = this._renaming.asReadonly();
  /** Why the last rename failed (blank name or herdr's error); the old label stays. */
  readonly renameError = this._renameError.asReadonly();
  private readonly _closingIds = signal<ReadonlySet<string>>(new Set());
  /** Spaces whose close was confirmed and is not finished: shown as "Closing…", actions disabled. */
  readonly closingIds = this._closingIds.asReadonly();
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

  /**
   * Asks for the trust of `request` (a palette entry, a rebuild); "trust" calls `resume`, and so
   * does "without" when `without` is set (a rebuild without project commands); the rest nothing.
   */
  askTrust(
    request: TrustRequest,
    resume: (decision: TrustDecision) => Promise<void>,
    without = false,
  ): void {
    this._trust.set({ cwd: request.root, label: "", command: null, request, resume, without });
  }

  /** Shows `message` in the status bar like a layout warning (a failed step of a task). */
  notify(message: string): void {
    this._warning.set(message);
  }

  /** An existing space was focused instead of a new one: select it and say so. */
  private announceExisting(id: string): void {
    this.ui.setSplitWorkspace(id);
    const label = this.store.snapshot().spaces.find((s) => s.space_id === id)?.label ?? id;
    this._warning.set(`Opened the existing space "${label}".`);
  }

  ask(): void {
    this._template.set(null);
    this._error.set(null);
    this._dirs.set([]);
    this._open.set(true);
  }

  cancel(): void {
    this._open.set(false);
    this._error.set(null);
  }

  private readonly _template = signal<TemplateHint | null>(null);
  /** The space template that applies to the directory typed in the dialog; `null`: the default. */
  readonly template = this._template.asReadonly();
  private templateSeq = 0;

  /** Looks up the template for the typed or picked directory (the newest lookup wins). */
  async lookupTemplate(dir: string): Promise<void> {
    const seq = ++this.templateSeq;
    const cwd = dir.trim();
    let hint: TemplateHint | null = null;
    if (cwd) {
      try {
        const raw = await this.bridge.invoke<Partial<TemplateHint> | null>(
          "resolve_space_template",
          { cwd },
        );
        if (raw && typeof raw.name === "string" && typeof raw.agent === "string") {
          hint = { name: raw.name, agent: raw.agent, default: raw.default === true };
        }
      } catch {
        hint = null;
      }
    }
    if (seq === this.templateSeq) this._template.set(hint);
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
      if (!outcome.created) this.announceExisting(outcome.space_id);
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
    if (pending.resume) {
      if (answer === "trust") await pending.resume({ kind: "trust", hash: pending.request.hash });
      else if (answer === "without" && pending.without) await pending.resume({ kind: "without" });
      return undefined;
    }
    if (answer === "cancel") return undefined;
    const decision: TrustDecision =
      answer === "trust" ? { kind: "trust", hash: pending.request.hash } : { kind: "without" };
    return this.create(pending.cwd, pending.label, decision, pending.command, pending.after);
  }

  /** Opens the rename dialog for a space, prefilled with its label. */
  askRename(workspaceId: string): void {
    const space = this.store.snapshot().spaces.find((s) => s.space_id === workspaceId);
    if (!space) return;
    this._renameError.set(null);
    this._renaming.set({ workspaceId, current: space.label });
  }

  cancelRename(): void {
    this._renaming.set(null);
    this._renameError.set(null);
  }

  clearRenameError(): void {
    this._renameError.set(null);
  }

  /** Renames the space of the open dialog; on failure the dialog stays open with the error. */
  async confirmRename(name: string): Promise<boolean> {
    const dialog = this._renaming();
    if (!dialog) return false;
    const ok = await this.rename(dialog.workspaceId, name);
    if (ok) this._renaming.set(null);
    return ok;
  }

  /**
   * Renames a space (`workspace.rename`): a blank name is rejected without calling herdr, an
   * unchanged one is a no-op success. Resolves to whether the space now has that label.
   */
  async rename(workspaceId: string, name: string): Promise<boolean> {
    const label = name.trim();
    this._renameError.set(null);
    if (!label) {
      this._renameError.set("The name of a space cannot be empty.");
      return false;
    }
    const space = this.store.snapshot().spaces.find((s) => s.space_id === workspaceId);
    if (space?.label === label) return true;
    const blocked = this.machines?.blockedReason(workspaceId) ?? null;
    if (blocked) {
      this._renameError.set(blocked);
      return false;
    }
    try {
      await this.bridge.invoke("space_rename", { spaceId: workspaceId, label });
    } catch (err) {
      this._renameError.set(String(err));
      return false;
    }
    await this.store.refresh();
    return true;
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

  /**
   * Drops `sourceId` before or after `targetId` in the user's space order. Nothing happens for the
   * same space, an unknown id or a drop that leaves the order as it is.
   */
  reorder(sourceId: string, targetId: string, after: boolean): void {
    const ids = this.store.orderedSpaces().map((s) => s.space_id);
    const next = reorderIds(ids, sourceId, targetId, after);
    if (next) this.ui.setSpaceOrder(next);
  }

  /**
   * Drops tab `sourceTab` before or after `targetTab` inside space `spaceId`. Tabs of another space,
   * unknown tabs, the same tab or a drop that leaves the order as it is change nothing.
   */
  reorderTabs(spaceId: string, sourceTab: string, targetTab: string, after: boolean): void {
    const tabs = this.store.snapshot().tabs.filter((t) => t.space_id === spaceId);
    const ids = orderTabs(tabs, this.store.tabOrder()[spaceId]).map((t) => t.tab_id);
    const next = reorderIds(ids, sourceTab, targetTab, after);
    if (next) this.ui.setTabOrder(spaceId, next);
  }

  /** Moves a space one step up (`-1`) or down (`1`); at either end nothing happens. */
  shift(spaceId: string, delta: -1 | 1): void {
    const ids = this.store.orderedSpaces().map((s) => s.space_id);
    const target = ids[ids.indexOf(spaceId) + delta];
    if (target) this.reorder(spaceId, target, delta === 1);
  }

  dismissCloseError(): void {
    this._closeError.set(null);
  }

  private async close(workspaceId: string): Promise<string[] | undefined> {
    if (this._closingIds().has(workspaceId)) return undefined;
    const blocked = this.machines?.blockedReason(workspaceId) ?? null;
    if (blocked) {
      this._closeError.set(blocked);
      return undefined;
    }
    const snapshot = this.store.snapshot();
    const paneIds = snapshot.terminals
      .filter((p) => p.space_id === workspaceId)
      .map((p) => p.terminal_id);
    this.markClosing(workspaceId, true);
    try {
      await this.bridge.invoke("space_close", {
        spaceId: workspaceId,
        focus: neighbourSpace(snapshot, workspaceId, this.store.spaceOrder()),
      });
    } catch (err) {
      this._closeError.set(String(err));
      this.markClosing(workspaceId, false);
      return undefined;
    }
    try {
      // A fresh snapshot: without the space, the busy state ends.
      await this.store.refresh();
    } finally {
      this.markClosing(workspaceId, false);
    }
    return paneIds;
  }

  private markClosing(workspaceId: string, on: boolean): void {
    this._closingIds.update((ids) => {
      const next = new Set(ids);
      if (on) next.add(workspaceId);
      else next.delete(workspaceId);
      return next;
    });
  }
}
