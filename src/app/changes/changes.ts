import { Injectable, computed, effect, inject, signal, untracked } from "@angular/core";
import { EditorTabs } from "../core/editor-tabs";
import { Attention } from "../core/attention";
import { parseHarnesses } from "../core/settings";
import { SpacesStore } from "../core/spaces-store";
import { paneDir } from "../core/snapshot";
import { TauriBridge } from "../core/tauri";
import { rangeBetween, sectionsOf, type SectionId } from "./change-sections";

/** What is compared: the working tree against the merge base with the base branch, or `HEAD`. */
export type ChangesScope = "base" | "uncommitted";
export const CHANGES_SCOPES: readonly ChangesScope[] = ["base", "uncommitted"];

export type DiffLayout = "inline" | "split";

/** A request to scroll to the next or previous change; each press is a new object. */
export interface DiffJump {
  step: "next" | "previous";
}

/** Width under which the diff falls back to inline whatever the user chose. */
export const NARROW_WIDTH = 900;

/** The layout shown: side by side needs room, so a narrow tab is inline. */
export function effectiveLayout(chosen: DiffLayout, width: number): DiffLayout {
  return width < NARROW_WIDTH ? "inline" : chosen;
}

export type ChangeStatus = "added" | "modified" | "deleted" | "renamed" | "untracked";

/** One entry of `changes_list` (`ChangedFile` of `changes.rs`). */
export interface ChangedFile {
  path: string;
  old_path: string | null;
  status: ChangeStatus;
  additions: number;
  deletions: number;
  binary: boolean;
  /** The index holds a change for the file. */
  staged: boolean;
  /** The file has an unresolved merge conflict. */
  conflicted: boolean;
}

/** The answer of `changes_list`: `base` is the ref used, `from` the commit the old content comes from. */
export interface ChangeList {
  base: string;
  scope: ChangesScope;
  from: string;
  files: ChangedFile[];
}

/** The answer of `changes_file`. */
export type FileContent =
  | { state: "text"; old: string; new: string }
  | { state: "binary" }
  | { state: "too_large" };

/** The one-letter mark of a status, as in `git status`. */
export function statusLetter(status: ChangeStatus): string {
  switch (status) {
    case "added":
      return "A";
    case "deleted":
      return "D";
    case "renamed":
      return "R";
    case "untracked":
      return "U";
    default:
      return "M";
  }
}

/** The paths an action on the files touches: both paths of a rename. */
function pathsOf(files: readonly ChangedFile[]): string[] {
  return [...new Set(files.flatMap((f) => (f.old_path ? [f.old_path, f.path] : [f.path])))];
}

/** Totals over a file list. */
export function totals(files: readonly ChangedFile[]): {
  additions: number;
  deletions: number;
  files: number;
} {
  return {
    files: files.length,
    additions: files.reduce((n, f) => n + f.additions, 0),
    deletions: files.reduce((n, f) => n + f.deletions, 0),
  };
}

/** `dir` is `root` or below it. */
export function isWithin(dir: string, root: string): boolean {
  const base = root.endsWith("/") ? root.slice(0, -1) : root;
  return dir === base || dir.startsWith(`${base}/`);
}

/**
 * The state of one Changes tab: the changed files of a directory, the selected file and its
 * content. Provided per tab (component providers). Reads on start, on `refresh` and when an agent
 * of the directory turns done; never polls.
 */
@Injectable()
export class Changes {
  private readonly bridge = inject(TauriBridge);
  private readonly store = inject(SpacesStore);
  private readonly attention = inject(Attention);
  private readonly tabs = inject(EditorTabs);

  readonly dir = signal("");
  readonly scope = signal<ChangesScope>("base");
  readonly list = signal<ChangeList | null>(null);
  readonly error = signal<string | null>(null);
  readonly loading = signal(false);
  readonly selected = signal<string | null>(null);
  readonly content = signal<FileContent | null>(null);
  readonly contentError = signal<string | null>(null);

  readonly commitMessage = signal("");
  /** A stage, commit or push is running. */
  readonly busy = signal(false);
  /** The output or error of the last stage, commit or push. */
  readonly actionResult = signal<{ ok: boolean; text: string } | null>(null);

  /** The sections of the uncommitted scope that are collapsed. */
  readonly collapsed = signal<ReadonlySet<SectionId>>(new Set());
  /** The files picked with Shift-click (empty: only the opened file counts). */
  readonly picked = signal<ReadonlySet<string>>(new Set());
  private anchor: string | null = null;

