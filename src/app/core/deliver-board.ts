import { Injectable, inject, signal } from "@angular/core";
import { TauriBridge } from "./tauri";

/** The plugin and pane entrypoint the native board replaces (ADR 0004). */
export const BOARD_PLUGIN = "deliver-board";
export const BOARD_ENTRYPOINT = "board";

export interface BoardChecks {
  state: string;
  passed: number;
  failed: number;
  pending: number;
  skipped: number;
}

export interface BoardPr {
  number: number;
  url: string;
  state: string;
  checks: BoardChecks;
}

export interface BoardRow {
  number: number;
  title: string;
  url: string;
  assignee: string | null;
  labels: string[];
  branch: string | null;
  pr: BoardPr | null;
}

/** What reading one repository came to (`BoardOutcome` of `plugin_view.rs`). */
export type BoardOutcome =
  | { status: "ok"; generated: string | null; repo: string | null; rows: BoardRow[] }
  | { status: "plugin_error"; code: string; message: string }
  | { status: "outdated"; version: number }
  | { status: "failed"; message: string };

export interface RepoBoard {
  root: string;
  name: string;
  outcome: BoardOutcome;
}

/** The `deliver_board` answer: no native view for the installed plugin, or one board per repo. */
export type BoardView =
  { status: "unsupported"; reason: string } | { status: "repos"; repos: RepoBoard[] };

/** Only GitHub web links are opened in the browser. */
export function isGithubUrl(url: string): boolean {
  return /^https:\/\/github\.com\//.test(url);
}

/** Short text for a PR's check counts, empty without checks. */
export function checksText(checks: BoardChecks): string {
  if (checks.state === "none") return "";
  const parts = [`${checks.passed} passed`];
  if (checks.failed) parts.push(`${checks.failed} failed`);
  if (checks.pending) parts.push(`${checks.pending} pending`);
  return parts.join(", ");
}

/**
 * The deliver board data. The plugin owns the logic (`data` action, run by the backend); this only
 * holds the last payload for display. Refresh is on demand, on focus and every {@link REFRESH_MS}.
 */
@Injectable({ providedIn: "root" })
export class DeliverBoard {
  private readonly bridge = inject(TauriBridge);
  private readonly _view = signal<BoardView | null>(null);
  private readonly _loading = signal(false);
  private readonly _error = signal<string | null>(null);
  private readonly _updated = signal<number | null>(null);
  private last = 0;

  readonly view = this._view.asReadonly();
  readonly loading = this._loading.asReadonly();
  /** Why the board could not be read at all (herdr not reachable); per-repo errors are in `view`. */
  readonly error = this._error.asReadonly();
  /** Epoch ms of the last successful read. */
  readonly updated = this._updated.asReadonly();

  /** Reads the board; a read already running is not repeated. `force` skips the 30 s throttle. */
  async load(force = true): Promise<void> {
    if (this._loading() || (!force && Date.now() - this.last < REFRESH_MIN_MS)) return;
    this._loading.set(true);
    this.last = Date.now();
    try {
      this._view.set(await this.bridge.invoke<BoardView>("deliver_board"));
      this._error.set(null);
      this._updated.set(Date.now());
    } catch (err) {
      this._error.set(String(err));
    } finally {
      this._loading.set(false);
    }
  }

  /** Opens a GitHub issue or PR in the browser; other links are ignored. */
  async openUrl(url: string): Promise<void> {
    if (isGithubUrl(url)) await this.bridge.openUrl(url);
  }

  /**
   * Starts `/deliver` for the PBI through the deliver-starter `start-link` action with the issue URL
   * as the clicked link and the repository as the working directory. Resolves to an error text.
   */
  async startDeliver(root: string, row: BoardRow): Promise<string | null> {
    if (!isGithubUrl(row.url)) return `Start deliver #${row.number}: not a GitHub issue URL`;
    try {
      await this.bridge.invoke("plugin_invoke", {
        pluginId: "deliver-starter",
        actionId: "start-link",
        context: { focused_pane_cwd: root, clicked_url: row.url },
      });
      return null;
    } catch (err) {
      return `Start deliver #${row.number}: ${String(err)}`;
    }
  }
}

/** Periodic refresh of an open board; the plugin caches `gh` for 30 s, so never faster. */
export const REFRESH_MS = 60_000;
const REFRESH_MIN_MS = 30_000;
