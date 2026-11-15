import { Injectable, computed, inject, signal } from "@angular/core";
import { EditorTabs } from "../core/editor-tabs";
import { paneDir } from "../core/snapshot";
import { SettingsStore } from "../core/settings";
import { SpacesStore } from "../core/spaces-store";
import { TauriBridge } from "../core/tauri";
import { UiState } from "../core/ui-state";
import { GithubIssues, type Issue, type RepoIssues } from "../github/issues";
import type { RepoMilestones } from "../github/milestones";
import { ghErrorOf, type GhError } from "../github/pulls";
import type { OpenTask, RunGroup } from "./deliver-status";
import { Deliver } from "./deliver";
import {
  buildOverview,
  githubProgress,
  groupsOf,
  taskTitle,
  type RepoOverview,
} from "./milestone-overview";

/** The overview is read again at least this often while the Runs view is open. */
export const OVERVIEW_REFRESH_MS = 5 * 60_000;

/**
 * The milestone overview at the top of the Runs view (#585): the open milestones of the
 * repositories of the open spaces with the state of their issues (`github_milestones`,
 * `github_issues`, both through the 30 s cache of the shared GitHub client) and the runs of
 * `Deliver` attached to them. Failures keep the last state and show a hint.
 */
@Injectable({ providedIn: "root" })
export class Overview {
  private readonly bridge = inject(TauriBridge);
  private readonly store = inject(SpacesStore);
  private readonly settings = inject(SettingsStore);
  private readonly deliver = inject(Deliver);
  private readonly ui = inject(UiState);
  private readonly editor = inject(EditorTabs);
  private readonly issues = inject(GithubIssues);

  private readonly milestones = signal<RepoMilestones[]>([]);
  private readonly open = signal<RepoIssues[]>([]);
  private readonly closed = signal<RepoIssues[]>([]);
  private readonly _loading = signal(false);
  private readonly _failure = signal<GhError | string | null>(null);
  private inFlight = false;
  private again = false;

  readonly loading = this._loading.asReadonly();
  /** Why the last read failed (the previous state stays); `null` when it worked. */
  readonly failure = this._failure.asReadonly();

  /** One block per repository: GitHub's milestones with the runs, else the runs alone. */
  readonly repos = computed<RepoOverview[]>(() =>
    buildOverview({
      milestones: this.milestones(),
      issues: this.open(),
      closed: this.closed(),
      groups: this.deliver.groups(),
      now: this.deliver.now(),
    }),
  );

  /** Every issue read, for titles of bare `#<nr>` task subjects. */
  private readonly known = computed(() => {
    const out = new Map<string, Issue[]>();
    for (const r of [...this.open(), ...this.closed()])
      if (r.status === "ok") out.set(r.root, [...(out.get(r.root) ?? []), ...r.issues]);
    return out;
  });

  /** GitHub's `closed/total` of the milestone a group works on, else `null` (no GitHub). */
  progressOf(group: RunGroup): { done: number; total: number; label: string } | null {
    if (group.machine !== undefined || group.subject?.kind !== "github.milestone") return null;
    for (const r of this.milestones()) {
      if (r.status !== "ok" || r.root !== group.root) continue;
      const m = r.milestones.find((x) => groupsOf([group], group.root, x).length > 0);
      if (m) return githubProgress(m);
    }
    return null;
  }

  /** The title of a task: the issue's own for a bare `#<nr>`, else what the run said. */
  titleOf(task: Pick<OpenTask, "subject">, root: string, machine?: string): string {
    return machine === undefined
      ? taskTitle(task, this.known().get(root) ?? [])
      : task.subject.title;
  }

  /** Reads milestones and issues of the open repositories; a failure keeps the last state. */
  async load(): Promise<void> {
    if (this.inFlight) {
      this.again = true;
      return;
    }
    this.inFlight = true;
    this._loading.set(true);
    try {
      do {
        this.again = false;
        const dirs = this.dirs();
        if (dirs.length === 0) {
          this.milestones.set([]);
          this.open.set([]);
          this.closed.set([]);
          this._failure.set(null);
          continue;
        }
        try {
          const [m, o, c] = await Promise.all([
            this.bridge.invoke<RepoMilestones[]>("github_milestones", { dirs }),
            this.bridge.invoke<RepoIssues[]>("github_issues", { dirs, closed: false }),
            this.bridge.invoke<RepoIssues[]>("github_issues", { dirs, closed: true }),
          ]);
          this.milestones.set(m ?? []);
          this.open.set(o ?? []);
          this.closed.set(c ?? []);
          this._failure.set(null);
        } catch (err) {
          this._failure.set(ghErrorOf(err));
        }
      } while (this.again);
    } finally {
      this.inFlight = false;
      this._loading.set(false);
    }
  }

  /** Opens the issue in the GitHub tab. */
  openIssue(repo: string, number: number): void {
    this.issues.select(repo, number);
    this.ui.selectGithubPage("issues");
    this.editor.openGithub();
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
