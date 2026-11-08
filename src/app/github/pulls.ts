import { Injectable, computed, inject, signal } from "@angular/core";
import { HerdrStore } from "../core/herdr-store";
import { paneDir } from "../core/herdr";
import { SettingsStore } from "../core/settings";
import { SpaceActions } from "../core/space-actions";
import { UiState } from "../core/ui-state";
import { TauriBridge } from "../core/tauri";
import { GithubFilter } from "./github-filter";

/** What a failed `gh` call came to (`GhError` of `github.rs`). */
export type GhError =
  | { kind: "gh-missing" | "not-logged-in" | "offline" | "not-found" }
  | { kind: "rate-limited"; reset: number | null }
  | { kind: "other"; message: string };

export interface Checks {
  state: "none" | "passing" | "failing" | "pending";
  passed: number;
  failed: number;
  pending: number;
}

/** A pull request as `github_pulls` returns it (`Pull` of `github_pulls.rs`). */
export interface Pull {
  number: number;
  title: string;
  url: string;
  author: string | null;
  isDraft: boolean;
  additions: number;
  deletions: number;
  createdAt: string;
  headRef: string;
  isCrossRepository: boolean;
  baseRef: string;
  reviewDecision: "APPROVED" | "CHANGES_REQUESTED" | "REVIEW_REQUIRED" | null;
  reviewRequests: string[];
  checks: Checks;
}

/** One repository: its pull requests, or why they could not be read. */
export type RepoPulls = { root: string; repo: string } & (
  { status: "ok"; pulls: Pull[] } | { status: "error"; error: GhError }
);

export interface PullsResult {
  viewer: string | null;
  repos: RepoPulls[];
}

/** A pull request in the table, with the repository it belongs to. */
export interface PullRow extends Pull {
  repo: string;
  root: string;
  /** Unique over all repositories: `owner/name#number`. */
  key: string;
}

/** A worktree and its branch (`Checkout` of `worktree.rs`). */
export interface WorktreeInfo {
  path: string;
  branch: string | null;
}

/** The worktree whose branch is the head of `pull`: `pr/<number>`, or the head branch unless the head is in a fork. */
export function worktreeOf(pull: Pull, worktrees: readonly WorktreeInfo[]): WorktreeInfo | null {
  return (
    worktrees.find((w) => w.branch === `pr/${pull.number}`) ??
    worktrees.find(
      (w) => !pull.isCrossRepository && w.branch !== null && w.branch === pull.headRef,
    ) ??
    null
  );
}

export type Scope = "review" | "mine" | "all";

export const SCOPES: readonly { id: Scope; label: string }[] = [
  { id: "review", label: "Review requested" },
  { id: "mine", label: "Mine" },
  { id: "all", label: "All" },
];

/** Never ask GitHub more often than this; the backend caches its answers for as long. */
export const REFRESH_MIN_MS = 30_000;

/** Only GitHub web links are opened in the browser. */
export function isGithubUrl(url: string): boolean {
  return /^https:\/\/github\.com\//.test(url);
}

/** Text for an error of one repository. */
export function errorText(error: GhError): string {
  switch (error.kind) {
    case "gh-missing":
      return "The GitHub CLI (gh) is not installed. Install it from https://cli.github.com.";
    case "not-logged-in":
      return "Not signed in to GitHub. Run `gh auth login` in a terminal.";
    case "offline":
      return "GitHub is not reachable. Check the network connection.";
    case "rate-limited":
      return error.reset
        ? `GitHub rate limit reached; resets at ${new Date(error.reset * 1000).toLocaleTimeString()}.`
        : "GitHub rate limit reached. Try again later.";
    case "not-found":
      return "The repository was not found, or the signed-in account cannot see it.";
    case "other":
      return error.message;
  }
}

/** All pull requests of the repositories that could be read, newest repository order kept. */
export function flattenPulls(repos: readonly RepoPulls[]): PullRow[] {
  return repos.flatMap((r) =>
    r.status === "ok"
      ? r.pulls.map((p) => ({ ...p, repo: r.repo, root: r.root, key: `${r.repo}#${p.number}` }))
      : [],
  );
}

export interface PullFilter {
  scope: Scope;
  viewer: string | null;
  /** `owner/name`, or `null` for all repositories. */
  repo: string | null;
  text: string;
}

/**
 * Applies the filters. The text matches the title (case-insensitive) or the number (`12` and `#12`
 * both match #12 and #123).
 */
