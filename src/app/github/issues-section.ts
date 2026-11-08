import { ChangeDetectionStrategy, Component, HostListener, inject } from "@angular/core";
import {
  GithubIssues,
  LEVELS,
  issueProgress,
  labelChips,
  levelBadge,
  type Level,
  type StateFilter,
} from "./issues";
import { checkBadges, checksText, errorText, isGithubUrl } from "./pulls";

/** The "Issues" section of the GitHub tab: the issue tree per repository with linked pull requests. */
@Component({
  selector: "app-github-issues",
  templateUrl: "./issues-section.html",
  styleUrl: "./issues-section.css",
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class IssuesSection {
  protected readonly issues = inject(GithubIssues);
  protected readonly levels = LEVELS;
  protected readonly labelChips = labelChips;
  protected readonly levelBadge = levelBadge;
  protected readonly progress = issueProgress;
  protected readonly checkBadges = checkBadges;
  protected readonly errorText = errorText;
  protected readonly checksText = checksText;
  protected readonly isGithubUrl = isGithubUrl;

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
    if (row) void this.issues.openDetail(row.dataset["repo"] ?? "", Number(row.dataset["number"]));
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
