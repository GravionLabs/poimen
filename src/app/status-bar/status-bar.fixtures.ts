import type { Fixture } from "../../fixtures/fixture";
import { StatusBar } from "./status-bar";

const VIEWPORT = { width: 1000, height: 120 };

export const STATUS_BAR_FIXTURES: Fixture[] = [
  {
    id: "status-bar/connected",
    title: "Connected with an active terminal",
    component: StatusBar,
    inputs: {
      connection: { state: "connected" },
      paneId: "p1",
      mode: "control",
      size: { cols: 120, rows: 32 },
      appVersion: "0.29.0",
      info: { version: "0.29.0", protocol: 1, supported: 1 },
      waiting: 2,
    },
    viewport: VIEWPORT,
  },
  {
    id: "status-bar/unreachable",
    title: "Server unreachable, read-only terminal",
    component: StatusBar,
    inputs: {
      connection: { state: "unreachable", reason: "connection refused" },
      paneId: "p1",
      mode: "observe",
      blocked: "poimen console",
      notice: "Pane layout of the new space was not applied",
    },
    viewport: VIEWPORT,
  },
];
