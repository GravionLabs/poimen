import { ChangeDetectionStrategy, Component, inject } from "@angular/core";
import { HxButton } from "@gravionlabs/helix-ui";
import { DialogShell } from "../ui/dialog-shell/dialog-shell";
import { CreatePullRequest } from "./pull-request";

/** "Create pull request..." of the Changes view (#996): base, title, description and Draft. */
@Component({
  selector: "app-pull-request-dialog",
  imports: [DialogShell, HxButton],
  changeDetection: ChangeDetectionStrategy.OnPush,
  templateUrl: "./pull-request-dialog.html",
  styles: `
    .spacer {
      flex: 1;
    }
  `,
})
export class PullRequestDialog {
  protected readonly pr = inject(CreatePullRequest);
}
