// @vitest-environment jsdom
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { beforeAll, describe, expect, it, vi } from "vitest";
import { SpacesStore } from "../core/spaces-store";
import type { Snapshot } from "../core/snapshot";
import { Deliver } from "./deliver";
import type { DeliverCard } from "./deliver-status";

vi.mock("@tauri-apps/api/core", async () => ({
  invoke: (await import("../core/tauri.mock")).tauri.invoke,
}));
vi.mock("@tauri-apps/api/event", async () => ({
  listen: (await import("../core/tauri.mock")).tauri.listen,
}));
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));

beforeAll(() => {
  try {
    getTestBed().initTestEnvironment(BrowserTestingModule, platformBrowserTesting());
  } catch {
    // already initialized by the shared setup
  }
});

const card = (host: Record<string, string>): DeliverCard =>
  ({ root: "/r", run: "x", state: { kind: "ok", status: { host } } }) as unknown as DeliverCard;

describe("the starter of a run (#982)", () => {
  it("names the agent that started the terminal of the run, with its space", () => {
    const deliver = TestBed.inject(Deliver);
    vi.spyOn(TestBed.inject(SpacesStore), "all").mockReturnValue({
      spaces: [{ space_id: "s1", label: "api", number: 1, agent_status: "idle" }],
      tabs: [],
      terminals: [
        { terminal_id: "lead", space_id: "s1", label: "lead", agent: "claude" },
        { terminal_id: "helper", space_id: "s1", started_by: "lead" },
      ],
    } as unknown as Snapshot);
    expect(deliver.starterOf(card({ "poimen.terminal": "helper" }))).toBe("lead (api)");
    expect(deliver.starterOf(card({ "poimen.terminal": "lead" }))).toBeUndefined();
    expect(deliver.starterOf(card({}))).toBeUndefined();
    expect(
      deliver.starterOf({ root: "/r", run: "", state: { kind: "invalid", error: "x" } }),
    ).toBeUndefined();
  });
});
