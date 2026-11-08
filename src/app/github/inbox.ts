import type { RepoIssues } from "./issues";
import type { PullsResult } from "./pulls";

export type InboxSectionId = "review" | "mine" | "issues";

/** One thing that waits for the viewer. */
export interface InboxEntry {
  kind: "pull" | "issue";
  repo: string;
  number: number;
  title: string;
  url: string;
  /** Why it is here, e.g. `Checks failed`. */
  reason: string;
}

export interface InboxSection {
  id: InboxSectionId;
  label: string;
  entries: InboxEntry[];
}

const same = (a: string | null, b: string): boolean =>
  a !== null && a.toLowerCase() === b.toLowerCase();

/**
 * What waits for the viewer, from the lists already loaded (no query of its own). Empty sections
 * are left out; without a viewer there is nothing to collect.
 */
export function deriveInbox(
  pulls: PullsResult | null,
  issues: readonly RepoIssues[] | null,
): InboxSection[] {
  const viewer = pulls?.viewer ?? null;
  if (viewer === null) return [];
  const review: InboxEntry[] = [];
  const mine: InboxEntry[] = [];
  for (const r of pulls?.repos ?? []) {
    if (r.status !== "ok") continue;
    for (const p of r.pulls) {
      const base = {
        kind: "pull" as const,
        repo: r.repo,
        number: p.number,
        title: p.title,
        url: p.url,
      };
      if (p.reviewRequests.some((l) => same(l, viewer))) {
        review.push({ ...base, reason: "Review requested" });
      }
      if (same(p.author, viewer)) {
        const reasons: string[] = [];
        if (p.checks.state === "failing") reasons.push("Checks failed");
        if (p.reviewDecision === "CHANGES_REQUESTED") reasons.push("Changes requested");
        if (reasons.length) mine.push({ ...base, reason: reasons.join(", ") });
      }
    }
  }
  const assigned: InboxEntry[] = [];
  for (const r of issues ?? []) {
    if (r.status !== "ok") continue;
    for (const i of r.issues) {
      if (i.state === "OPEN" && i.assignees.some((a) => same(a, viewer))) {
        assigned.push({
          kind: "issue",
          repo: r.repo,
          number: i.number,
          title: i.title,
          url: i.url,
          reason: "Assigned",
        });
      }
    }
  }
  const sections: InboxSection[] = [
    { id: "review", label: "Review requested", entries: review },
    { id: "mine", label: "My pull requests that need action", entries: mine },
    { id: "issues", label: "Issues assigned to me", entries: assigned },
  ];
  return sections.filter((s) => s.entries.length > 0);
}

/** The navigation count: the entries over all sections. */
export function inboxCount(sections: readonly InboxSection[]): number {
  return sections.reduce((n, s) => n + s.entries.length, 0);
}
