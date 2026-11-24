import { ChangeDetectionStrategy, Component, inject, output } from "@angular/core";
import { HxButton, HxTag } from "@gravionlabs/helix-ui";
import { AgentGrants, type ActiveGrant } from "../core/agent-grants";
import { Deliver } from "./deliver";
import { memberSeverity, type MemberState } from "./lead-run";

/**
 * The lead runs of the Runs view (ADR 0018, #1213): per repository its deliver run, state, current
 * task and blocked sessions, and the dependencies across repositories with the open ones. It reads
 * the run file of the lead through `Deliver.leadRuns` and stores nothing of its own; the active
 * grant is the server's, revoked through `AgentGrants`.
 */
@Component({
  selector: "app-lead-runs",
  templateUrl: "./lead-runs.html",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [HxButton, HxTag],
})
export class LeadRuns {
  protected readonly deliver = inject(Deliver);
  protected readonly grants = inject(AgentGrants);
  protected readonly severity = memberSeverity;
  /** A terminal of the lead run was asked for. */
  readonly paneChosen = output<string>();

  protected stateLabel(state: MemberState): string {
    return state === "planned" ? "not started" : state;
  }

  protected grantText(grant: ActiveGrant): string {
    const names = grant.spaces.map((s) => s.name).join(", ") || "no space";
    return `may write in ${names}, ${grant.running} of ${grant.maxAgents} agents running`;
  }
}
