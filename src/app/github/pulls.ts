import { Injectable, computed, inject, signal } from "@angular/core";
import { HerdrStore } from "../core/herdr-store";
import { paneDir } from "../core/herdr";
import { SettingsStore } from "../core/settings";
import { TauriBridge } from "../core/tauri";

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

/** Review state for the table; a draft has none yet. */
export function reviewText(pull: Pull): string {
  if (pull.isDraft) return "Draft";
  switch (pull.reviewDecision) {
    case "APPROVED":
      return "Approved";
    case "CHANGES_REQUESTED":
      return "Changes requested";
    case "REVIEW_REQUIRED":
      return "Review required";
    default:
      return pull.reviewRequests.length ? "Requested" : "None";
  }
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

  readonly scope = signal<Scope>("review");
  readonly repoFilter = signal<string | null>(null);
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
      repo: this.repoFilter(),
      text: this.text(),
    }),
  );
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
      const known = new Set(this.rows().map((r) => r.key));
      this._selected.update((s) => new Set([...s].filter((k) => known.has(k))));
    } catch (err) {
      this._error.set(String(err));
    } finally {
      this._loading.set(false);
    }
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
