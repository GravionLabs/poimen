import { ChangeDetectionStrategy, Component, HostListener, inject } from "@angular/core";
import { GithubMilestones, dueText, progressPercent } from "./milestones";
import { errorText, isGithubUrl } from "./pulls";

/** The "Milestones" section of the GitHub tab: open milestones with progress; selecting filters the issues. */
@Component({
  selector: "app-github-milestones",
  templateUrl: "./milestones-section.html",
  styleUrl: "./milestones-section.css",
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class MilestonesSection {
  protected readonly milestones = inject(GithubMilestones);
  protected readonly errorText = errorText;
  protected readonly isGithubUrl = isGithubUrl;
  protected readonly dueText = dueText;
  protected readonly percent = progressPercent;

  constructor() {
    void this.milestones.load(false);
  }

  @HostListener("window:focus")
  protected onFocus(): void {
    void this.milestones.load(false);
  }
}
