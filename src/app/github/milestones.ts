import { Injectable, computed, inject, signal } from "@angular/core";
import { paneDir } from "../core/snapshot";
import { SpacesStore } from "../core/spaces-store";
import { SettingsStore } from "../core/settings";
import { TauriBridge } from "../core/tauri";
import { GithubFilter } from "./github-filter";
import { UiState } from "../core/ui-state";
import { SpaceActions } from "../core/space-actions";
import { REFRESH_MIN_MS, errorText, ghErrorOf, isGithubUrl, type GhError } from "./pulls";

/** A milestone as `github_milestones` returns it (`Milestone` of `github_milestones.rs`). */
export interface Milestone {
  number: number;
  title: string;
  url: string;
  dueOn: string | null;
  description: string;
  open: number;
  closed: number;
  /** The lowest versioned milestone (`x.y.z`, `M<n>` or `M<n> (x.y.z)`) with open issues. */
  current: boolean;
}

export type RepoMilestones = { root: string; repo: string } & (
  | { status: "ok"; milestones: Milestone[] }
  | { status: "error"; error: GhError }
);

/** Share of closed issues in percent, 0 for a milestone without issues. */
export function progressPercent(m: Pick<Milestone, "open" | "closed">): number {
  const total = m.open + m.closed;
  return total === 0 ? 0 : Math.round((m.closed / total) * 100);
}

/** A milestone with closed issues and no open ones can be closed. */
export function readyToClose(m: Pick<Milestone, "open" | "closed">): boolean {
  return m.open === 0 && m.closed > 0;
}

/** "Due 2026-11-20", or "No due date". */
export function dueText(dueOn: string | null): string {
  return dueOn ? `Due ${dueOn.slice(0, 10)}` : "No due date";
}

/** The open step of a milestone write: a form (create, edit) or a confirmation (close). */
export interface MilestoneCompose {
  kind: "create" | "edit" | "close";
  root: string;
  repo: string;
  /** Milestone number, for edit and close. */
  number: number | null;
  title: string;
  description: string;
  /** `YYYY-MM-DD` or empty. */
  dueOn: string;
  /** Open issues of the milestone to close. */
  open: number;
}

/** The selected milestone of one repository; it filters that repository's issue tree. */
export interface MilestoneSelection {
  repo: string;
  title: string;
}

/** The open milestones of the repositories behind the spaces and the explorer roots. */
@Injectable({ providedIn: "root" })
export class GithubMilestones {
  private readonly bridge = inject(TauriBridge);
  private readonly store = inject(SpacesStore);
  private readonly settings = inject(SettingsStore);
  private readonly ui = inject(UiState);
  private readonly filter = inject(GithubFilter);
  private readonly spaces = inject(SpaceActions);

  private readonly _result = signal<RepoMilestones[] | null>(null);
  private readonly _loading = signal(false);
  private readonly _error = signal<string | null>(null);
  private last = 0;

  readonly result = this._result.asReadonly();
  readonly loading = this._loading.asReadonly();
  readonly error = this._error.asReadonly();
  readonly selected = signal<MilestoneSelection | null>(null);

  readonly repos = computed(() =>
    (this._result() ?? []).flatMap((r) =>
      r.status === "ok" &&
      this.filter.allows(r.repo) &&
      (r.milestones.length > 0 || this.ui.githubShowEmpty())
        ? [r]
        : [],
    ),
  );
  /** Repositories that were read, for the shared repository filter. */
  readonly repoNames = computed(() => (this._result() ?? []).map((r) => r.repo));
  /** Open milestones of the repositories that pass the repository filter. */
  readonly count = computed(() => this.repos().reduce((n, r) => n + r.milestones.length, 0));
  readonly errors = computed(() =>
    (this._result() ?? []).flatMap((r) =>
      r.status === "error" ? [{ repo: r.repo, error: r.error }] : [],
    ),
  );

  /** Selecting the selected milestone again clears the filter; a new selection shows the issues. */
  toggle(repo: string, title: string): void {
    const clear = this.isSelected(repo, title);
    this.selected.set(clear ? null : { repo, title });
    if (!clear) this.ui.selectGithubPage("issues");
  }