export function filterPulls(rows: readonly PullRow[], f: PullFilter): PullRow[] {
  const text = f.text.trim().toLowerCase().replace(/^#/, "");
  const me = f.viewer?.toLowerCase();
  return rows.filter((r) => {
    if (f.repo && r.repo !== f.repo) return false;
    if (f.scope === "mine" && (!me || r.author?.toLowerCase() !== me)) return false;
    if (f.scope === "review" && (!me || !r.reviewRequests.some((l) => l.toLowerCase() === me)))
      return false;
    return !text || r.title.toLowerCase().includes(text) || String(r.number).includes(text);
  });
}

/** A small colored label; the tone picks a helix-ui color token in the stylesheet. */
export interface Badge {
  label: string;
  tone: "success" | "danger" | "warning" | "primary" | "neutral";
}

/** Review decision as a badge; none for a draft (it has its own badge) or without a decision. */
export function reviewBadge(pull: Pick<Pull, "isDraft" | "reviewDecision">): Badge | null {
  if (pull.isDraft) return null;
  switch (pull.reviewDecision) {
    case "APPROVED":
      return { label: "Approved", tone: "success" };
    case "CHANGES_REQUESTED":
      return { label: "Changes requested", tone: "danger" };
    case "REVIEW_REQUIRED":
      return { label: "Review required", tone: "warning" };
    default:
      return null;
  }
}

/** One check state with its count, shown as an icon and a number. */
export interface CheckBadge {
  kind: "passed" | "failed" | "pending";
  count: number;
  icon: string;
  label: string;
}

/** The non-empty check counts as icons; nothing without checks. */
export function checkBadges(checks: Checks): CheckBadge[] {
  if (checks.state === "none") return [];
  const all: CheckBadge[] = [
    { kind: "passed", count: checks.passed, icon: "✓", label: "passed" },
    { kind: "failed", count: checks.failed, icon: "✗", label: "failed" },
    { kind: "pending", count: checks.pending, icon: "●", label: "pending" },
  ];
  return all.filter((c) => c.count > 0);
}

/** The pull requests of one repository, in the group view. */
export interface PullGroup {
  repo: string;
  root: string;
  rows: PullRow[];
}

/** Rows grouped by repository in first-seen order. */
export function groupPulls(rows: readonly PullRow[]): PullGroup[] {
  const groups = new Map<string, PullGroup>();
  for (const r of rows) {
    const g = groups.get(r.root) ?? { repo: r.repo, root: r.root, rows: [] };
    g.rows.push(r);
    groups.set(r.root, g);
  }
  return [...groups.values()];
}

/** Short text for the check counts, empty without checks. */
export function checksText(checks: Checks): string {
  if (checks.state === "none") return "";
  const parts = [`${checks.passed} passed`];
  if (checks.failed) parts.push(`${checks.failed} failed`);
  if (checks.pending) parts.push(`${checks.pending} pending`);
  return parts.join(", ");
}

/** Age as `5m`, `3h`, `2d`, `6w`, `4mo` or `2y`; empty for an unreadable date. */
export function ageText(createdAt: string, now: number): string {
  const then = Date.parse(createdAt);
  if (Number.isNaN(then)) return "";
  const minutes = Math.max(0, Math.floor((now - then) / 60_000));
  if (minutes < 60) return `${minutes}m`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}h`;
  const days = Math.floor(hours / 24);
  if (days < 14) return `${days}d`;
  if (days < 60) return `${Math.floor(days / 7)}w`;
  if (days < 730) return `${Math.floor(days / 30)}mo`;
  return `${Math.floor(days / 365)}y`;
}

/** The keys between `from` and `to` (both included) in list order; just `to` without an anchor. */
export function keyRange(keys: readonly string[], from: string | null, to: string): string[] {
  const a = from === null ? -1 : keys.indexOf(from);
  const b = keys.indexOf(to);
  if (b < 0) return [];
  if (a < 0) return [to];
  return keys.slice(Math.min(a, b), Math.max(a, b) + 1);
}

/**
 * The open pull requests of the repositories behind the spaces and the explorer roots. The backend
 * (`github_pulls`) does the reading and caches for 30 s; this holds the last answer, the filters
 * and the selection. The selection is a signal for later actions on several pull requests.
 */
@Injectable({ providedIn: "root" })
export class GithubPulls {
  private readonly bridge = inject(TauriBridge);
  private readonly store = inject(HerdrStore);
  private readonly settings = inject(SettingsStore);
  private readonly spaces = inject(SpaceActions);
  private readonly ui = inject(UiState);

  private readonly _result = signal<PullsResult | null>(null);
  private readonly _loading = signal(false);
  private readonly _error = signal<string | null>(null);
  private readonly _selected = signal<ReadonlySet<string>>(new Set());
  private readonly _cursor = signal<string | null>(null);
  private last = 0;
  private anchor: string | null = null;

  readonly result = this._result.asReadonly();
  readonly loading = this._loading.asReadonly();
  /** Why nothing could be read at all (the command itself failed); per-repository errors are in `result`. */
  readonly error = this._error.asReadonly();

  private readonly _worktrees = signal<ReadonlyMap<string, readonly WorktreeInfo[]>>(new Map());

  readonly scope = signal<Scope>("review");
  private readonly filter = inject(GithubFilter);
  readonly text = signal("");

  /** Keys (`owner/name#number`) of the selected pull requests. */
  readonly selected = this._selected.asReadonly();
  /** The row the keyboard works on. */
  readonly cursor = this._cursor.asReadonly();

  readonly rows = computed(() => flattenPulls(this._result()?.repos ?? []));
  readonly repoNames = computed(() => (this._result()?.repos ?? []).map((r) => r.repo));
  readonly visible = computed(() =>
    filterPulls(this.rows(), {
      scope: this.scope(),
      viewer: this._result()?.viewer ?? null,
      repo: this.filter.repo(),
      text: this.text(),
    }),
  );
  /**
   * Visible rows grouped by repository, in repository order; a readable repository without
   * matches only shows up as an empty group when "Show empty repositories" is on.
   */
  readonly groups = computed<PullGroup[]>(() => {
    const byRoot = new Map(groupPulls(this.visible()).map((g) => [g.root, g]));
    if (!this.ui.githubShowEmpty()) return [...byRoot.values()];
    const wanted = this.filter.repo();
    return (this._result()?.repos ?? []).flatMap((r) =>
      r.status === "ok" && (!wanted || r.repo === wanted)
        ? [byRoot.get(r.root) ?? { repo: r.repo, root: r.root, rows: [] }]
        : [],
    );
  });
  /** The selected pull requests that pass the filters, in table order. */
  readonly selectedPulls = computed(() => {
    const keys = this._selected();
    return this.visible().filter((r) => keys.has(r.key));
  });

  /** Space and explorer directories; the backend resolves them to repositories. */
  private dirs(): string[] {
    const found = new Set<string>();
    for (const pane of this.store.snapshot().terminals) {
      const dir = paneDir(pane);
      if (dir) found.add(dir);
    }
    for (const root of this.settings.settings().explorerRoots) found.add(root);
    return [...found];
  }

  /**
   * Reads the pull requests. A read already running is not repeated; unless `force`, a read within
   * the last 30 s is not repeated either (the backend would only answer from its cache).
   */
  async load(force = true): Promise<void> {
    if (this._loading() || (!force && Date.now() - this.last < REFRESH_MIN_MS)) return;
    this._loading.set(true);
    this.last = Date.now();
    try {
      const result = await this.bridge.invoke<PullsResult>("github_pulls", { dirs: this.dirs() });
      this._result.set(result);
      this._error.set(null);
      await this.loadWorktrees(result);
      const known = new Set(this.rows().map((r) => r.key));
      this._selected.update((s) => new Set([...s].filter((k) => known.has(k))));
    } catch (err) {
      this._error.set(String(err));
    } finally {
      this._loading.set(false);
    }
  }

  /** The worktree that has the head of `row` checked out, if any. */
  worktreeFor(row: PullRow): WorktreeInfo | null {
    return worktreeOf(row, this._worktrees().get(row.root) ?? []);
  }

  /** Reads the worktrees of the readable repositories; a failure only leaves them blank. */
  private async loadWorktrees(result: PullsResult): Promise<void> {
    const found = new Map<string, readonly WorktreeInfo[]>();
    await Promise.all(
      result.repos.map(async (r) => {
        if (r.status !== "ok") return;
        try {
          found.set(
            r.root,
            await this.bridge.invoke<WorktreeInfo[]>("worktree_branches", { dir: r.root }),
          );
        } catch {
          // No worktree marks for this repository.
        }
      }),
    );
    this._worktrees.set(found);
  }

  /** Focuses or creates the space of the worktree that has the head of `row` checked out. */
  async openSpace(row: PullRow): Promise<void> {
    const wt = this.worktreeFor(row);
    if (wt) await this.spaces.create(wt.path, `#${row.number}`);
  }

  /** Creates or reuses the worktree of `row` (also for forks), then focuses or creates its space. */
  async checkout(row: PullRow): Promise<void> {
    try {
      const wt = await this.bridge.invoke<{ path: string }>("pr_worktree", {
        repoDir: row.root,
        nr: row.number,
        headRef: row.headRef,
      });
      await this.spaces.create(wt.path, `#${row.number}`);
    } catch (err) {
      this.spaces.notify(`Pull request #${row.number}: ${String(err)}`);
    }
    const result = this._result();
    if (result) await this.loadWorktrees(result);
  }

  /** Opens a pull request in the browser; other links are ignored. */
  async openUrl(url: string): Promise<void> {
    if (isGithubUrl(url)) await this.bridge.openUrl(url);
  }

  /** Adds or removes one row from the selection; with `range` selects everything since the anchor. */
  select(key: string, mode: "toggle" | "range" = "toggle"): void {
    this._cursor.set(key);
    if (mode === "range") {
      const keys = keyRange(
        this.visible().map((r) => r.key),
        this.anchor,
        key,
      );
      this._selected.update((s) => new Set([...s, ...keys]));
      return;
    }
    this._selected.update((s) => {
      const next = new Set(s);
      if (!next.delete(key)) next.add(key);
      return next;
    });
    this.anchor = key;
  }

  /** Moves the keyboard cursor by `delta` rows within the visible rows (clamped). */
  moveCursor(delta: number): void {
    const keys = this.visible().map((r) => r.key);
    if (!keys.length) return;
    const at = this._cursor() === null ? -1 : keys.indexOf(this._cursor()!);
    const next = Math.min(
      keys.length - 1,
      Math.max(0, at < 0 ? (delta > 0 ? 0 : keys.length - 1) : at + delta),
    );
    this._cursor.set(keys[next]);
  }

  clearSelection(): void {
    this._selected.set(new Set());
    this._cursor.set(null);
    this.anchor = null;
  }
}
