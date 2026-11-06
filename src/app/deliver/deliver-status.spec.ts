import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import {
  STALE_MS,
  ageText,
  barSummary,
  cardOf,
  needsAttention,
  parseStatus,
  percent,
  runState,
  stateText,
  transitions,
  type DeliverCard,
  type DeliverStatus,
} from "./deliver-status";

const fixture = (name: string): Record<string, unknown> =>
  JSON.parse(readFileSync(`src/app/deliver/fixtures/status-${name}.json`, "utf8"));
const status = (name: string): DeliverStatus => {
  const parsed = parseStatus(fixture(name));
  if (typeof parsed === "string") throw new Error(parsed);
  return parsed;
};

const RUNNING = status("running");
const BLOCKED = status("blocked");
const DONE = status("done");
/** Two minutes after the last update of the running fixture. */
const NOW = Date.parse(RUNNING.updatedAt) + 2 * 60_000;

describe("parseStatus", () => {
  it("reads the three fixtures of the schema", () => {
    expect(RUNNING).toMatchObject({
      closed: 3,
      total: 9,
      result: "running",
      current: { number: 332, step: { n: 3, total: 7, text: "implement: tests green" } },
    });
    expect(BLOCKED.current).toBeNull();
    expect(BLOCKED.needsYou).toHaveLength(1);
    expect(DONE.release.prUrl).toMatch(/\/pull\//);
  });

  it("ignores unknown fields at every level", () => {
    const raw = fixture("running");
    const extended = { ...raw, future: 1, milestone: { ...(raw["milestone"] as object), x: 2 } };
    expect(parseStatus(extended)).toMatchObject({ run: RUNNING.run });
  });

  it.each([
    ["not an object", 42],
    ["a missing milestone", { ...fixture("running"), milestone: undefined }],
    ["a result outside the enum", { ...fixture("running"), result: "paused" }],
    ["a negative count", { ...fixture("running"), closed: -1 }],
    [
      "a blocked item without a reason",
      { ...fixture("running"), blocked: [{ number: 1, url: "u" }] },
    ],
    ["an updatedAt that is no date", { ...fixture("running"), updatedAt: "yesterday" }],
  ])("reports %s instead of throwing", (_name, value) => {
    expect(typeof parseStatus(value)).toBe("string");
  });
});

describe("cardOf", () => {
  it("has no card for a repository without a status file", () => {
    expect(cardOf("/r", { kind: "missing" })).toBeNull();
  });

  it("turns another schema version into a hint, not an error", () => {
    expect(cardOf("/r", { kind: "unsupported", schemaVersion: 2 })?.state).toEqual({
      kind: "unsupported",
      version: "2",
    });
    expect(cardOf("/r", { kind: "unsupported" })?.state).toEqual({
      kind: "unsupported",
      version: "none",
    });
  });

  it("keeps a broken file as an invalid card", () => {
    expect(cardOf("/r", { kind: "invalid", error: "EOF" })?.state).toEqual({
      kind: "invalid",
      error: "EOF",
    });
    expect(cardOf("/r", { kind: "ok", status: { schemaVersion: 1 } })?.state.kind).toBe("invalid");
  });

  it("parses an ok read", () => {
    expect(cardOf("/r", { kind: "ok", status: fixture("done") })).toMatchObject({
      root: "/r",
      state: { kind: "ok", status: { result: "done" } },
    });
  });
});

describe("run state, age and progress", () => {
  it("is stale only when running for over an hour without an update", () => {
    const updated = Date.parse(RUNNING.updatedAt);
    expect(runState(RUNNING, updated + STALE_MS)).toBe("running");
    expect(runState(RUNNING, updated + STALE_MS + 1)).toBe("stale");
    expect(runState(DONE, updated + 10 * STALE_MS)).toBe("done");
  });

  it("words the age", () => {
    const t = Date.parse("2026-11-06T10:00:00Z");
    expect(ageText("2026-11-06T10:00:00Z", t + 20_000)).toBe("just now");
    expect(ageText("2026-11-06T10:00:00Z", t + 12 * 60_000)).toBe("12 min ago");
    expect(ageText("2026-11-06T10:00:00Z", t + 3 * 3_600_000)).toBe("3 h ago");
    expect(ageText("2026-11-06T10:00:00Z", t + 50 * 3_600_000)).toBe("2 d ago");
    expect(ageText("nonsense", t)).toBe("");
  });

  it("says finished, running and stale in words", () => {
    const doneAt = Date.parse(DONE.updatedAt);
    expect(stateText(DONE, doneAt + 12 * 60_000)).toBe("finished 12 min ago");
    expect(stateText(RUNNING, NOW)).toBe("running, updated 2 min ago");
    expect(stateText(RUNNING, NOW + 2 * STALE_MS)).toContain("stale");
  });

  it("computes the progress, also for an empty milestone", () => {
    expect(percent({ closed: 3, total: 9 })).toBe(33);
    expect(percent({ closed: 0, total: 0 })).toBe(0);
    expect(percent({ closed: 12, total: 9 })).toBe(100);
  });

  it("asks for the user on blocked, paused and needs-you items", () => {
    expect(needsAttention(RUNNING)).toBe(false);
    expect(needsAttention(BLOCKED)).toBe(true);
    expect(needsAttention({ ...RUNNING, paused: BLOCKED.paused })).toBe(true);
  });
});

describe("barSummary", () => {
  const card = (s: DeliverStatus, root = "/r"): DeliverCard => ({
    root,
    state: { kind: "ok", status: s },
  });

  it("shows one running loop as milestone and progress", () => {
    expect(barSummary([card(RUNNING)], NOW)).toMatchObject({
      text: "▶ M4 3/9",
      warning: false,
    });
  });

  it("counts several running loops", () => {
    expect(barSummary([card(RUNNING, "/a"), card(RUNNING, "/b")], NOW)?.text).toBe("▶ 2 runs");
  });

  it("turns into a warning when a loop is blocked or needs the user", () => {
    const needs = { ...RUNNING, needsYou: BLOCKED.needsYou };
    expect(barSummary([card(needs)], NOW)).toMatchObject({ text: "⚠ M4 3/9", warning: true });
    const blocked = { ...BLOCKED, updatedAt: RUNNING.updatedAt };
    expect(barSummary([card(blocked)], NOW)?.warning).toBe(true);
  });

  it("hides finished, stale and unreadable runs", () => {
    expect(barSummary([card(DONE)], Date.parse(DONE.updatedAt))).toBeNull();
    expect(barSummary([card(RUNNING)], NOW + 2 * STALE_MS)).toBeNull();
    expect(
      barSummary([{ root: "/r", state: { kind: "unsupported", version: "2" } }], NOW),
    ).toBeNull();
    expect(barSummary([], NOW)).toBeNull();
  });
});

describe("transitions", () => {
  it("has nothing to say without a previous state or for another run", () => {
    expect(transitions(undefined, BLOCKED)).toEqual([]);
    expect(transitions({ ...RUNNING, run: "other" }, BLOCKED)).toEqual([]);
  });

  it("reports a newly blocked item once", () => {
    const next = { ...RUNNING, blocked: BLOCKED.blocked };
    expect(transitions(RUNNING, next)).toEqual([
      expect.objectContaining({
        kind: "blocked",
        body: "#335 is blocked: CI red on the PR: check lint:ps failed",
      }),
    ]);
    expect(transitions(next, next)).toEqual([]);
  });

  it("reports a run that ends blocked and a new decision", () => {
    expect(transitions(RUNNING, { ...RUNNING, result: "blocked" }).map((e) => e.kind)).toEqual([
      "blocked",
    ]);
    expect(
      transitions(RUNNING, { ...RUNNING, needsYou: BLOCKED.needsYou }).map((e) => e.kind),
    ).toEqual(["needs-you"]);
  });

  it("reports the release pull request and the end of the run once", () => {
    const next = { ...RUNNING, release: DONE.release, result: "done" as const };
    expect(transitions(RUNNING, next).map((e) => e.kind)).toEqual(["release", "ended"]);
    expect(transitions(next, next)).toEqual([]);
  });
});
