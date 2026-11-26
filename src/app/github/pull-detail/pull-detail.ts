import {
  ChangeDetectionStrategy,
  Component,
  HostListener,
  computed,
  effect,
  inject,
  signal,
} from "@angular/core";
import { UiState } from "../../core/ui-state";
import { AgentRuns } from "../agent-runs";
import { AgentActions, pullTarget, type ActionTarget } from "../agent-actions";
import { GithubIssues } from "../issues";
import { ChipPicker } from "../chip-picker/chip-picker";
import { renderMarkdown } from "../markdown";
import { checkLogKey, hasCheckLog } from "../check-log";
import {
  GithubPulls,
  MERGE_METHOD_LABELS,
  PR_ACTION_LABELS,
  canEditPeople,
  canMerge,
  canRerun,
  type MergeMethod,
  errorText,
  isGithubUrl,
  type LinkedIssue,
  type PrAction,
} from "../pulls";
import { HxButton, HxField, HxSelect } from "@gravionlabs/helix-ui";
import { Icon } from "../../ui/icon/icon";
import { LoadingRows } from "../../ui/loading/loading-rows";

/**
 * The detail pane of the selected pull request: description, checks, reviews and linked issues.
 * The action bar in the header is empty on purpose; later actions (merge, review) go there and
 * read the `viewerCan*` flags of the detail.
 */
@Component({
  selector: "app-pull-detail",
  templateUrl: "./pull-detail.html",
  styleUrl: "./pull-detail.css",
  imports: [LoadingRows, HxButton, ChipPicker, Icon, HxField, HxSelect],
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

  protected readonly labels = PR_ACTION_LABELS;
  protected readonly mergeLabels = MERGE_METHOD_LABELS;
  protected readonly mergeOptions = computed(() =>
    (this.pulls.detail()?.mergeMethods ?? []).map((value) => ({
      label: MERGE_METHOD_LABELS[value],
      value,
    })),
  );
  protected readonly canMerge = canMerge;
  protected readonly canRerun = canRerun;
  protected readonly canEdit = canEditPeople;
  protected readonly text = signal("");
  protected readonly hasCheckLog = hasCheckLog;
  /** URLs of the checks whose log is expanded. */
  protected readonly openLogs = signal<ReadonlySet<string>>(new Set());

  constructor() {
    // A draft belongs to one pull request: switching rows drops it.
    effect(() => {
      this.pulls.detailKey();
      this.text.set("");
      this.openLogs.set(new Set());
    });
  }

  protected logOf(url: string) {
    const sha = this.pulls.detail()?.headSha ?? "";
    return this.pulls.checkLogs().get(checkLogKey(sha, url)) ?? null;
  }

  protected toggleLog(url: string): void {
    const next = new Set(this.openLogs());
    if (next.delete(url)) {
      this.openLogs.set(next);
      return;
    }
    next.add(url);
    this.openLogs.set(next);
    void this.pulls.loadCheckLog(url);
  }

  protected choose(action: PrAction): void {
    this.text.set("");
    this.pulls.choose(action);
  }

  protected async submit(action: PrAction): Promise<void> {
    const body = this.text().trim();
    if (action !== "close" && !body) return;
    if (await this.pulls.act(action, action === "close" ? undefined : body)) this.text.set("");
  }

  protected setMethod(method: MergeMethod): void {
    const draft = this.pulls.mergeDraft();
    if (draft) this.pulls.mergeDraft.set({ ...draft, method });
  }

  protected setDelete(deleteBranch: boolean): void {
    const draft = this.pulls.mergeDraft();
    if (draft) this.pulls.mergeDraft.set({ ...draft, deleteBranch });
  }

  protected cancel(): void {
    this.pulls.compose.set(null);
  }

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
