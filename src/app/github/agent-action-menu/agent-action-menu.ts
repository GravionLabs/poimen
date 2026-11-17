import { ChangeDetectionStrategy, Component, HostListener, computed, inject } from "@angular/core";
import { AgentActions, type ActionTarget } from "../agent-actions";
import { IssueTasks } from "../issue-task";
import { HxButton } from "@gravionlabs/helix-ui";

/**
 * The popup of the agent actions of a row (the `⋯` button or the context menu). Shift+click on an
 * entry opens the harness and model dialog. Escape or a click elsewhere closes it.
 */
@Component({
  selector: "app-agent-action-menu",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [HxButton],
  templateUrl: "./agent-action-menu.html",
  styleUrl: "./agent-action-menu.css",
})
export class AgentActionMenu {
  protected readonly actions = inject(AgentActions);
  protected readonly issueTasks = inject(IssueTasks);
  /** The open issue of the menu: it gets "Start task…" or "Open space" above the agent actions. */
  protected readonly openIssue = computed<ActionTarget | null>(() => {
    const t = this.actions.menu()?.targets ?? [];
    return t.length === 1 && t[0].kind === "issue" && t[0].state === "open" ? t[0] : null;
  });
  protected readonly offered = computed(() => {
    const m = this.actions.menu();
    return m ? this.actions.offered(m.targets) : [];
  });

  protected startTask(issue: ActionTarget): void {
    this.actions.closeMenu();
    void this.issueTasks.start(issue.root, issue.number);
  }

  @HostListener("document:keydown.escape")
  protected onEscape(): void {
    this.actions.closeMenu();
  }

  @HostListener("document:click", ["$event"])
  protected onDocumentClick(event: MouseEvent): void {
    // The `⋯` button opens in its own click handler; that click must not close it again.
    if (event.target instanceof HTMLElement && event.target.closest("[data-action-menu-button]"))
      return;
    this.actions.closeMenu();
  }
}
