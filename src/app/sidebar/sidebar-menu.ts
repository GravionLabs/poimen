/** A row's context menu: `kind` says what was clicked, `action` what was picked. */
export interface SidebarMenuAction {
  kind: "workspace" | "pane";
  id: string;
  action:
    | "new-terminal"
    | "split-right"
    | "split-down"
    | "start-agent"
    | "resume-session"
    | "rename"
    | "restart"
    | "mark-unread"
    | "close"
    | "close-space"
    | "rename-space"
    | "pin-space"
    | "unpin-space"
    | "start-task"
    | "cleanup-tasks"
    | "show-changes"
    | "open-in-editor";
}

/** One entry of a context menu. */
export interface SidebarMenuItem {
  action: SidebarMenuAction["action"];
  label: string;
  /** Why the entry cannot be used (its machine is not connected, #693); set means disabled. */
  disabled?: string;
}

/** A group of context menu entries; `label` is the heading, `null` for the built-in group. */
export interface SidebarMenuGroup {
  id: string;
  label: string | null;
  items: SidebarMenuItem[];
}

/**
 * Groups of the context menu of a workspace row or a pane row: the built-in actions.
 */
export function sidebarMenuItems(
  kind: SidebarMenuAction["kind"],
  /** The space is in a repository: it offers to start a task. */
  repo = false,
  /** The terminal runs on an older PTY host: it offers a restart on the new one. */
  outdatedHost = false,
  /** The row is on a machine that is not connected: every entry is disabled with this reason. */
  blocked: string | null = null,
  /** The row is an agent that is done or idle and not unread (#725). */
  markable = false,
  /** Whether a local space is pinned (#710); `null` offers no pin entry (pane, remote space). */
  pinned: boolean | null = null,
  /** Why "Resume session..." is disabled (a remote machine, #986); `null` when it is offered. */
  resumeBlocked: string | null = null,
): SidebarMenuGroup[] {
  const resume = {
    action: "resume-session" as const,
    label: "Resume session...",
    ...(resumeBlocked ? { disabled: resumeBlocked } : {}),
  };
  const builtin: SidebarMenuGroup = {
    id: "builtin",
    label: null,
    items:
      kind === "workspace"
        ? [
            { action: "new-terminal", label: "New pane" },
            resume,
            ...(repo
              ? [
                  { action: "show-changes" as const, label: "Show changes" },
                  { action: "start-task" as const, label: "Start task from issue…" },
                  { action: "cleanup-tasks" as const, label: "Clean up merged tasks" },
                ]
              : []),
            ...(pinned === null
              ? []
              : [
                  { action: "open-in-editor" as const, label: "Open in editor" },
                  pinned
                    ? { action: "unpin-space" as const, label: "Unpin space" }
                    : { action: "pin-space" as const, label: "Pin space" },
                ]),
            { action: "rename-space", label: "Rename space..." },
            { action: "close-space", label: "Close space" },
          ]
        : [
            ...(markable ? [{ action: "mark-unread" as const, label: "Mark as unread" }] : []),
            { action: "split-right", label: "Split right" },
            { action: "split-down", label: "Split down" },
            { action: "start-agent", label: "Start agent..." },
            resume,
            { action: "rename", label: "Rename..." },
            ...(outdatedHost
              ? [{ action: "restart" as const, label: "Restart terminal on the new host..." }]
              : []),
            { action: "close", label: "Close pane..." },
          ],
  };
  return [
    blocked
      ? { ...builtin, items: builtin.items.map((item) => ({ ...item, disabled: blocked })) }
      : builtin,
  ];
}
