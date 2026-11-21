import { ChangeDetectionStrategy, Component, inject, output } from "@angular/core";
import { HxButton } from "@gravionlabs/helix-ui";
import { agentName } from "../core/backend";
import { SessionResume, type AgentSessionInfo } from "../core/session-resume";
import type { Terminal } from "../core/snapshot";
import { DialogShell } from "../ui/dialog-shell/dialog-shell";

/** The sessions of a directory to resume (#986): start, last activity, first prompt and agent. */
@Component({
  selector: "app-resume-dialog",
  imports: [DialogShell, HxButton],
  changeDetection: ChangeDetectionStrategy.OnPush,
  templateUrl: "./resume-dialog.html",
  styleUrl: "./resume-dialog.css",
})
export class ResumeDialog {
  protected readonly resume = inject(SessionResume);
  /** A session was started in a new pane; the host opens it. */
  readonly created = output<Terminal>();

  protected agent(id: string): string {
    return agentName(id);
  }

  protected time(iso: string | null): string {
    const date = iso ? new Date(iso) : null;
    return date && !Number.isNaN(date.getTime()) ? date.toLocaleString() : "unknown";
  }

  protected async pick(session: AgentSessionInfo): Promise<void> {
    const pane = await this.resume.pick(session);
    if (pane) this.created.emit(pane);
  }
}