  isSelected(repo: string, title: string): boolean {
    const s = this.selected();
    return !!s && s.repo === repo && s.title === title;
  }

  /** The milestone title that filters `repo`'s issues, if any. */
  forRepo(repo: string): string | null {
    const s = this.selected();
    return s && s.repo === repo ? s.title : null;
  }

  async load(force = true): Promise<void> {
    if (this._loading() || (!force && Date.now() - this.last < REFRESH_MIN_MS)) return;
    this._loading.set(true);
    this.last = Date.now();
    try {
      const result =
        (await this.bridge.invoke<RepoMilestones[]>("github_milestones", {
          dirs: this.dirs(),
        })) ?? [];
      this._result.set(result);
      // A selection whose milestone is gone (closed, renamed) no longer filters.
      const s = this.selected();
      const still = (r: RepoMilestones) =>
        r.status === "ok" && r.repo === s?.repo && r.milestones.some((m) => m.title === s.title);
      if (s && !result.some(still)) this.selected.set(null);
      this._error.set(null);
    } catch (err) {
      this._error.set(String(err));
    } finally {
      this._loading.set(false);
    }
  }

  readonly compose = signal<MilestoneCompose | null>(null);
  private readonly _busy = signal(false);
  readonly busy = this._busy.asReadonly();

  /** Opens the create form with `M<n+1>` suggested from the repository's milestones. */
  async openCreate(root: string, repo: string): Promise<void> {
    let title = "";
    try {
      title = (await this.bridge.invoke<string>("github_milestone_title", { root })) ?? "";
    } catch {
      // No suggestion; the title stays empty.
    }
    this.compose.set({
      kind: "create",
      root,
      repo,
      number: null,
      title,
      description: "",
      dueOn: "",
      open: 0,
    });
  }

  openEdit(root: string, repo: string, m: Milestone): void {
    this.compose.set({
      kind: "edit",
      root,
      repo,
      number: m.number,
      title: m.title,
      description: m.description,
      dueOn: m.dueOn ? m.dueOn.slice(0, 10) : "",
      open: m.open,
    });
  }

  /** Close asks first; the dialog names the open issues, if any. */
  openClose(root: string, repo: string, m: Milestone): void {
    this.compose.set({
      kind: "close",
      root,
      repo,
      number: m.number,
      title: m.title,
      description: "",
      dueOn: "",
      open: m.open,
    });
  }

  cancel(): void {
    this.compose.set(null);
  }

  /** Sends the open write; on success the milestones reload, on failure a notification names it. */
  async submit(): Promise<boolean> {
    const c = this.compose();
    if (!c || this._busy()) return false;
    const number = c.number ?? 0;
    const write =
      c.kind === "close"
        ? { kind: "close", number }
        : {
            kind: c.kind === "create" ? "create" : "update",
            ...(c.kind === "edit" ? { number } : {}),
            title: c.title,
            description: c.description,
            dueOn: c.dueOn.trim() || null,
          };
    this._busy.set(true);
    try {
      await this.bridge.invoke("github_milestone_write", { root: c.root, write });
    } catch (err) {
      const e = ghErrorOf(err);
      const verb = c.kind === "create" ? "Create" : c.kind === "edit" ? "Edit" : "Close";
      this.spaces.notify(
        `${verb} milestone ${c.title}: ${typeof e === "string" ? e : errorText(e)}`,
        "error",
      );
      return false;
    } finally {
      this._busy.set(false);
    }
    this.compose.set(null);
    await this.load(true);
    return true;
  }

  async openUrl(url: string): Promise<void> {
    if (isGithubUrl(url)) await this.bridge.openUrl(url);
  }

  private dirs(): string[] {
    const found = new Set<string>();
    for (const pane of this.store.snapshot().terminals) {
      const dir = paneDir(pane);
      if (dir) found.add(dir);
    }
    for (const root of this.settings.settings().explorerRoots) found.add(root);
    return [...found];
  }
}
