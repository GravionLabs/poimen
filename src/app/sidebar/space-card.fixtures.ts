import { signal } from "@angular/core";
import type { HxTreeNode } from "@gravionlabs/helix-ui";
import type { Fixture } from "../../fixtures/fixture";
import { Machines } from "../core/machines";
import { SpacesStore } from "../core/spaces-store";
import { UI_STORAGE } from "../core/ui-state";
import type { SpaceData } from "./space-nodes";
import { SpaceCard } from "./space-card";
import type { Segment } from "./spaces";

const seg = (over: Partial<Segment> & Pick<Segment, "kind" | "text">): Segment => ({
  status: "idle",
  bold: false,
  dim: false,
  sep: false,
  ...over,
});

function node(
  label: string,
  status: "idle" | "blocked" | "working" | "done",
  branch: string,
): HxTreeNode<SpaceData> {
  return {
    key: `workspace:${label}`,
    label,
    expanded: true,
    children: [],
    data: {
      kind: "workspace",
      status,
      member: false,
      lines: [
        [
          seg({ kind: "dot", text: "●", status }),
          seg({ kind: "name", text: label, bold: true }),
          seg({ kind: "state", text: status, status }),
        ],
        [
          seg({ kind: "text", text: branch, token: "branch" }),
          seg({ kind: "text", text: "~/src/poimen", token: "dir", tip: "~/src/poimen" }),
        ],
      ],
      workspace: { space_id: label, label, number: 1, agent_status: status },
    },
  } as unknown as HxTreeNode<SpaceData>;
}

/** The services of the card, without a backend: no machines, no other spaces, nothing stored. */
const PROVIDERS = [
  { provide: Machines, useValue: { machines: signal([]) } },
  {
    provide: SpacesStore,
    useValue: { spaces: signal([]), remoteSpaces: () => [], pinnedSpaces: signal([]) },
  },
  { provide: UI_STORAGE, useValue: { getItem: () => null, setItem: () => undefined } },
];

const VIEWPORT = { width: 360, height: 160 };

export const SPACE_CARD_FIXTURES: Fixture[] = [
  {
    id: "space-card/idle",
    title: "Space card, idle agent",
    component: SpaceCard,
    inputs: { node: node("poimen", "idle", "main"), headerOnly: true },
    providers: PROVIDERS,
    viewport: VIEWPORT,
  },
  {
    id: "space-card/blocked",
    title: "Space card, agent waiting for the user",
    component: SpaceCard,
    inputs: { node: node("api", "blocked", "feat/1020-fixtures"), headerOnly: true },
    providers: PROVIDERS,
    viewport: VIEWPORT,
  },
];
