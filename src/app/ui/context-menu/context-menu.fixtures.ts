import type { Fixture } from "../../../fixtures/fixture";
import { ContextMenu } from "./context-menu";
import type { MenuGroup } from "./menu-model";

const GROUPS: MenuGroup[] = [
  {
    id: "main",
    items: [
      { id: "rename", label: "Rename", shortcut: "F2" },
      { id: "pin", label: "Pin space", active: true },
      { id: "copy", label: "Copy path", disabled: "No directory" },
    ],
  },
  {
    id: "danger",
    label: "Danger",
    items: [{ id: "close", label: "Close space", mark: "attention" }],
  },
];

export const CONTEXT_MENU_FIXTURES: Fixture[] = [
  {
    id: "context-menu/groups",
    title: "Menu with a shortcut, a disabled entry and a heading",
    component: ContextMenu,
    inputs: { at: { x: 40, y: 40 }, groups: GROUPS, label: "Space menu" },
    viewport: { width: 400, height: 300 },
  },
  {
    id: "context-menu/empty",
    title: "Menu without entries",
    component: ContextMenu,
    inputs: { at: { x: 40, y: 40 }, groups: [], empty: "Nothing to do", label: "Empty menu" },
    viewport: { width: 400, height: 200 },
  },
];
