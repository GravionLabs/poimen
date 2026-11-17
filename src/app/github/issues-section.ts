import { ChangeDetectionStrategy, Component, HostListener, inject } from "@angular/core";
import { AgentRuns } from "./agent-runs";
import { AgentActions, issueTarget } from "./agent-actions";
import {
  GithubIssues,
  issueKey,
  type Issue,
  LEVELS,
  issueProgress,
  labelChips,
  levelBadge,
  type Level,
  type StateFilter,
} from "./issues";
import { IssueFormPane } from "./issue-form/issue-form";
import { checkBadges, checksText, errorText, isGithubUrl } from "./pulls";
import { HxButton } from "@gravionlabs/helix-ui";

/** The "Issues" section of the GitHub tab: the issue tree per repository with linked pull requests. */
@Component({
  selector: "app-github-issues",
  templateUrl: "./issues-section.html",
  imports: [HxButton, IssueFormPane],
  styleUrl: "./issues-section.css",
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class IssuesSection {
  protected readonly issues = inject(GithubIssues);
  private readonly agentActions = inject(AgentActions);
  protected readonly runs = inject(AgentRuns);
  protected readonly levels = LEVELS;
  protected readonly labelChips = labelChips;
  protected readonly levelBadge = levelBadge;
  protected readonly progress = issueProgress;
  protected readonly checkBadges = checkBadges;
  protected readonly errorText = errorText;
  protected readonly checksText = checksText;
  protected readonly isGithubUrl = isGithubUrl;

  /** The `⋯` button or the context menu of an issue row; on a marked row, for the whole selection. */
  protected openRowMenu(event: MouseEvent, repo: string, root: string, issue: Issue): void {
    event.preventDefault();
    const marked = this.issues.markedIssues();
    const targets =
      marked.length > 1 && marked.some((m) => m.repo === repo && m.issue.number === issue.number)
        ? marked.map((m) => issueTarget(m.repo, m.root, m.issue))
        : [issueTarget(repo, root, issue)];
    this.agentActions.openMenu(targets, event.clientX, event.clientY);
  }

  /** The checkbox: Shift marks the range since the last click. */
  protected onCheck(event: MouseEvent, repo: string, number: number): void {
    this.issues.mark(issueKey(repo, number), event.shiftKey ? "range" : "toggle");
    void this.issues.loadBulkOptions();
  }

  protected async bulkMilestone(title: string): Promise<void> {
    if (title) await this.issues.bulkEdit({ milestone: title });
  }

  protected async bulkAddLabel(name: string): Promise<void> {
    if (name) await this.issues.bulkEdit({ addLabels: [name] });
  }

  protected async bulkRemoveLabel(name: string): Promise<void> {
    if (name) await this.issues.bulkEdit({ removeLabels: [name] });
  }

  protected async bulkClose(reason: string): Promise<void> {
    if (reason === "completed" || reason === "not_planned") await this.issues.bulkClose(reason);
  }

  constructor() {
    void this.issues.load(false);
  }

  @HostListener("window:focus")
  protected onFocus(): void {
    void this.issues.load(false);
  }

  /** Up and Down move the cursor row, Enter opens its detail pane. */
  protected onKey(event: KeyboardEvent): void {
    // Inside a filter or button the key keeps its own meaning.
    if (event.target instanceof HTMLElement && event.target.closest("input, button, select"))
      return;
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      this.issues.moveCursor(event.key === "ArrowDown" ? 1 : -1);
    } else if (event.key === " " && this.issues.cursor()) {
      event.preventDefault();
      this.issues.mark(this.issues.cursor()!, event.shiftKey ? "range" : "toggle");
      void this.issues.loadBulkOptions();
    } else if (event.key === "Enter" && this.issues.cursor()) {
      event.preventDefault();
      this.issues.openCursor();
    }
  }

  /** A click on a row (not on its buttons) puts the cursor there and opens the pane. */
  protected onClick(event: MouseEvent): void {
    if (!(event.target instanceof HTMLElement) || event.target.closest("input, button, select")) {
      return;
    }
    const row = event.target.closest<HTMLElement>("li.row[data-number]");
    if (!row) return;
    // Cmd/Ctrl-click and Shift-click select instead of opening the pane.
    if (event.metaKey || event.ctrlKey || event.shiftKey) {
      const repo = row.dataset["repo"] ?? "";
      this.onCheck(event, repo, Number(row.dataset["number"]));
      return;
    }
    void this.issues.openDetail(row.dataset["repo"] ?? "", Number(row.dataset["number"]));
  }

  protected value(event: Event): string {
    return (event.target as HTMLInputElement | HTMLSelectElement).value;
  }

  protected setLevel(value: string): void {
    this.issues.level.set((value || null) as Level | null);
  }

  protected setState(value: string): void {
    void this.issues.setState(value as StateFilter);
  }
}
