import { NgTemplateOutlet } from "@angular/common";
import { ChangeDetectionStrategy, Component, HostListener, inject } from "@angular/core";
import { GithubMilestones, dueText, progressPercent, readyToClose } from "./milestones";
import { errorText, isGithubUrl } from "./pulls";
import { HxButton, HxField, HxInput, HxMessage } from "@gravionlabs/helix-ui";
import { LoadingRows } from "../ui/loading/loading-rows";

/** The "Milestones" section of the GitHub tab: open milestones with progress; selecting filters the issues. */
@Component({
  selector: "app-github-milestones",
  imports: [LoadingRows, HxButton, NgTemplateOutlet, HxField, HxInput, HxMessage],
  templateUrl: "./milestones-section.html",
  styleUrl: "./milestones-section.css",
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class MilestonesSection {
  protected readonly milestones = inject(GithubMilestones);
  protected readonly errorText = errorText;
  protected readonly isGithubUrl = isGithubUrl;
  protected readonly dueText = dueText;
  protected readonly readyToClose = readyToClose;
  protected readonly percent = progressPercent;

  constructor() {
    void this.milestones.load(false);
  }

  @HostListener("window:focus")
  protected onFocus(): void {
    void this.milestones.load(false);
  }
}
