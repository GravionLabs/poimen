import { Injectable, Injector, inject, signal } from "@angular/core";
import { SpaceActions, type TrustDecision } from "../core/space-actions";
import { SpacesStore } from "../core/spaces-store";
import { TauriBridge } from "../core/tauri";
import type { RepoMilestones } from "../github/milestones";
import { Deliver } from "./deliver";
import {
  MISSING_TEXT,
  milestoneName,
  type DeliverOutcome,
  type DeliverPlan,
  type DeliverRequest,
  type DeliverTarget,
} from "./deliver-start";

/** What the dialog is open for. */
export interface DeliverOpen {
  /** The repository root on this Mac. */
  root: string;
  /** The target the dialog starts with; a milestone target has a picker over `milestones`. */
  target: DeliverTarget;
  /** The open milestones of the repository (`M<nnn>`), for the picker; empty for issues. */
  milestones: string[];
}

/**
 * Starts a deliver loop from the app (#746): the dialog shows the target, the options and the
 * exact command, and "Start loop" types `deliver-loop …` into a new tab of the repository's space.
 * Only an explicit action starts a loop; nothing is scheduled and nothing restarts. The backend
 * (`deliver_start`) builds the command line from validated values and gates the project file
 * through the trust check; poimen-hosted sessions are a later step (ADR 0016).
 */
@Injectable({ providedIn: "root" })
export class DeliverLauncher {
  private readonly bridge = inject(TauriBridge);
  private readonly spaces = inject(SpaceActions);
  private readonly store = inject(SpacesStore);
  private readonly injector = inject(Injector);
  /** Looked up on use: the menus that open this dialog must not build the run watcher at once. */
  private get deliver(): Deliver {
    return this.injector.get(Deliver);
  }
  private readonly _open = signal<DeliverOpen | null>(null);
  /** The dialog's subject, or `null` while it is closed. */
  readonly opened = this._open.asReadonly();

  /**
   * Opens the dialog for the milestone `name` of the repository at `root`, or for the repository's
   * open milestones when `name` is empty (the palette entry). `false` when there is none to pick.
   */
  async openMilestone(root: string | undefined, name?: string): Promise<boolean> {
    if (!root) {
      this.spaces.notify("Deliver milestone: the active pane is not in a repository");
      return false;
    }
    const milestones = await this.openMilestones(root);
    const first = name ?? milestones[0];
    if (!first) {
      this.spaces.notify("Deliver milestone: the repository has no open milestone named M<nnn>");
      return false;
    }
    this._open.set({
      root,
      target: { kind: "milestone", name: first },
      milestones: milestones.includes(first) ? milestones : [first, ...milestones],
    });
    return true;
  }

  /** Opens the dialog for the issues `numbers` of the repository at `root`. */
  openIssues(root: string, numbers: number[]): void {
    this._open.set({
      root,
      target: { kind: "issues", numbers },
      milestones: [],
    });
  }

  close(): void {
    this._open.set(null);
  }

  /** The dialog's plan for `request`: the command line, the base and whether the command exists. */
  plan(root: string, request: DeliverRequest): Promise<DeliverPlan> {
    return this.bridge.invoke<DeliverPlan>("deliver_plan", {
      dir: root,
      request,
    });
  }

  /** "Show run" of a refusal: the Runs view with the repository's newest group under the cursor. */
  showRun(root: string): void {
    this.close();
    this.deliver.reveal(root);
  }

  /**
   * Starts the loop: the space of `root` (created or focused), then a new tab with the command.
   * An untrusted project file asks first and starts nothing until it is trusted. A second start
   * for a repository with a loop is refused here as well as in the dialog.
   */
  async start(root: string, request: DeliverRequest): Promise<boolean> {
    this.close();
    if (this.deliver.activeLoop(root)) return false;
    const outcome = await this.spaces.create(root, "", null, null, async (space) => {
      await this.run(space.space_id, root, request, null);
    });
    return outcome !== undefined;
  }

  private async run(
    spaceId: string,
    root: string,
    request: DeliverRequest,
    decision: TrustDecision | null,
  ): Promise<void> {
    let outcome: DeliverOutcome;
    try {
      outcome = await this.bridge.invoke<DeliverOutcome>("deliver_start", {
        spaceId,
        dir: root,
        request,
        decision,
      });
    } catch (err) {
      const reason = err && typeof err === "object" && "message" in err ? err.message : err;
      this.spaces.fail(`Deliver loop failed: ${String(reason)}`);
      return;
    }
    switch (outcome.kind) {
      case "started":
        this.deliver.markLaunched(root, outcome.terminal_id);
        await this.store.refresh();
        return;
      case "ask":
        this.spaces.askTrust(outcome.request, (answer) => this.run(spaceId, root, request, answer));
        return;
      case "missing":
        this.spaces.notify(`Deliver loop: ${MISSING_TEXT}`);
        return;
      case "declined":
        return;
    }
  }

  /** The open milestones named `M<nnn>` of the repository at `root`, lowest first. */
  private async openMilestones(root: string): Promise<string[]> {
    try {
      const result =
        (await this.bridge.invoke<RepoMilestones[]>("github_milestones", {
          dirs: [root],
        })) ?? [];
      const names = result.flatMap((r) =>
        r.status === "ok" ? r.milestones.map((m) => milestoneName(m.title)) : [],
      );
      return [...new Set(names.filter((n): n is string => n !== null))].sort();
    } catch {
      return [];
    }
  }
}
