import { ChangeDetectionStrategy, Component, HostListener, computed, inject } from "@angular/core";
import { UiState } from "../../core/ui-state";
import { AgentRuns } from "../agent-runs";
import { AgentActions, issueTarget, type ActionTarget } from "../agent-actions";
import { ChipPicker } from "../chip-picker/chip-picker";
import {
  GithubIssues,
  ISSUE_ACTION_LABELS,
  canEditIssue,
  issueKey,
  issueLevel,
  levelBadge,
  type ClosingPr,
} from "../issues";
import { renderMarkdown } from "../markdown";
import { GithubPulls, errorText, isGithubUrl } from "../pulls";
import { IssueTasks } from "../issue-task";

/**
 * The detail pane of the selected issue: description, labels, assignees, milestone, sub-issues
 * and the pull requests that close it. Same shell (`detail-pane.css`) as the pull request pane;
 * the action bar stays empty until actions read `viewerCanUpdate`.
 */
@Component({
  selector: "app-issue-detail",
  templateUrl: "./issue-detail.html",
  styleUrl: "./issue-detail.css",
  imports: [ChipPicker],
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class IssueDetailPane {
  protected readonly issues = inject(GithubIssues);
  private readonly pulls = inject(GithubPulls);
  private readonly ui = inject(UiState);
  protected readonly isGithubUrl = isGithubUrl;
  protected readonly levelBadge = levelBadge;
  protected readonly body = computed(() => renderMarkdown(this.issues.detail()?.body ?? ""));
  protected readonly canEdit = canEditIssue;
  protected readonly actionLabels = ISSUE_ACTION_LABELS;
  protected readonly preview = computed(() => renderMarkdown(this.issues.bodyDraft() ?? ""));
  protected readonly errorMessage = computed(() => {
    const err = this.issues.detailError();
    return err === null ? null : typeof err === "string" ? err : errorText(err);
  });

  protected readonly agentActions = inject(AgentActions);
  protected readonly runs = inject(AgentRuns);

  /** The issue the agent actions run on (as the list knows it). */
  protected readonly targets = computed<ActionTarget[]>(() => {
    const sel = this.issues.detailIssue();
    const root = (this.issues.result() ?? []).find((r) => r.repo === sel?.repo)?.root;
    return sel && root ? [issueTarget(sel.repo, root, sel.issue)] : [];
  });
  protected readonly offered = computed(() => this.agentActions.offered(this.targets()));
  protected readonly issueTasks = inject(IssueTasks);
  /** The open issue, or null: only an open issue gets "Start task…" or "Open space". */
  protected readonly startable = computed<ActionTarget | null>(() => {
    const [t] = this.targets();
    return t?.state === "open" ? t : null;
  });

  protected startTask(issue: ActionTarget): void {
    void this.issueTasks.start(issue.root, issue.number);
  }

  /** What the header shows: the list row while the detail loads, then the detail. */
  protected readonly head = computed(() => {
    const key = this.issues.detailKey();
    if (key === null) return null;
    const repo = key.slice(0, key.lastIndexOf("#"));
    const d = this.issues.detail();
    const listed = this.issues.detailIssue()?.issue;
    const src = d ?? listed;
    if (!src)
      return {
        repo,
        number: Number(key.slice(key.lastIndexOf("#") + 1)),
        title: "",
        url: "",
        level: null,
      };
    return { repo, number: src.number, title: src.title, url: src.url, level: issueLevel(src) };
  });

  /** Selects a sub-issue or the parent: its detail opens in this pane. */
  protected openIssue(repo: string, number: number): void {
    void this.issues.openDetail(repo, number);
  }

  protected canOpen(repo: string): boolean {
    return this.issues.knows(repo);
  }

  /** Switches to the Pull requests sub-page and opens the pull request when the list has it. */
  protected openPull(pr: ClosingPr): void {
    this.ui.selectGithubPage("pulls");
    const row = this.pulls.rows().find((r) => r.key === issueKey(pr.repo, pr.number));
    if (row) void this.pulls.openDetail(row);
  }

  /** Links in a body open in the browser, never inside the app. */
  @HostListener("click", ["$event"])
  protected onBodyClick(event: MouseEvent): void {
    const target = event.target as HTMLElement;
    const link = target.closest<HTMLAnchorElement>(".markdown a");
    if (!link) return;
    event.preventDefault();
    void this.issues.openUrl(link.getAttribute("href") ?? "");
  }

  protected setTitle(value: string): void {
    this.issues.titleDraft.set(value);
  }

  protected setBody(value: string): void {
    this.issues.bodyDraft.set(value);
  }

  protected stateLabel(state: string): string {
    return state.toLowerCase().replaceAll("_", " ");
  }
}
