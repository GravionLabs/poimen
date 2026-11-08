import { describe, expect, it } from "vitest";
import { parseStatus, type DeliverCard } from "../deliver/deliver-status";
import { eventTime, runsOfSpace, selectRun, toneStyle } from "./run-events";

function card(run: string, updated: string, terminal?: string, root = "/repo"): DeliverCard {
  const status = parseStatus({
    v: 1,
    run,
    status: "running",
    tasks: [],
    finished: {},
    attentions: [],
    needsYou: false,
    links: [],
    updated,
    tone: "neutral",
    host: terminal ? { "poimen.terminal": terminal } : {},
    events: [
      { type: "step.finished", tone: "info", task: "t5", step: "implement", ts: updated },
      { type: "bogus-row-without-tone" },
      "not a row",
    ],
  });
  if (typeof status === "string") throw new Error(status);
  return { root, run, state: { kind: "ok", status } };
}

describe("runs of a space", () => {
  const cards = [
    card("old", "2026-11-07T10:00:00.000Z", "t1"),
    card("new", "2026-11-07T11:00:00.000Z", "t2"),
    card("other", "2026-11-07T12:00:00.000Z", undefined, "/elsewhere"),
  ];

  it("lists only the runs of the repository around the directory", () => {
    expect(runsOfSpace(cards, "/repo/sub").map((r) => r.status.run)).toEqual(["old", "new"]);
    expect(runsOfSpace(cards, undefined)).toEqual([]);
    expect(runsOfSpace(cards, "/repository")).toEqual([]);
  });

  it("selects the run whose host terminal is the active terminal", () => {
    const runs = runsOfSpace(cards, "/repo");
    expect(selectRun(runs, "t1", null)?.status.run).toBe("old");
  });

  it("falls back to the latest run when no terminal matches", () => {
    const runs = runsOfSpace(cards, "/repo");
    expect(selectRun(runs, "t9", null)?.status.run).toBe("new");
  });

  it("lets the picker win and ignores a stale pick", () => {
    const runs = runsOfSpace(cards, "/repo");
    expect(selectRun(runs, "t2", runs[0]!.key)?.status.run).toBe("old");
    expect(selectRun(runs, "t2", "gone")?.status.run).toBe("new");
  });

  it("has no run for a space without runs", () => {
    expect(selectRun([], "t1", null)).toBeUndefined();
  });

  it("keeps valid event rows and drops malformed ones", () => {
    const events = runsOfSpace(cards, "/repo")[0]!.status.events;
    expect(events.map((e) => e.type)).toEqual(["step.finished", "bogus-row-without-tone"]);
    expect(events[0]).toMatchObject({ tone: "info", task: "t5", step: "implement" });
    expect(events[1]!.tone).toBe("neutral");
  });
});

describe("tone style", () => {
  it("maps every tone to its class and icon", () => {
    expect(toneStyle("success")).toEqual({ class: "tone-success", icon: "✓", attention: false });
    expect(toneStyle("neutral").class).toBe("tone-neutral");
    expect(toneStyle("info").class).toBe("tone-info");
  });

  it("makes warning and danger rows stand out", () => {
    expect(toneStyle("warning").attention).toBe(true);
    expect(toneStyle("danger").attention).toBe(true);
    expect(toneStyle("danger").class).toBe("tone-danger");
  });
});

describe("event time", () => {
  it("is empty for a missing or invalid timestamp", () => {
    expect(eventTime(null)).toBe("");
    expect(eventTime("nope")).toBe("");
    expect(eventTime("2026-11-07T10:01:02")).toBe("10:01:02");
  });
});
