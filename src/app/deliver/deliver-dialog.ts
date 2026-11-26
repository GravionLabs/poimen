import {
  ChangeDetectionStrategy,
  Component,
  computed,
  effect,
  inject,
  signal,
  untracked,
} from "@angular/core";
import { HxButton, HxField, HxSelect } from "@gravionlabs/helix-ui";
import { DialogShell } from "../ui/dialog-shell/dialog-shell";
import { DeliverLauncher } from "./deliver-launcher";
import { Deliver } from "./deliver";
import {
  HARNESSES,
  MISSING_TEXT,
  activeText,
  parseMax,
  requestOf,
  targetError,
  targetText,
  type DeliverPlan,
  type DeliverTarget,
} from "./deliver-start";
import { selectOptions } from "../ui/select/select-options";

/**
 * The dialog that shows before a deliver loop starts (#746): the target, the harness, the model,
 * `--max`, the base branch and the exact command line. Without `deliver-loop` on the `PATH`, or
 * with a loop already running for the repository, it says so and offers no start.
 */
@Component({
  selector: "app-deliver-dialog",
  imports: [DialogShell, HxButton, HxField, HxSelect],
  changeDetection: ChangeDetectionStrategy.OnPush,
  templateUrl: "./deliver-dialog.html",
  styleUrl: "./deliver-dialog.css",
})
export class DeliverDialog {
  protected readonly launcher = inject(DeliverLauncher);
  private readonly deliver = inject(Deliver);
  protected readonly harnessOptions = [
    { label: "loop default", value: "" },
    ...selectOptions(HARNESSES),
  ];
  protected readonly milestoneOptions = computed(() =>
    selectOptions(this.launcher.opened()?.milestones ?? []),
  );
  protected readonly missing = MISSING_TEXT;
  protected readonly targetText = targetText;

  protected readonly milestone = signal<string | null>(null);
  protected readonly harness = signal("");
  protected readonly model = signal("");
  protected readonly max = signal("");
  protected readonly plan = signal<DeliverPlan | null>(null);
  private seq = 0;

  /** The target with the milestone the picker has chosen. */
  protected readonly target = computed<DeliverTarget | null>(() => {
    const open = this.launcher.opened();
    if (!open) return null;
    const picked = this.milestone();
    return open.target.kind === "milestone" && picked
      ? { kind: "milestone", name: picked }
      : open.target;
  });

  protected readonly request = computed(() => {
    const target = this.target();
    return target
      ? requestOf(target, {
          harness: this.harness(),
          model: this.model(),
          max: this.max(),
        })
      : null;
  });

  /** The refusal of a second start, else `null`. */
  protected readonly refusal = computed(() => {
    const open = this.launcher.opened();
    const active = open ? this.deliver.activeLoop(open.root) : undefined;
    return active ? activeText(active) : null;
  });

  /** Why the values cannot start, else `null`. */
  protected readonly invalid = computed(() => {
    const target = this.target();
    if (!target) return null;
    const error = targetError(target);
    if (error) return error;
    return parseMax(this.max()).ok ? null : "--max is a positive whole number.";
  });

  protected readonly canStart = computed(
    () =>
      this.request() !== null &&
      this.plan()?.available === true &&
      this.plan()?.error === null &&
      this.refusal() === null,
  );

  constructor() {
    // A new dialog starts from its own target and empty options.
    effect(() => {
      const open = this.launcher.opened();
      untracked(() => {
        this.milestone.set(open?.target.kind === "milestone" ? open.target.name : null);
        this.harness.set("");
        this.model.set("");
        this.max.set("");
        this.plan.set(null);
      });
    });
    // The plan (command line, base, whether the command exists) follows the values.
    effect(() => {
      const open = this.launcher.opened();
      const request = this.request();
      const target = this.target();
      if (!open || !target) return;
      const seq = ++this.seq;
      // An invalid value still asks, to learn whether the command exists.
      const asked = request ?? { target };
      void this.launcher
        .plan(open.root, asked)
        .then((plan) => seq === this.seq && this.plan.set(plan))
        .catch(() => seq === this.seq && this.plan.set(null));
    });
  }

  protected value(event: Event): string {
    return (event.target as HTMLInputElement).value;
  }

  protected start(): void {
    const open = this.launcher.opened();
    const request = this.request();
    if (open && request && this.canStart()) void this.launcher.start(open.root, request);
  }

  protected showRun(): void {
    const open = this.launcher.opened();
    if (open) this.launcher.showRun(open.root);
  }
}
