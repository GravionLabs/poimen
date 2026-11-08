import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import {
  STALE_MS,
  ageText,
  barSummary,
  cardsOf,
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
/** A run state as the backend folds it from progress events (`progress_events.rs`). */
const eventsFixture = (name: string): Record<string, unknown> =>
  JSON.parse(readFileSync(`src/app/deliver/fixtures/events-${name}.json`, "utf8"));
const runOf = (name: string): DeliverStatus => {
  const parsed = parseStatus(eventsFixture(name));
  if (typeof parsed === "string") throw new Error(parsed);
  return parsed;
};
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

describe("a status file written between iterations (#537)", () => {
  const between = {
    ...fixture("done"),
    closed: 7,
    total: 23,
    release: { reviewRound: null, prUrl: "https://github.com/GravionLabs/skills/pull/530" },
  };

  it("stays running with current null and open items, without a release ask", () => {
    const parsed = parseStatus(between);
    if (typeof parsed === "string") throw new Error(parsed);
    expect(parsed.result).toBe("running");
    expect(runState(parsed, Date.parse(parsed.updatedAt) + 60_000)).toBe("running");
    expect(parsed.needsYou).toEqual([]);
    expect(parsed.tone).not.toBe("warning");
    expect(parsed.release.prUrl).toBeNull();
  });

  it("keeps done when every item is closed", () => {
    expect(DONE.result).toBe("done");
    expect(DONE.needsYou).toHaveLength(1);
  });
});

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

describe("cardsOf", () => {
  it("has no card for a repository without runs", () => {
    expect(cardsOf("/r", { kind: "missing" })).toEqual([]);
  });

  it("turns another schema version into a hint, not an error", () => {
    expect(cardsOf("/r", { kind: "unsupported", schemaVersion: 2 })[0].state).toEqual({
      kind: "unsupported",
      version: "2",
    });
    expect(cardsOf("/r", { kind: "unsupported" })[0].state).toEqual({
      kind: "unsupported",
      version: "none",
    });
  });

  it("keeps a broken file as an invalid card", () => {
    expect(cardsOf("/r", { kind: "invalid", error: "EOF" })[0].state).toEqual({
      kind: "invalid",
      error: "EOF",
    });
    expect(cardsOf("/r", { kind: "ok", status: { schemaVersion: 1 } })[0].state.kind).toBe(
      "invalid",
    );
  });

  it("parses an ok read", () => {
    expect(cardsOf("/r", { kind: "ok", status: fixture("done") })).toMatchObject([
      { root: "/r", run: DONE.run, state: { kind: "ok", status: { result: "done" } } },
    ]);
  });

  it("makes one card per run of the events, loops and sessions alike", () => {
    const cards = cardsOf("/r", {
      kind: "events",
      runs: [eventsFixture("session"), eventsFixture("blocked")],
    });
    expect(
      cards.map((c) => (c.state.kind === "ok" ? c.state.status.source : c.state.kind)),
    ).toEqual(["session", "loop"]);
    expect(cards[0].run).not.toBe(cards[1].run);
  });

  it("adds a hint for events of an unknown version next to the runs it can read", () => {
    const cards = cardsOf("/r", {
      kind: "events",
      runs: [eventsFixture("blocked")],
      unknownVersion: 2,
    });
    expect(cards).toHaveLength(2);
    expect(cards[1].state).toEqual({ kind: "events-version", version: "2" });
    expect(cardsOf("/r", { kind: "events", runs: [], unknownVersion: 3 })).toHaveLength(1);
  });

  it("reports a run state it cannot read as an invalid card", () => {
    const cards = cardsOf("/r", { kind: "events", runs: [{ run: "x" }] });
    expect(cards[0].state.kind).toBe("invalid");
  });
});

describe("tone", () => {
  it("is the tone of the events, not derived from the text", () => {
    expect(runOf("blocked").tone).toBe("danger");
    expect(runOf("rc-test").tone).toBe("success");
    expect(runOf("session").tone).toBe("neutral");
    expect(needsAttention(runOf("blocked"))).toBe(true);
    expect(needsAttention(runOf("rc-test"))).toBe(false);
  });

  it("is derived for a status file, which has none", () => {
    expect(RUNNING.tone).toBe("neutral");
    expect(BLOCKED.tone).toBe("danger");
    expect(DONE.tone).toBe("success");
    expect(status("running").source).toBeNull();
  });

  it("falls back to neutral-by-state for a tone it does not know", () => {
    const parsed = parseStatus({ ...eventsFixture("blocked"), tone: "loud" });
    expect(typeof parsed === "object" && parsed.tone).toBe("danger");
  });
});

describe("runs from events", () => {
  it("shows the release candidate and the open RC test beside the release PR as needs you", () => {
    const rc = runOf("rc-test");
    expect(rc.release).toMatchObject({
      rcVersion: "3.2.0-rc.1",
      rcUrl: expect.stringContaining("v3.2.0-rc.1"),
      prUrl: expect.stringContaining("/pull/391"),
      testIssue: { number: 400 },
    });
    expect(rc.needsYou).toEqual([expect.objectContaining({ number: 400, reason: "RC test" })]);
    expect(rc.blocked).toEqual([]);
    expect(runState(rc, Date.parse(rc.updatedAt))).toBe("done");
  });

  it("reads a session without milestone", () => {
    const session = runOf("session");
    expect(session.milestone).toBeNull();
    expect(
      barSummary(
        [{ root: "/r", run: session.run, state: { kind: "ok", status: session } }],
        Date.parse(session.updatedAt) + 60_000,
      )?.text,
    ).toBe("▶ session");
  });

  it("shows an abandoned run as stale, also in words", () => {
    const abandoned = runOf("abandoned");
    const now = Date.parse(abandoned.updatedAt) + 60_000;
    expect(runState(abandoned, now)).toBe("stale");
    expect(stateText(abandoned, now)).toContain("stale");
    expect(
      barSummary(
        [{ root: "/r", run: abandoned.run, state: { kind: "ok", status: abandoned } }],
        now,
      ),
    ).toBeNull();
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

  it("asks for a look on a warning or danger tone", () => {
    expect(needsAttention(RUNNING)).toBe(false);
    expect(needsAttention(BLOCKED)).toBe(true);
    expect(needsAttention({ ...RUNNING, tone: "warning" })).toBe(true);
  });
});

describe("barSummary", () => {
  const card = (s: DeliverStatus, root = "/r"): DeliverCard => ({
    root,
    run: s.run,
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

  it("turns into a warning by the tone of the run, not by its items", () => {
    const needs = { ...RUNNING, needsYou: BLOCKED.needsYou, tone: "warning" as const };
    const quiet = { ...RUNNING, needsYou: BLOCKED.needsYou };
    expect(barSummary([card(quiet)], NOW)?.warning).toBe(false);
    expect(barSummary([card(needs)], NOW)).toMatchObject({ text: "⚠ M4 3/9", warning: true });
    const blocked = { ...BLOCKED, updatedAt: RUNNING.updatedAt };
    expect(barSummary([card(blocked)], NOW)?.warning).toBe(true);
  });

  it("hides finished, stale and unreadable runs", () => {
    expect(barSummary([card(DONE)], Date.parse(DONE.updatedAt))).toBeNull();
    expect(barSummary([card(RUNNING)], NOW + 2 * STALE_MS)).toBeNull();
    expect(
      barSummary([{ root: "/r", run: "", state: { kind: "unsupported", version: "2" } }], NOW),
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

  it("names a session without a milestone", () => {
    const session = runOf("session");
    const next = { ...session, needsYou: BLOCKED.needsYou };
    expect(transitions(session, next)[0].title).toBe("Deliver session");
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

describe("delivered entries (#538)", () => {
  const withDelivered = (delivered: unknown[]) => {
    const parsed = parseStatus({ ...fixture("running"), delivered });
    if (typeof parsed === "string") throw new Error(parsed);
    return parsed.delivered;
  };

  it("drops a prUrl that is the issue URL, keeps a pull request URL", () => {
    const issue = "https://github.com/GravionLabs/poimen/issues/334";
    const pull = "https://github.com/GravionLabs/poimen/pull/335";
    expect(
      withDelivered([
        { pbi: 334, url: issue, prUrl: issue },
        { pbi: 335, url: "u", prUrl: pull },
        { pbi: 336, url: "u" },
      ]).map((d) => d.prUrl),
    ).toEqual([null, pull, null]);
  });
});