  readonly files = computed(() => this.list()?.files ?? []);
  readonly totals = computed(() => totals(this.files()));
  readonly stagedCount = computed(() => this.files().filter((f) => f.staged).length);
  /** Something is staged, the message is not blank and no action is running. */
  readonly canCommit = computed(
    () => !this.busy() && this.stagedCount() > 0 && this.commitMessage().trim() !== "",
  );
  /** The harness that drafts commit messages (first available with `printArgs`); none hides the button. */
  readonly messageHarness = signal<string | null>(null);
  /** A commit message is being generated. */
  readonly generating = signal(false);
  private generation = 0;
  private currentGeneration: string | null = null;

  readonly selectedFile = computed(() => this.files().find((f) => f.path === this.selected()));

  private listRun = 0;
  private fileRun = 0;

  constructor() {
    // "Show in Changes" for a file of this directory: read again, the refresh selects it.
    effect(() => {
      const wanted = this.tabs.changesFocus();
      untracked(() => {
        if (wanted && wanted.dir === this.dir()) void this.refresh();
      });
    });
    // An agent of this directory finished: its edits are what the list should show now.
    effect(() => {
      const changes = this.attention.lastChanges();
      untracked(() => {
        const dir = this.dir();
        if (!dir) return;
        const panes = this.store.panes();
        const mine = changes.some((c) => {
          const pane = panes.get(c.paneId);
          const at = pane ? paneDir(pane) : undefined;
          return c.status === "done" && !!at && isWithin(at, dir);
        });
        if (mine) void this.refresh();
      });
    });
  }

  /** Reads which harness can draft a commit message (`printArgs`, program found). */
  async loadMessageHarness(): Promise<void> {
    try {
      const harnesses = parseHarnesses(await this.bridge.invoke<unknown>("get_harnesses"));
      const found = harnesses.find(
        (h) => h.available && h.printArgs.some((a) => a.includes("{prompt}")),
      );
      this.messageHarness.set(found?.id ?? null);
    } catch {
      this.messageHarness.set(null);
    }
  }

  /**
   * Drafts a commit message from the staged diff into the box. Never commits; an error or a
   * cancel leaves the box as it was and the error is shown.
   */
  async generateMessage(): Promise<void> {
    const harness = this.messageHarness();
    if (!harness || this.generating() || this.busy() || this.stagedCount() === 0) return;
    const id = `commit-message-${++this.generation}`;
    this.generating.set(true);
    this.currentGeneration = id;
    try {
      const text = await this.bridge.invoke<string>("changes_generate_message", {
        dir: this.dir(),
        harness,
        id,
      });
      if (this.currentGeneration !== id) return;
      this.commitMessage.set(text);
      this.actionResult.set(null);
    } catch (error) {
      if (this.currentGeneration !== id) return;
      this.actionResult.set({ ok: false, text: String(error) });
    } finally {
      if (this.currentGeneration === id) {
        this.generating.set(false);
        this.currentGeneration = null;
      }
    }
  }

  /** Stops the running generation; the box stays as it was. */
  async cancelGenerate(): Promise<void> {
    const id = this.currentGeneration;
    if (!id) return;
    this.currentGeneration = null;
    this.generating.set(false);
    try {
      await this.bridge.invoke("changes_generate_cancel", { id });
    } catch {
      // The call ends by its own deadline.
    }
  }

  /** Starts following `dir` (the tab's directory) and reads it. */
  follow(dir: string): Promise<void> {
    if (this.dir() !== dir) void this.cancelGenerate();
    this.dir.set(dir);
    void this.loadMessageHarness();
    return this.refresh();
  }

  setScope(scope: ChangesScope): Promise<void> {
    if (scope === this.scope()) return Promise.resolve();
    this.scope.set(scope);
    return this.refresh();
  }

  /** Reads the list again; keeps the selection when its file is still changed, else selects the first. */
  async refresh(): Promise<void> {
    const dir = this.dir();
    if (!dir) return;
    const run = ++this.listRun;
    this.loading.set(true);
    try {
      const list = await this.bridge.invoke<ChangeList>("changes_list", {
        dir,
        scope: this.scope(),
      });
      if (run !== this.listRun) return;
      this.list.set(list);
      this.error.set(null);
      const present = new Set(list.files.map((f) => f.path));
      this.picked.set(new Set([...this.picked()].filter((p) => present.has(p))));
      const wanted = this.tabs.takeChangesFocus(dir);
      const keep = list.files.some((f) => f.path === this.selected());
      const pick = wanted && list.files.some((f) => f.path === wanted) ? wanted : undefined;
      await this.select(pick ?? (keep ? this.selected() : (list.files[0]?.path ?? null)), true);
    } catch (error) {
      if (run !== this.listRun) return;
      this.error.set(String(error));
    } finally {
      if (run === this.listRun) this.loading.set(false);
    }
  }

