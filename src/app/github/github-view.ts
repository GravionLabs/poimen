import {
  ChangeDetectionStrategy,
  Component,
  HostListener,
  computed,
  effect,
  inject,
} from "@angular/core";
import { UiState, type GithubPage } from "../core/ui-state";
import { AgentActionMenu } from "./agent-action-menu/agent-action-menu";
import { AgentRuns } from "./agent-runs";
import { AgentActions, pullTarget } from "./agent-actions";
import { GithubIssues } from "./issues";
import { GithubMilestones } from "./milestones";
import { IssuesSection } from "./issues-section";
import { GithubFilter } from "./github-filter";
import { IssueDetailPane } from "./issue-detail/issue-detail";
import { PullDetailPane } from "./pull-detail/pull-detail";
import { MilestonesSection } from "./milestones-section";
import { InboxSection } from "./inbox-section";
import { deriveInbox, inboxCount } from "./inbox";
import {
  GithubPulls,
  SCOPES,
  ageText,
  checkBadges,
  checksText,
  errorText,
  isGithubUrl,
  reviewBadge,
  type PullRow,
} from "./pulls";

/** The GitHub pull request view as an editor tab: open pull requests of the known repositories. */
@Component({
  selector: "app-github-view",
  imports: [
    AgentActionMenu,
    InboxSection,
    MilestonesSection,
    IssuesSection,
    PullDetailPane,
    IssueDetailPane,
  ],
  templateUrl: "./github-view.html",
  styleUrl: "./github-view.css",
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class GithubView {
  protected readonly pulls = inject(GithubPulls);
  protected readonly issues = inject(GithubIssues);
  protected readonly milestones = inject(GithubMilestones);
  protected readonly filter = inject(GithubFilter);
  private readonly agentActions = inject(AgentActions);
  protected readonly runs = inject(AgentRuns);
  protected readonly ui = inject(UiState);
  /** The sub-pages of the navigation, the Inbox in front. */
  protected readonly pages = computed<{ id: GithubPage; label: string; count: number | null }[]>(
    () => [
      {
        id: "inbox",
        label: "Inbox",
        count: this.pulls.result()
          ? inboxCount(deriveInbox(this.pulls.result(), this.issues.result()))
          : null,
      },
      {
        id: "pulls",
        label: "Pull requests",
        count: this.pulls.result() ? this.pulls.visible().length : null,
      },
      { id: "issues", label: "Issues", count: this.issues.result() ? this.issues.count() : null },
      {
        id: "milestones",
        label: "Milestones",
        count: this.milestones.result() ? this.milestones.count() : null,
      },
    ],
  );
  /** `All repositories` plus every repository any of the three lists knows. */
  protected readonly repoNames = computed(() =>
    [
      ...new Set([
        ...this.pulls.repoNames(),
        ...this.issues.repoNames(),
        ...this.milestones.repoNames(),
      ]),
    ].sort(),
  );
  protected readonly scopes = SCOPES;
  protected readonly errorText = errorText;
  protected readonly checksText = checksText;
  protected readonly reviewBadge = reviewBadge;
  protected readonly checkBadges = checkBadges;
  protected readonly isGithubUrl = isGithubUrl;

  constructor() {
    this.loadAll(false);
    effect(() => this.agentActions.ensureLoaded());
  }

  /** Escape closes the detail pane of a pull request or an issue. */
  @HostListener("keydown.escape")
  protected onEscape(): void {
    this.pulls.closeDetail();
    this.issues.closeDetail();
  }

  /** The tab regained focus: read again, but never faster than the 30 s cache. */
  @HostListener("window:focus")
  protected onFocus(): void {
    this.loadAll(false);
  }

  /** The counts of the navigation need all three lists. */
  private loadAll(force: boolean): void {
    void this.pulls.load(force);
    void this.issues.load(force);
    void this.milestones.load(force);
  }

  /** Reloads the active sub-page. */
  protected refresh(): void {
    const page = this.ui.githubPage();
    if (page === "inbox") this.loadAll(true);
    else if (page === "pulls") void this.pulls.load();
    else if (page === "issues") void this.issues.load();
    else void this.milestones.load();
  }

  protected get loading(): boolean {
    const page = this.ui.githubPage();
    return page === "inbox"
      ? this.pulls.loading() || this.issues.loading()
      : page === "pulls"
        ? this.pulls.loading()
        : page === "issues"
          ? this.issues.loading()
          : this.milestones.loading();
  }

  protected setRepo(value: string): void {
    this.filter.repo.set(value || null);
  }

  protected selectPage(page: GithubPage): void {
    this.ui.selectGithubPage(page);
  }

  /** Up and Down move to the previous or next sub-page and select it; Enter selects the focused one. */
  protected onNavKey(event: KeyboardEvent, page: GithubPage): void {
    const ids = this.pages().map((p) => p.id);
    const at = ids.indexOf(page);
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      const next =
        ids[Math.min(ids.length - 1, Math.max(0, at + (event.key === "ArrowDown" ? 1 : -1)))];
      this.selectPage(next);
      (event.currentTarget as HTMLElement | null)?.parentElement
        ?.querySelector<HTMLElement>(`[data-page="${next}"]`)
        ?.focus();
    } else if (event.key === "Enter") {
      event.preventDefault();
      this.selectPage(page);
    }
  }

  /** The `⋯` button or the context menu of a row: the actions for it, or for the whole selection. */
  protected openRowMenu(event: MouseEvent, row: PullRow): void {
    event.preventDefault();
    const selected = this.pulls.selectedPulls();
    const rows = selected.length > 1 && selected.some((r) => r.key === row.key) ? selected : [row];
    this.agentActions.openMenu(rows.map(pullTarget), event.clientX, event.clientY);
  }

  protected age(row: PullRow): string {
    return ageText(row.createdAt, Date.now());
  }

  /** The checkbox: Shift selects the range since the last click. */
  protected onCheck(event: MouseEvent, key: string): void {
    this.pulls.select(key, event.shiftKey ? "range" : "toggle");
  }

  /** A click on a row (not on its checkbox or buttons) opens the detail pane. */
  protected onRowClick(event: MouseEvent, row: PullRow): void {
    if (event.target instanceof HTMLElement && event.target.closest("input, button")) return;
    void this.pulls.openDetail(row);
  }

  /** Up and Down move the cursor row, Space selects it (Shift+Space the range since the anchor). */
  protected onKey(event: KeyboardEvent): void {
    // Inside a checkbox or button the key keeps its own meaning.
    if (event.target instanceof HTMLElement && event.target.closest("input, button")) return;
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      this.pulls.moveCursor(event.key === "ArrowDown" ? 1 : -1);
    } else if (event.key === "Enter" && this.pulls.cursor()) {
      const row = this.pulls.rows().find((r) => r.key === this.pulls.cursor());
      if (row) {
        event.preventDefault();
        void this.pulls.openDetail(row);
      }
    } else if (event.key === " " && this.pulls.cursor()) {
      event.preventDefault();
      this.pulls.select(this.pulls.cursor()!, event.shiftKey ? "range" : "toggle");
    }
  }
}
