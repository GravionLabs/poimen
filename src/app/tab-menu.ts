import { canMarkUnread } from "./core/attention";
import { olderHost, supportsFlow, type Terminal } from "./core/snapshot";
import type { ShortcutAction } from "./core/shortcuts";
import type { TabBarMenuItem } from "./ui/helix-candidates/tab-bar/tab-bar";
import { tabKind } from "./core/editor-tabs";

/** What the tab menu needs from the app. */
export interface TabMenuContext {
  shortcut: (action: ShortcutAction) => string;
  pane: (id: string) => Terminal | undefined;
  unread: () => ReadonlySet<string>;
}

/** The entries of the context menu of a group's tabs; `single` disables the splits. */
export function tabMenuItems(single: boolean, ctx: TabMenuContext): TabBarMenuItem[] {
  const isPane = (id: string) => tabKind(id) === "pane";
  return [
    {
      action: "split-right",
      label: "Split right",
      shortcut: ctx.shortcut("splitRight"),
      disabled: single,
    },
    {
      action: "split-down",
      label: "Split down",
      shortcut: ctx.shortcut("splitDown"),
      disabled: single,
    },
    { action: "close", label: "Close" },
    {
      action: "open-flow",
      label: "Open agent flow",
      appliesTo: (id) => isPane(id) && supportsFlow(ctx.pane(id)),
    },
    { action: "pane-split-right", label: "New pane to the right" },
    { action: "pane-split-down", label: "New pane below" },
    { action: "pane-start-agent", label: "Start agent..." },
    { action: "pane-resume-session", label: "Resume session...", appliesTo: isPane },
    {
      action: "pane-mark-unread",
      label: "Mark as unread",
      appliesTo: (id) => isPane(id) && canMarkUnread(ctx.pane(id), ctx.unread()),
    },
    { action: "pane-rename", label: "Rename pane..." },
    { action: "pane-save-layout", label: "Save current tab as layout…", appliesTo: isPane },
    {
      action: "pane-restart",
      label: "Restart terminal on the new host...",
      appliesTo: (id) => isPane(id) && olderHost(ctx.pane(id)) !== null,
    },
    { action: "pane-close", label: "Close pane..." },
  ];
}
