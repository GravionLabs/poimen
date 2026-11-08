import { ChangeDetectionStrategy, Component, computed, inject } from "@angular/core";
import { AgentActions } from "../agent-actions";

/**
 * Harness and model of an agent action, opened by Shift+click on the action. Prefilled with the
 * action's own choice, else the first harness of the registry; only available harnesses are listed.
 */
@Component({
  selector: "app-agent-action-dialog",
  changeDetection: ChangeDetectionStrategy.OnPush,
  templateUrl: "./agent-action-dialog.html",
  styleUrl: "./agent-action-dialog.css",
})
export class AgentActionDialog {
  protected readonly actions = inject(AgentActions);

  /** The models of the chosen harness, plus the preset one when the registry does not list it. */
  protected readonly models = computed(() => {
    const d = this.actions.dialog();
    if (!d) return [];
    const known = this.actions.harnesses().find((h) => h.id === d.harness)?.models ?? [];
    return d.model && !known.includes(d.model) ? [d.model, ...known] : known;
  });

  protected targetsText(): string {
    return (this.actions.dialog()?.targets ?? []).map((t) => `#${t.number}`).join(", ");
  }

  protected harness(event: Event): void {
    this.actions.setDialog({ harness: (event.target as HTMLSelectElement).value, model: "" });
  }

  protected where(event: Event): void {
    const value = (event.target as HTMLSelectElement).value;
    this.actions.setDialog({ where: value === "worktree" ? "worktree" : "root" });
  }

  protected model(event: Event): void {
    this.actions.setDialog({ model: (event.target as HTMLSelectElement).value });
  }

  protected onKey(event: KeyboardEvent): void {
    if (event.key === "Escape") {
      event.preventDefault();
      this.actions.cancelDialog();
    }
  }
}
