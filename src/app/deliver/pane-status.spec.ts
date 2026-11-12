import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { STALE_MS, parseStatus, type DeliverCard, type DeliverStatus } from "./deliver-status";
import { effectiveStatus, runsByTerminal } from "./pane-status";

const load = (name: string): DeliverStatus => {
  const parsed = parseStatus(
    JSON.parse(readFileSync(`src/app/deliver/fixtures/events-${name}.json`, "utf8")),
  );
  if (typeof parsed === "string") throw new Error(parsed);
  return parsed;
};

/** Running, linked to terminal t7, needs the user (fixture). */
const MILESTONE = load("milestone");
const NOW = Date.parse(MILESTONE.updated) + 2 * 60_000;
const quiet: DeliverStatus = { ...MILESTONE, needsYou: false };
const finished: DeliverStatus = { ...quiet, status: "finished", outcome: "succeeded" };

describe("effectiveStatus", () => {
  it("keeps a pane working after Stop while its run is running", () => {
    expect(effectiveStatus("done", [quiet], NOW)).toBe("working");
    expect(effectiveStatus("idle", [quiet], NOW)).toBe("working");
  });

  it("is blocked when the run needs the user or the hook is blocked", () => {
    expect(effectiveStatus("done", [MILESTONE], NOW)).toBe("blocked");
    expect(effectiveStatus("blocked", [quiet], NOW)).toBe("blocked");
  });

  it("leaves the hook status when the run is stale, abandoned or finished", () => {
    expect(effectiveStatus("done", [quiet], NOW + STALE_MS)).toBe("done");
    expect(effectiveStatus("done", [load("abandoned")], NOW)).toBe("done");
    expect(effectiveStatus("done", [finished], NOW)).toBe("done");
  });

  it("leaves the hook status without a linked run", () => {
    expect(effectiveStatus("done", [], NOW)).toBe("done");
    expect(effectiveStatus("working", [], NOW)).toBe("working");
  });

  it("lets gone win", () => {
    expect(effectiveStatus("gone", [MILESTONE], NOW)).toBe("gone");
  });
});

describe("runsByTerminal", () => {
  const card = (status: DeliverStatus): DeliverCard => ({
    root: "/r",
    run: status.run,
    state: { kind: "ok", status },
  });

  it("links a run only through host poimen.terminal", () => {
    const unlinked: DeliverStatus = { ...quiet, run: "x", host: { "herdr.pane": "t7" } };
    const map = runsByTerminal([card(MILESTONE), card(unlinked)]);
    expect([...map.keys()]).toEqual(["t7"]);
    expect(map.get("t7")).toEqual([MILESTONE]);
  });

  it("skips cards that did not parse", () => {
    const bad: DeliverCard = {
      root: "/r",
      run: "y",
      state: { kind: "invalid", error: "x" } as never,
    };
    expect(runsByTerminal([bad]).size).toBe(0);
  });
});
