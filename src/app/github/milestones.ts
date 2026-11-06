import { Injectable, computed, inject, signal } from "@angular/core";
import { paneDir } from "../core/herdr";
import { HerdrStore } from "../core/herdr-store";
import { SettingsStore } from "../core/settings";
import { TauriBridge } from "../core/tauri";
import { REFRESH_MIN_MS, isGithubUrl, type GhError } from "./pulls";

/** A milestone as `github_milestones` returns it (`Milestone` of `github_milestones.rs`). */
export interface Milestone {
  number: number;
  title: string;
  url: string;
  dueOn: string | null;
  open: number;
  closed: number;
  /** The lowest versioned milestone (`x.y.z`, `M<n>` or `M<n> (x.y.z)`) with open issues. */
  current: boolean;
}

export type RepoMilestones = { root: string; repo: string } & (
  { status: "ok"; milestones: Milestone[] } | { status: "error"; error: GhError }
);

/** Share of closed issues in percent, 0 for a milestone without issues. */
export function progressPercent(m: Pick<Milestone, "open" | "closed">): number {
  const total = m.open + m.closed;
  return total === 0 ? 0 : Math.round((m.closed / total) * 100);
}

/** "Due 2026-11-20", or "No due date". */
export function dueText(dueOn: string | null): string {
  return dueOn ? `Due ${dueOn.slice(0, 10)}` : "No due date";
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
  private readonly store = inject(HerdrStore);
  private readonly settings = inject(SettingsStore);

  private readonly _result = signal<RepoMilestones[] | null>(null);
  private readonly _loading = signal(false);
  private readonly _error = signal<string | null>(null);
  private last = 0;

  readonly result = this._result.asReadonly();
  readonly loading = this._loading.asReadonly();
  readonly error = this._error.asReadonly();
  readonly selected = signal<MilestoneSelection | null>(null);

  readonly repos = computed(() =>
    (this._result() ?? []).flatMap((r) => (r.status === "ok" ? [r] : [])),
  );
  readonly errors = computed(() =>
    (this._result() ?? []).flatMap((r) =>
      r.status === "error" ? [{ repo: r.repo, error: r.error }] : [],
    ),
  );

  /** Selecting the selected milestone again clears the filter. */
  toggle(repo: string, title: string): void {
    this.selected.set(this.isSelected(repo, title) ? null : { repo, title });
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
