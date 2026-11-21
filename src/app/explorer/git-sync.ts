import { Injectable, inject } from "@angular/core";
import { SpacesStore } from "../core/spaces-store";
import { SpaceActions } from "../core/space-actions";
import { TauriBridge } from "../core/tauri";

/** Result of one repository (`git_sync.rs`). */
export type SyncOutcome =
  | { state: "updated" }
  | { state: "current" }
  | { state: "failed"; reason: string };

export type RepoResult = SyncOutcome & { path: string };

export interface SyncSummary {
  results: RepoResult[];
  updated: number;
  current: number;
  failed: number;
}

const name = (path: string): string => path.split(/[\\/]/).filter(Boolean).pop() ?? path;

/** One status line for a fetch or pull of a single repository. */
export function outcomeText(verb: "Fetch" | "Pull", dir: string, outcome: SyncOutcome): string {
  switch (outcome.state) {
    case "updated":
      return `${verb} ${name(dir)}: updated.`;
    case "current":
      return `${verb} ${name(dir)}: already up to date.`;
    case "failed":
      return `${verb} ${name(dir)} failed: ${outcome.reason}`;
  }
}

/** The summary of "Fetch all": counts, then each failed repository with its reason. */
export function summaryText(summary: SyncSummary): string {
  if (summary.results.length === 0) return "Fetch all: no repositories found.";
  const head = `Fetch all: ${summary.updated} updated, ${summary.current} up to date, ${summary.failed} failed.`;
  const failed = summary.results
    .filter((r): r is RepoResult & { state: "failed"; reason: string } => r.state === "failed")
    .map((r) => `${name(r.path)}: ${r.reason.split("\n")[0]}`);
  return failed.length > 0 ? `${head} ${failed.join("; ")}` : head;
}

/** Fetch and pull from the explorer; results are toasts, then ahead/behind refreshes. */
@Injectable({ providedIn: "root" })
export class GitSync {
  private readonly bridge = inject(TauriBridge);
  private readonly actions = inject(SpaceActions);
  private readonly store = inject(SpacesStore);

  async fetch(dir: string): Promise<void> {
    await this.single("Fetch", "git_fetch", dir);
  }

  async pull(dir: string): Promise<void> {
    await this.single("Pull", "git_pull", dir);
  }

  async fetchAll(dir: string): Promise<void> {
    try {
      const summary = await this.bridge.invoke<SyncSummary>("git_fetch_all", { dir });
      this.actions.notify(summaryText(summary), summary.failed > 0 ? "error" : "success");
    } catch (err) {
      this.actions.notify(`Fetch all failed: ${String(err)}`, "error");
    } finally {
      this.store.refreshRepos();
    }
  }

  private async single(verb: "Fetch" | "Pull", command: string, dir: string): Promise<void> {
    try {
      const outcome = await this.bridge.invoke<SyncOutcome>(command, { dir });
      this.actions.notify(
        outcomeText(verb, dir, outcome),
        outcome.state === "failed" ? "error" : "success",
      );
    } catch (err) {
      this.actions.notify(
        outcomeText(verb, dir, { state: "failed", reason: String(err) }),
        "error",
      );
    } finally {
      this.store.refreshRepos();
    }
  }
}
