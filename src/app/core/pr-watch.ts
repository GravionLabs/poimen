import { DestroyRef, Injectable, effect, inject, untracked } from "@angular/core";
import { AgentActions, type ActionTarget } from "../github/agent-actions";
import type { Checks } from "../github/pulls";
import { NotificationCenter, type CheckResult } from "./notification-center";
import { SpaceActions } from "./space-actions";
import { Sounds } from "./sounds";
import { SpacesStore } from "./spaces-store";
import { TauriBridge } from "./tauri";
import type { BranchPull, SpaceNode } from "./snapshot";

/** Pending checks are read again this often; the `gh` client caches an answer for 30 s anyway. */
export const WATCH_MS = 30_000;

/** What was seen of one pull request at the last look. */
export interface Seen {
  sha: string;
  pending: boolean;
}

/** An open pull request whose branch is checked out in a space. */
export interface WatchedPull {
  /** Names the pull request (its URL). */
  key: string;
  sha: string;
  checks: Checks;
}

/** The checks are over: nothing pending and an outcome. */
export function isFinal(checks: Checks): boolean {
  return checks.pending === 0 && (checks.state === "passing" || checks.state === "failing");
}

/**
 * The pull requests whose checks finished since `previous`: they were pending on the same head
 * commit then, are final now, and `reported` (`<key>@<sha>`) does not name them yet. A result seen
 * for the first time (at start, a new commit) is not news. `next` is what to remember.
 */
export function finishedChecks(
  previous: ReadonlyMap<string, Seen>,
  reported: ReadonlySet<string>,
  pulls: readonly WatchedPull[],
): { next: Map<string, Seen>; finished: WatchedPull[] } {
  // A failed or partial read leaves a pull request out; what was seen of it stays.
  const next = new Map<string, Seen>(previous);
  const finished: WatchedPull[] = [];
  for (const pull of pulls) {
    if (!pull.sha) continue;
    const before = previous.get(pull.key);
    if (
      before?.pending &&
      before.sha === pull.sha &&
      isFinal(pull.checks) &&
      !reported.has(`${pull.key}@${pull.sha}`)
    ) {
      finished.push(pull);
    }
    next.set(pull.key, { sha: pull.sha, pending: pull.checks.pending > 0 });
  }
  return { next, finished };
}

/** The open pull requests of the local spaces: one per pull request, the first space wins. */
export function watchedPulls(
  spaces: readonly SpaceNode[],
  pulls: ReadonlyMap<string, BranchPull>,
): { space: SpaceNode; dir: string; pull: BranchPull }[] {
  const seen = new Set<string>();
  const out: { space: SpaceNode; dir: string; pull: BranchPull }[] = [];
  for (const space of spaces) {
    const dir = space.workspace.cwd;
    const pull = dir ? pulls.get(dir) : undefined;
    if (!dir || !pull || pull.state !== "open" || seen.has(pull.url)) continue;
    seen.add(pull.url);
    out.push({ space, dir, pull });
  }
  return out;
}

/** Whether a read is worth it: some watched pull request still has pending checks. */
export function anyPending(pulls: readonly { pull: BranchPull }[]): boolean {
  return pulls.some((p) => p.pull.checks.pending > 0);
}

/**
 * Tells the user when the checks of the pull request of a space finish (#995): one notification
 * center entry and the sound per result and head commit. It reads nothing itself except the
 * pull requests the store already holds, and asks the store for them again every 30 s only while
 * a check is pending (so never without `gh`: no pull request, nothing pending).
 */
@Injectable({ providedIn: "root" })
export class PrWatch {
  private readonly store = inject(SpacesStore);
  private readonly center = inject(NotificationCenter);
  private readonly sounds = inject(Sounds);
  private readonly actions = inject(AgentActions);
  private readonly spaces = inject(SpaceActions);
  private readonly bridge = inject(TauriBridge);
  private seen: ReadonlyMap<string, Seen> = new Map();
  private readonly reported = new Set<string>();

  constructor() {
    effect(() => {
      const watched = watchedPulls(this.store.spaces(), this.store.pulls());
      untracked(() => this.observe(watched));
    });
    const timer = setInterval(() => {
      if (anyPending(watchedPulls(this.store.spaces(), this.store.pulls()))) {
        void this.store.refreshPulls();
      }
    }, WATCH_MS);
    inject(DestroyRef).onDestroy(() => clearInterval(timer));
  }

  /** Compares the pull requests with the last look and records what finished. */
  observe(
    watched: readonly { space: SpaceNode; dir: string; pull: BranchPull }[],
    now: number = Date.now(),
  ): void {
    const { next, finished } = finishedChecks(
      this.seen,
      this.reported,
      watched.map((w) => ({ key: w.pull.url, sha: w.pull.headSha ?? "", checks: w.pull.checks })),
    );
    this.seen = next;
    const results: CheckResult[] = [];
    for (const done of finished) {
      const w = watched.find((x) => x.pull.url === done.key);
      if (!w) continue;
      this.reported.add(`${done.key}@${done.sha}`);
      results.push({
        passed: done.checks.state === "passing",
        number: w.pull.number,
        title: w.pull.title,
        url: w.pull.url,
        sha: done.sha,
        label: w.space.workspace.label,
        paneId: w.space.rows[0]?.pane.terminal_id ?? "",
        dir: w.dir,
        branch: w.space.repo?.branch ?? "",
        base: w.pull.baseRef ?? "",
        crossRepo: w.pull.isCrossRepository === true,
      });
    }
    if (results.length === 0) return;
    this.center.recordChecks(results, now);
    // A failure asks for the user like a block: it plays the blocked sound.
    this.sounds.notifyResult(results.every((r) => r.passed) ? "done" : "blocked", now);
  }

  /** Starts the `fix-checks` action on the pull request of a failed entry. */
  async fixChecks(result: CheckResult): Promise<void> {
    const repo = await this.bridge
      .invoke<{ owner: string; name: string } | null>("github_repo", { dir: result.dir })
      .catch(() => null);
    if (!repo) {
      this.spaces.notify("Fix checks: the repository has no GitHub origin", "error");
      return;
    }
    const target: ActionTarget = {
      kind: "pr",
      repo: `${repo.owner}/${repo.name}`,
      root: result.dir,
      number: result.number,
      title: result.title,
      url: result.url,
      branch: result.branch,
      base: result.base,
      level: null,
      labels: [],
      hasChildren: false,
      checks: "failed",
      crossRepo: result.crossRepo,
      state: "open",
      review: null,
    };
    await this.actions.load(result.dir);
    const offered = this.actions.offered([target]).find((o) => o.action.id === "fix-checks");
    if (!offered) {
      this.spaces.notify("Fix checks: the action is not available here", "error");
    } else if (!offered.enabled) {
      this.spaces.notify(`Fix checks: ${offered.reason}`, "error");
    } else {
      await this.actions.run(offered.action, [target]);
    }
  }
}
