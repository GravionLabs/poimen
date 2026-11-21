import type { Fixture } from "../../../../fixtures/fixture";
import { TabBar, type TabBarItem } from "./tab-bar";

const TABS: TabBarItem[] = [
  { id: "1", label: "claude", group: "api" },
  { id: "2", label: "tests", group: "api", preview: true },
  { id: "3", label: "review", group: "web", attention: true, mark: "silent 12m" },
  { id: "4", label: "shell", unread: true, note: "controlled elsewhere" },
];

export const TAB_BAR_FIXTURES: Fixture[] = [
  {
    id: "tab-bar/states",
    title: "Active, preview, attention, unread and silent tabs",
    component: TabBar,
    inputs: { tabs: TABS, activeId: "1" },
    viewport: { width: 800, height: 120 },
  },
  {
    id: "tab-bar/overflow",
    title: "More tabs than fit: scroll buttons",
    component: TabBar,
    inputs: {
      tabs: Array.from({ length: 14 }, (_, i) => ({
        id: `t${i}`,
        label: `session ${i + 1}`,
        group: "api",
      })),
      activeId: "t12",
    },
    viewport: { width: 600, height: 120 },
  },
];
