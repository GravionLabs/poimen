import { ChangeDetectionStrategy, Component, computed, inject } from "@angular/core";
import { UiState } from "../core/ui-state";
import { deriveInbox, type InboxEntry } from "./inbox";
import { GithubFilter } from "./github-filter";
import { GithubIssues } from "./issues";
import { GithubPulls, flattenPulls } from "./pulls";

/** The "Inbox" sub-page: what waits for the viewer, from the loaded pull requests and issues. */
@Component({
  selector: "app-github-inbox",
  templateUrl: "./inbox-section.html",
  styleUrl: "./inbox-section.css",
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class InboxSection {
  private readonly pulls = inject(GithubPulls);
  private readonly issues = inject(GithubIssues);
  private readonly filter = inject(GithubFilter);
  private readonly ui = inject(UiState);

  protected readonly sections = computed(() =>
    deriveInbox(this.pulls.result(), this.issues.result()),
  );
  protected readonly loaded = computed(() => this.pulls.result() !== null);

  /** Switches to the sub-page of the entry with the item selected and its pane open. */
  protected open(entry: InboxEntry): void {
    this.filter.repo.set(null);
    if (entry.kind === "pull") {
      const row = flattenPulls(this.pulls.result()?.repos ?? []).find(
        (r) => r.repo === entry.repo && r.number === entry.number,
      );
      if (!row) return;
      this.pulls.scope.set("all");
      this.pulls.text.set("");
      this.ui.selectGithubPage("pulls");
      void this.pulls.openDetail(row);
    } else {
      this.issues.state.set("open");
      this.issues.label.set(null);
      this.issues.level.set(null);
      this.issues.assignee.set(null);
      this.issues.text.set("");
      this.ui.selectGithubPage("issues");
      void this.issues.openDetail(entry.repo, entry.number);
    }
  }
}
