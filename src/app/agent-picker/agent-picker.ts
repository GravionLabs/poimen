import { ChangeDetectionStrategy, Component, inject } from "@angular/core";
import { HxButton, HxList, HxListRow } from "@gravionlabs/helix-ui";
import { AgentPicker as PickerState } from "../core/agent-picker";
import { DialogShell } from "../ui/dialog-shell/dialog-shell";

/** Asks which agent of the space gets a project command as its prompt. */
@Component({
  selector: "app-agent-picker",
  imports: [DialogShell, HxButton, HxList, HxListRow],
  changeDetection: ChangeDetectionStrategy.OnPush,
  templateUrl: "./agent-picker.html",
})
export class AgentPicker {
  protected readonly state = inject(PickerState);
}
