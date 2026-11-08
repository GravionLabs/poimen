import { ChangeDetectionStrategy, Component, HostListener, computed, inject } from "@angular/core";
import { UiState } from "../../core/ui-state";
import { AgentRuns } from "../agent-runs";
import { AgentActions, pullTarget, type ActionTarget } from "../agent-actions";
import { GithubIssues } from "../issues";
import { renderMarkdown } from "../markdown";
import { GithubPulls, errorText, isGithubUrl, type LinkedIssue } from "../pulls";

/**
 * The detail pane of the selected pull request: description, checks, reviews and linked issues.
 * The action bar in the header is empty on purpose; later actions (merge, review) go there and
 * read the `viewerCan*` flags of the detail.
 */
@Component({
  selector: "app-pull-detail",
  templateUrl: "./pull-detail.html",
  styleUrl: "./pull-detail.css",
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class PullDetailPane {
  protected readonly pulls = inject(GithubPulls);
  private readonly issues = inject(GithubIssues);
  private readonly ui = inject(UiState);
  protected readonly agentActions = inject(AgentActions);
  protected readonly runs = inject(AgentRuns);
  protected readonly isGithubUrl = isGithubUrl;
  /** The pull request the agent actions run on. */
  protected readonly targets = computed<ActionTarget[]>(() => {
    const row = this.pulls.detailRow();
    return row ? [pullTarget(row)] : [];
  });
  protected readonly offered = computed(() => this.agentActions.offered(this.targets()));
  protected readonly errorText = errorText;
  protected readonly body = computed(() => renderMarkdown(this.pulls.detail()?.body ?? ""));
  protected readonly errorMessage = computed(() => {
    const err = this.pulls.detailError();
    return err === null ? null : typeof err === "string" ? err : errorText(err);
  });

  /** Selects the issue on the Issues sub-page. */
  protected openIssue(issue: LinkedIssue): void {
    this.issues.select(issue.repo, issue.number);
    this.ui.selectGithubPage("issues");
  }

  /** Links in a body open in the browser, never inside the app. */
  @HostListener("click", ["$event"])
  protected onBodyClick(event: MouseEvent): void {
    const target = event.target as HTMLElement;
    const link = target.closest<HTMLAnchorElement>(".markdown a");
    if (!link) return;
    event.preventDefault();
    void this.pulls.openUrl(link.getAttribute("href") ?? "");
  }

  protected stateLabel(state: string): string {
    return state.toLowerCase().replaceAll("_", " ");
  }
}
