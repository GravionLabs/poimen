import { ChangeDetectionStrategy, Component, HostListener, computed, inject } from "@angular/core";
import { AgentActions } from "../agent-actions";

/**
 * The popup of the agent actions of a row (the `⋯` button or the context menu). Shift+click on an
 * entry opens the harness and model dialog. Escape or a click elsewhere closes it.
 */
@Component({
  selector: "app-agent-action-menu",
  changeDetection: ChangeDetectionStrategy.OnPush,
  templateUrl: "./agent-action-menu.html",
  styleUrl: "./agent-action-menu.css",
})
export class AgentActionMenu {
  protected readonly actions = inject(AgentActions);
  protected readonly offered = computed(() => {
    const m = this.actions.menu();
    return m ? this.actions.offered(m.targets) : [];
  });

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
