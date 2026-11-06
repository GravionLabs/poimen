import { ChangeDetectionStrategy, Component, inject } from "@angular/core";
import { AgentPicker as PickerState } from "../core/agent-picker";

/** Asks which agent of the space gets a project command as its prompt. */
@Component({
  selector: "app-agent-picker",
  changeDetection: ChangeDetectionStrategy.OnPush,
  templateUrl: "./agent-picker.html",
  styleUrl: "./agent-picker.css",
})
export class AgentPicker {
  protected readonly state = inject(PickerState);
}
