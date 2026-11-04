import { ChangeDetectionStrategy, Component, HostListener, inject } from "@angular/core";
import { GithubIssues, LEVELS, markers, type Level, type StateFilter } from "./issues";
import { checksText, errorText, isGithubUrl } from "./pulls";

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
  protected readonly markers = markers;
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
