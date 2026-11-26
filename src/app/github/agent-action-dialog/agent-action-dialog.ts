import { ChangeDetectionStrategy, Component, computed, inject } from "@angular/core";
import { AgentActions, hasReviewExtras } from "../agent-actions";
import { DialogShell } from "../../ui/dialog-shell/dialog-shell";
import { HxButton, HxField, HxSelect, HxCheckbox } from "@gravionlabs/helix-ui";
import { selectOptions } from "../../ui/select/select-options";

/**
 * Harness and model of an agent action, opened by Shift+click on the action. Prefilled with the
 * action's own choice, else the first harness of the registry; only available harnesses are listed.
 */
@Component({
  selector: "app-agent-action-dialog",
  imports: [DialogShell, HxButton, HxField, HxSelect, HxCheckbox],
  changeDetection: ChangeDetectionStrategy.OnPush,
  templateUrl: "./agent-action-dialog.html",
})
export class AgentActionDialog {
  protected readonly actions = inject(AgentActions);

  protected readonly modes = [
    { label: "Worktree", value: "worktree" },
    { label: "Diff only", value: "diff" },
  ];
  protected readonly places = [
    { label: "Repository root", value: "root" },
    { label: "Worktree", value: "worktree" },
  ];

  /** The available harnesses; without one, the preset as the only entry. */
  protected readonly harnessOptions = computed(() => {
    const known = this.actions.harnesses().map((h) => h.id);
    if (known.length) return selectOptions(known);
    const preset = this.actions.dialog()?.harness ?? "";
    return [{ label: preset || "(none available)", value: preset }];
  });

  /** The models of the chosen harness, plus the preset one when the registry does not list it. */
  protected readonly modelOptions = computed(() => {
    const d = this.actions.dialog();
    if (!d) return [];
    const known = this.actions.harnesses().find((h) => h.id === d.harness)?.models ?? [];
    const models = d.model && !known.includes(d.model) ? [d.model, ...known] : known;
    return [{ label: "(harness default)", value: "" }, ...selectOptions(models)];
  });

  protected targetsText(): string {
    return (this.actions.dialog()?.targets ?? []).map((t) => `#${t.number}`).join(", ");
  }

  protected harness(harness: string | null): void {
    this.actions.setDialog({ harness: harness ?? "", model: "" });
  }

  protected readonly hasReviewExtras = hasReviewExtras;

  protected where(value: string | null): void {
    this.actions.setDialog({
      where: value === "worktree" ? "worktree" : value === "diff" ? "diff" : "root",
    });
  }

  protected post(event: Event): void {
    this.actions.setDialog({ post: (event.target as HTMLInputElement).checked });
  }

  protected model(model: string | null): void {
    this.actions.setDialog({ model: model ?? "" });
  }
}
