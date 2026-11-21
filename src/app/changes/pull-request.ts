import { Injectable, computed, inject, signal } from "@angular/core";
import { EditorTabs } from "../core/editor-tabs";
import { TauriBridge } from "../core/tauri";
import { UiState } from "../core/ui-state";
import { GithubPulls } from "../github/pulls";
import { Changes } from "./changes";

/** `PullState` of `src-tauri/src/pull_request.rs`. */
export interface PullState {
  branch: string | null;
  base: string | null;
  pushed: boolean;
  existing: { number: number; url: string } | null;
}

interface Draft {
  title: string;
  body: string;
}

interface Created {
  number: number;
  url: string;
}

/** The dialog of "Create pull request..." (#996): its fields and what is running. */
export interface PullDialog {
  base: string;
  title: string;
  body: string;
  draft: boolean;
  /** An agent drafts title and body. */
  drafting: boolean;
  /** `gh pr create` runs. */
  creating: boolean;
  /** The error of the last draft or create; the fields stay as they are. */
  error: string | null;
}

/**
 * Creating a pull request for the branch of the directory of a Changes tab: the state that decides
 * whether the button shows (branch pushed, no open pull request), the dialog and its two backend
 * calls. Provided per Changes tab. Never pushes; a failure keeps every field.
 */
@Injectable()
export class CreatePullRequest {
  private readonly bridge = inject(TauriBridge);
  private readonly changes = inject(Changes);
  private readonly tabs = inject(EditorTabs);
  private readonly ui = inject(UiState);
  private readonly pulls = inject(GithubPulls);

  readonly state = signal<PullState | null>(null);
  readonly dialog = signal<PullDialog | null>(null);
  private run = 0;
  private generation = 0;
  private currentDraft: string | null = null;

  /** The branch is pushed, has no open pull request and is not the base itself. */
  readonly canCreate = computed(() => {
    const s = this.state();
    return !!s && !!s.branch && !!s.base && s.pushed && !s.existing && s.branch !== s.base;
  });

  /** Reads the state again; a failure (no `gh`, offline) hides the button. */
  async load(): Promise<void> {
    const dir = this.changes.dir();
    if (!dir) return;
    const run = ++this.run;
    try {
      const state = await this.bridge.invoke<PullState>("changes_pr_state", { dir });
      if (run === this.run) this.state.set(state);
    } catch {
      if (run === this.run) this.state.set(null);
    }
  }

  open(): void {
    const s = this.state();
    if (!s || !this.canCreate()) return;
    this.dialog.set({
      base: s.base ?? "",
      title: "",
      body: "",
      draft: false,
      drafting: false,
      creating: false,
      error: null,
    });
  }

  patch(change: Partial<PullDialog>): void {
    this.dialog.update((d) => (d ? { ...d, ...change } : d));
  }

  /** Closes the dialog; a running draft is stopped. */
  close(): void {
    void this.cancelDraft();
    this.dialog.set(null);
  }

  /** The harness for "Draft with agent" (the one of the commit message); none hides the button. */
  readonly harness = this.changes.messageHarness;

  /** Fills title and body from the commits and the diff; an error leaves the fields as they were. */
  async draftWithAgent(): Promise<void> {
    const d = this.dialog();
    const harness = this.harness();
    if (!d || !harness || d.drafting || d.creating) return;
    const id = `pull-request-${++this.generation}`;
    this.currentDraft = id;
    this.patch({ drafting: true, error: null });
    try {
      const draft = await this.bridge.invoke<Draft>("changes_draft_pull_request", {
        dir: this.changes.dir(),
        harness,
        base: d.base,
        id,
      });
      if (this.currentDraft !== id) return;
      this.patch({ title: draft.title, body: draft.body });
    } catch (error) {
      if (this.currentDraft === id) this.patch({ error: String(error) });
    } finally {
      if (this.currentDraft === id) {
        this.currentDraft = null;
        this.patch({ drafting: false });
      }
    }
  }

  async cancelDraft(): Promise<void> {
    const id = this.currentDraft;
    if (!id) return;
    this.currentDraft = null;
    this.patch({ drafting: false });
    try {
      await this.bridge.invoke("changes_generate_cancel", { id });
    } catch {
      // The call ends by its own deadline.
    }
  }

  /** Creates the pull request and opens its detail; an error stays in the dialog, nothing is lost. */
  async create(): Promise<void> {
    const d = this.dialog();
    if (!d || d.creating || d.drafting || !d.title.trim() || !d.base.trim()) return;
    this.patch({ creating: true, error: null });
    try {
      const created = await this.bridge.invoke<Created>("changes_create_pull_request", {
        dir: this.changes.dir(),
        base: d.base.trim(),
        title: d.title.trim(),
        body: d.body,
        draft: d.draft,
      });
      this.dialog.set(null);
      void this.load();
      await this.showDetail(created);
    } catch (error) {
      this.patch({ creating: false, error: String(error) });
    }
  }

  private async showDetail(created: Created): Promise<void> {
    this.ui.selectGithubPage("pulls");
    this.tabs.openGithub();
    await this.pulls.load(true);
    const row = this.pulls.rows().find((r) => r.url === created.url);
    if (row) await this.pulls.openDetail(row);
  }
}
