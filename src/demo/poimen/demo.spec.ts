// @vitest-environment jsdom
import { describe, expect, it } from "vitest";
import { parseSettings } from "../../app/core/settings";
import { parseMachineStatus } from "../../app/core/machines";
import { parseStatus } from "../../app/deliver/deliver-status";
import { toSnapshot, type WireSnapshot } from "../../app/core/snapshot";
import { NOW, RUN } from "./data";
import { createPoimenDemo } from "./demo";

describe("the poimen demo", () => {
  const demo = createPoimenDemo();

  it("answers the start commands in the shapes the app parses", async () => {
    const settings = parseSettings(await demo.invoke("get_settings"));
    expect(settings.machines.map((m) => m.name)).toEqual(["build-box", "gpu-lab"]);
    const snapshot = toSnapshot(await demo.invoke<WireSnapshot>("snapshot"));
    expect(snapshot.spaces.length).toBeGreaterThan(2);
    // Every terminal sits in a tab of its space.
    for (const t of snapshot.terminals) {
      expect(snapshot.tabs.find((tab) => tab.tab_id === t.tab_id)?.space_id).toBe(t.space_id);
    }
    const machines = (await demo.invoke<unknown[]>("list_machines")).map(parseMachineStatus);
    expect(machines.map((m) => m?.state)).toEqual(["connected", "offline"]);
  });

  it("has a run the Runs view accepts, updated shortly before the fixed clock", () => {
    const run = parseStatus(RUN);
    expect(typeof run).toBe("object");
    if (typeof run === "string") return;
    expect(Date.parse(run.updated)).toBeLessThanOrEqual(NOW);
    expect(NOW - Date.parse(run.updated)).toBeLessThan(5 * 60_000);
    expect(run.needsYou).toBe(true);
  });

  it("fails a command it has no answer for", async () => {
    await expect(demo.invoke("not_a_command")).rejects.toThrow("not_a_command");
  });
});
