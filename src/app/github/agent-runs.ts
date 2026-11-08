import { Injectable, Injector, computed, inject, signal } from "@angular/core";
import { EditorTabs } from "../core/editor-tabs";
import type { AgentStatus } from "../core/herdr";
import { GithubIssues } from "./issues";
import { GithubPulls } from "./pulls";

/** An agent action started for issues or pull requests of one repository, in one pane. */
export interface AgentRun {
  paneId: string;
  /** `owner/name`. */
  repo: string;
  numbers: number[];
  actionId: string;
  /** Label of the action, shown in the badge. */
  label: string;
  /** The pane was in a snapshot at least once (before that a missing pane is only not yet listed). */
  seen: boolean;
  state: "starting" | "running" | "waiting";
}

/** What a row or detail pane shows for an issue or pull request. */
export interface RunBadge {
  paneId: string;
  state: "running" | "waiting";
  text: string;
}

/**
 * Folds the pane statuses into the runs: `working` is running, `blocked` waiting (the badge shows
 * both), `done` or a pane that is gone ends the run. Pure; `finished` are the ended runs.
 */
export function advance(
  runs: readonly AgentRun[],
  statuses: ReadonlyMap<string, AgentStatus>,
): { runs: AgentRun[]; finished: AgentRun[] } {
  const kept: AgentRun[] = [];
  const finished: AgentRun[] = [];
  for (const run of runs) {
    const status = statuses.get(run.paneId);
    if (status === undefined) {
      // A pane not listed yet is waited for; one that was listed and vanished is closed.
      if (run.seen) finished.push(run);
      else kept.push(run);
    } else if (status === "done") {
      finished.push(run);
    } else {
      const state = status === "working" ? "running" : status === "blocked" ? "waiting" : run.state;
      kept.push({ ...run, seen: true, state });
    }
  }
  return { runs: kept, finished };
}

/**
 * The agent runs of the GitHub tab: `paneId` to repository, numbers and action, recorded when an
 * action starts an agent. In memory only (it survives the GitHub tab, not an app restart). A badge
 * never outlives its pane; when a run ends, the pull requests and issues are read again once.
 */
@Injectable({ providedIn: "root" })
export class AgentRuns {
  private readonly injector = inject(Injector);
  private readonly pulls = inject(GithubPulls);
  private readonly issues = inject(GithubIssues);
  private readonly _runs = signal<readonly AgentRun[]>([]);

  readonly runs = this._runs.asReadonly();
  private readonly badges = computed(() => {
    const map = new Map<string, RunBadge>();
    for (const run of this._runs()) {
      if (run.state === "starting") continue;
      for (const nr of run.numbers) {
        map.set(`${run.repo}#${nr}`, {
          paneId: run.paneId,
          state: run.state,
          text: `${run.label} ${run.state}`,
        });
      }
    }
    return map;
  });

  /** Records the pane of a started agent. */
  add(run: Pick<AgentRun, "paneId" | "repo" | "numbers" | "actionId" | "label">): void {
    this._runs.update((all) => [
      ...all.filter((r) => r.paneId !== run.paneId),
      { ...run, seen: false, state: "starting" },
    ]);
  }

  /** The badge of an issue or pull request, `null` when no agent works on it. */
  badge(repo: string, number: number): RunBadge | null {
    return this.badges().get(`${repo}#${number}`) ?? null;
  }

  /** Folds a snapshot of pane statuses in (the app feeds it with the `Attention` stream); ended runs reload the lists, once per call. */
  observe(statuses: ReadonlyMap<string, AgentStatus>): void {
    const { runs, finished } = advance(this._runs(), statuses);
    if (finished.length === 0 && runs.every((r, i) => r === this._runs()[i])) return;
    this._runs.set(runs);
    if (finished.length > 0) {
      void this.pulls.load();
      void this.issues.load();
    }
  }

  /** A click on a badge: show the pane of the agent. */
  focus(paneId: string): void {
    // Looked up on use: the tab model is not needed to follow the runs.
    this.injector.get(EditorTabs).open(paneId);
  }
}
