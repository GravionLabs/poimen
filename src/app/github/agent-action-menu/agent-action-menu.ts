import { ChangeDetectionStrategy, Component, computed, inject } from "@angular/core";
import { AgentActions, type ActionTarget } from "../agent-actions";
import { IssueTasks } from "../issue-task";
import { ContextMenu } from "../../ui/context-menu/context-menu";
import type { MenuGroup, MenuPick } from "../../ui/context-menu/menu-model";

const START_TASK = "start-task";

/**
 * The popup of the agent actions of a row (the `⋯` button or the context menu). Shift+click on an
 * entry opens the harness and model dialog. Escape or a click elsewhere closes it.
 */
@Component({
  selector: "app-agent-action-menu",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [ContextMenu],
  templateUrl: "./agent-action-menu.html",
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

  protected readonly groups = computed<MenuGroup[]>(() => {
    const issue = this.openIssue();
    return [
      {
        id: "actions",
        items: [
          ...(issue
            ? [
                {
                  id: START_TASK,
                  label: this.issueTasks.label(issue.root, issue.number),
                  testId: "menu-start-task",
                },
              ]
            : []),
          ...this.offered().map((o) => ({
            id: o.action.id,
            label: o.action.label,
            disabled: o.enabled ? false : (o.reason ?? true),
            testId: "menu-action",
          })),
        ],
      },
    ];
  });

  protected choose(pick: MenuPick): void {
    const m = this.actions.menu();
    if (!m) return;
    if (pick.id === START_TASK) {
      const issue = this.openIssue();
      this.actions.closeMenu();
      if (issue) void this.issueTasks.start(issue.root, issue.number);
      return;
    }
    const o = this.offered().find((x) => x.action.id === pick.id);
    if (o) this.actions.request(o.action, m.targets, pick.shiftKey);
  }

  protected close(): void {
    this.actions.closeMenu();
  }
}