  /** Selects a file and reads its content (`force` reads again though it is already selected). */
  async select(path: string | null, force = false): Promise<void> {
    if (!force && path === this.selected()) return;
    this.selected.set(path);
    const run = ++this.fileRun;
    if (path === null) {
      this.content.set(null);
      this.contentError.set(null);
      return;
    }
    try {
      const content = await this.bridge.invoke<FileContent>("changes_file", {
        dir: this.dir(),
        scope: this.scope(),
        path,
      });
      if (run !== this.fileRun) return;
      this.content.set(content);
      this.contentError.set(null);
    } catch (error) {
      if (run !== this.fileRun) return;
      this.content.set(null);
      this.contentError.set(String(error));
    }
  }

  /** Stages or unstages a file (both paths of a rename), then reads the list again. */
  setStaged(file: ChangedFile, staged: boolean): Promise<void> {
    return this.setStagedMany([file], staged);
  }

  /** Stages or unstages files (both paths of a renamed one) in one call, then reads the list again. */
  async setStagedMany(files: readonly ChangedFile[], staged: boolean): Promise<void> {
    const paths = pathsOf(files);
    if (!paths.length) return;
    await this.act(async () => {
      await this.bridge.invoke("changes_stage", {
        dir: this.dir(),
        paths,
        stage: staged,
      });
      return null;
    });
  }

  /** Throws away the changes of the files (the view asked first), then reads the list again. */
  async discard(files: readonly ChangedFile[]): Promise<void> {
    const paths = pathsOf(files);
    if (!paths.length) return;
    await this.act(async () => {
      await this.bridge.invoke("changes_discard", { dir: this.dir(), paths });
      return null;
    });
  }

  /** The files an action on `file` applies to: the picked ones when it is one of them, else it alone. */
  targets(file: ChangedFile): ChangedFile[] {
    const picked = this.picked();
    if (picked.size > 1 && picked.has(file.path)) {
      return this.listed().filter((f) => picked.has(f.path));
    }
    return [file];
  }

  /** The files in the order the list shows them (sections in the uncommitted scope, collapsed ones left out). */
  listed(): ChangedFile[] {
    if (this.scope() !== "uncommitted") return this.files();
    const collapsed = this.collapsed();
    return sectionsOf(this.files())
      .filter((s) => !collapsed.has(s.id))
      .flatMap((s) => s.files);
  }

  /** Click on a row: Shift extends the pick from the last clicked row, a plain click resets it. */
  pick(path: string, shift: boolean): void {
    if (shift && this.anchor) {
      const order = this.listed().map((f) => f.path);
      this.picked.set(new Set(rangeBetween(order, this.anchor, path)));
    } else {
      this.anchor = path;
      this.picked.set(new Set());
    }
  }

  clearPicked(): void {
    this.picked.set(new Set());
  }

  toggleSection(id: SectionId): void {
    const next = new Set(this.collapsed());
    if (!next.delete(id)) next.add(id);
    this.collapsed.set(next);
    const shown = new Set(this.listed().map((f) => f.path));
    this.picked.set(new Set([...this.picked()].filter((p) => shown.has(p))));
  }

  /** Commits the staged files with the message; the message is cleared on success. */
  async commit(): Promise<void> {
    if (!this.canCommit()) return;
    await this.act(async () => {
      const out = await this.bridge.invoke<string>("changes_commit", {
        dir: this.dir(),
        message: this.commitMessage(),
      });
      this.commitMessage.set("");
      return out || "Committed.";
    });
  }

  /** Pushes the current branch; the output of git (or the refusal) is shown. */
  async push(): Promise<void> {
    if (this.busy()) return;
    await this.act(async () => {
      const out = await this.bridge.invoke<string>("changes_push", {
        dir: this.dir(),
      });
      return out || "Pushed.";
    });
  }

  /** Runs a git action, shows its output or error, and reads the list again. */
  private async act(action: () => Promise<string | null>): Promise<void> {
    if (!this.dir()) return;
    this.busy.set(true);
    try {
      const text = await action();
      this.actionResult.set(text === null ? null : { ok: true, text });
    } catch (error) {
      this.actionResult.set({ ok: false, text: String(error) });
    } finally {
      this.busy.set(false);
    }
    await this.refresh();
  }

  /** Moves the selection by `delta` files (clamped). */
  move(delta: number): Promise<void> {
    const files = this.listed();
    if (!files.length) return Promise.resolve();
    const at = files.findIndex((f) => f.path === this.selected());
    const next = Math.min(files.length - 1, Math.max(0, (at < 0 ? 0 : at) + delta));
    return this.select(files[next].path);
  }
}
