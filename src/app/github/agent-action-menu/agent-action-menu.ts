import { ChangeDetectionStrategy, Component, computed, inject } from "@angular/core";
import { AgentActions, type ActionTarget } from "../agent-actions";
import { IssueTasks } from "../issue-task";
import { DeliverLauncher } from "../../deliver/deliver-launcher";
import { ContextMenu } from "../../ui/context-menu/context-menu";
import type { MenuGroup, MenuPick } from "../../ui/context-menu/menu-model";

const START_TASK = "start-task";
const DELIVER = "deliver";

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
  private readonly deliver = inject(DeliverLauncher);
  /** The open issue of the menu: it gets "Start task…" or "Open space" above the agent actions. */
  protected readonly openIssue = computed<ActionTarget | null>(() => {
    const t = this.actions.menu()?.targets ?? [];
    return t.length === 1 && t[0].kind === "issue" && t[0].state === "open" ? t[0] : null;
  });
  /** The open issues of one repository the menu is for (marked rows, else the row): "Deliver…". */
  protected readonly deliverTarget = computed<{
    root: string;
    numbers: number[];
  } | null>(() => {
    const targets = this.actions.menu()?.targets ?? [];
    const [first] = targets;
    if (!first || !targets.every((t) => t.kind === "issue" && t.state === "open")) return null;
    if (!targets.every((t) => t.root === first.root)) return null;
    return { root: first.root, numbers: targets.map((t) => t.number) };
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
          ...(this.deliverTarget()
            ? [{ id: DELIVER, label: "Deliver…", testId: "menu-deliver" }]
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
    if (pick.id === DELIVER) {
      const target = this.deliverTarget();
      this.actions.closeMenu();
      if (target) this.deliver.openIssues(target.root, target.numbers);
      return;
    }
    const o = this.offered().find((x) => x.action.id === pick.id);
    if (o) void this.actions.request(o.action, m.targets, pick.shiftKey);
  }

  protected close(): void {
    this.actions.closeMenu();
  }
}
