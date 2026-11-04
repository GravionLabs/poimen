import { ChangeDetectionStrategy, Component, HostListener, inject } from "@angular/core";
import { IssuesSection } from "./issues-section";
import { MilestonesSection } from "./milestones-section";
import {
  GithubPulls,
  SCOPES,
  ageText,
  checksText,
  errorText,
  isGithubUrl,
  reviewText,
  type PullRow,
} from "./pulls";

/** The GitHub pull request view as an editor tab: open pull requests of the known repositories. */
@Component({
  selector: "app-github-view",
  imports: [MilestonesSection, IssuesSection],
  templateUrl: "./github-view.html",
  styleUrl: "./github-view.css",
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class GithubView {
  protected readonly pulls = inject(GithubPulls);
  protected readonly scopes = SCOPES;
  protected readonly errorText = errorText;
  protected readonly checksText = checksText;
  protected readonly reviewText = reviewText;
  protected readonly isGithubUrl = isGithubUrl;

  constructor() {
    void this.pulls.load(false);
  }

  /** The tab regained focus: read again, but never faster than the 30 s cache. */
  @HostListener("window:focus")
  protected onFocus(): void {
    void this.pulls.load(false);
  }

  protected refresh(): void {
    void this.pulls.load();
  }

  protected setRepo(value: string): void {
    this.pulls.repoFilter.set(value || null);
  }

  protected age(row: PullRow): string {
    return ageText(row.createdAt, Date.now());
  }

  /** The checkbox: Shift selects the range since the last click. */
  protected onCheck(event: MouseEvent, key: string): void {
    this.pulls.select(key, event.shiftKey ? "range" : "toggle");
  }

  /** Up and Down move the cursor row, Space selects it (Shift+Space the range since the anchor). */
  protected onKey(event: KeyboardEvent): void {
    // Inside a checkbox or button the key keeps its own meaning.
    if (event.target instanceof HTMLElement && event.target.closest("input, button")) return;
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      this.pulls.moveCursor(event.key === "ArrowDown" ? 1 : -1);
    } else if (event.key === " " && this.pulls.cursor()) {
      event.preventDefault();
      this.pulls.select(this.pulls.cursor()!, event.shiftKey ? "range" : "toggle");
    }
  }
}
