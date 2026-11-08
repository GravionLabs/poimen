import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import {
  STALE_MS,
  ageText,
  barSummary,
  cardsOf,
  fromLegacy,
  needsAttention,
  parseStatus,
  percent,
  progressText,
  runState,
  stateText,
  transitions,
  type DeliverCard,
  type DeliverStatus,
} from "./deliver-status";

const json = (path: string): Record<string, unknown> =>
  JSON.parse(readFileSync(`src/app/deliver/fixtures/${path}.json`, "utf8"));
const must = (parsed: DeliverStatus | string): DeliverStatus => {
  if (typeof parsed === "string") throw new Error(parsed);
  return parsed;
};
/** A run state as the backend folds it from agent-events (`agent_events.rs`). */
const run = (name: string): DeliverStatus => must(parseStatus(json(`events-${name}`)));
const legacy = (name: string): DeliverStatus => must(fromLegacy(json(`status-${name}`)));

const MILESTONE = run("milestone");
const SESSION = run("session");
const FAILED = run("failed");
const ABANDONED = run("abandoned");
/** Two minutes after the last update of the milestone run. */
const NOW = Date.parse(MILESTONE.updated) + 2 * 60_000;

const ok = (status: DeliverStatus): DeliverCard => ({
  root: "/r",
  run: status.run,
  state: { kind: "ok", status },
});

describe("parseStatus of a core run", () => {
  it("reads subject, progress, open tasks with step, finished tasks, attentions and links", () => {
    expect(MILESTONE).toMatchObject({
      status: "running",
      subject: { kind: "github.milestone", title: "M4 Deliver loop" },
      progress: { done: 3, total: 9, label: "issues" },
      needsYou: true,
      tone: "warning",
      harness: "claude",
      host: { "poimen.terminal": "t7" },
    });
    expect(MILESTONE.tasks[0]).toMatchObject({
      task: "t12",
      step: { name: "implement", n: 2, total: 3, text: "tests green" },
      progress: { done: 1, total: 3, label: "steps" },
    });
    expect(MILESTONE.tasks[1]).toMatchObject({ parent: "t12", subject: { kind: "custom.thing" } });
    expect(MILESTONE.finished.map((t) => [t.task, t.outcome])).toEqual([
      ["t11", "succeeded"],
      ["t10", "failed"],
    ]);
    expect(MILESTONE.finished[0]!.links[0]).toMatchObject({ rel: "pull-request" });
    expect(MILESTONE.attentions.filter((a) => a.needsYou).map((a) => a.id)).toEqual(["a1"]);
    expect(MILESTONE.links.map((l) => l.rel)).toEqual(["pull-request", "log"]);
    expect(MILESTONE.events.map((e) => e.type)).toEqual([
      "run.started",
      "step.finished",
      "attention",
    ]);
    expect(MILESTONE.events[1]).toMatchObject({ task: "t11", step: "pr", tone: "success" });
  });

  it("shows a run from the core alone: no subject, no profile, no deliver fields", () => {
    expect(SESSION.subject).toBeNull();
    expect(SESSION.progress).toBeNull();
    expect(SESSION.tasks).toHaveLength(1);
    expect(SESSION.tasks[0]!.step).toEqual({ name: "review", n: null, total: null, text: null });
    expect(SESSION.harness).toBeNull();
  });

  it("ignores unknown fields, profile objects, unknown kinds and a malformed task", () => {
    const raw = json("events-milestone");
    const extended = {
      ...raw,
      future: 1,
      deliver: { anything: true },
      tasks: [...(raw["tasks"] as unknown[]), { task: "broken" }, "not a task"],
    };
    const parsed = must(parseStatus(extended));
    expect(parsed.tasks.map((t) => t.task)).toEqual(["t12", "t13"]);
    expect(parsed.run).toBe(MILESTONE.run);
  });

  it.each([
    ["not an object", 42],
    ["a missing run", { ...json("events-session"), run: undefined }],
    ["a status outside the enum", { ...json("events-session"), status: "paused" }],
    ["an updated that is no date", { ...json("events-session"), updated: "yesterday" }],
  ])("reports %s instead of throwing", (_name, value) => {
    expect(typeof parseStatus(value)).toBe("string");
  });

  it("falls back to neutral for a tone it does not know", () => {
    expect(must(parseStatus({ ...json("events-session"), tone: "purple" })).tone).toBe("neutral");
  });
});

describe("cardsOf", () => {
  it("has no card for a repository without runs", () => {
    expect(cardsOf("/r", { kind: "missing" })).toEqual([]);
  });

  it("makes one card per run, whoever wrote it", () => {
    const cards = cardsOf("/r", {
      kind: "events",
      runs: [json("events-milestone"), json("events-session")],
    });
    expect(cards.map((c) => c.run)).toEqual([MILESTONE.run, SESSION.run]);
    expect(cards.every((c) => c.state.kind === "ok")).toBe(true);
  });

  it("adds a hint for events of an unknown version next to the runs it can read", () => {
    const cards = cardsOf("/r", {
      kind: "events",
      runs: [json("events-session")],
      unknownVersion: 2,
    });
    expect(cards).toHaveLength(2);
    expect(cards[1]!.state).toEqual({ kind: "events-version", version: "2" });
  });

  it("reports a run it cannot read as an invalid card", () => {
    const [card] = cardsOf("/r", { kind: "events", runs: [{ run: "x" }] });
    expect(card!.state.kind).toBe("invalid");
  });

  it("turns another schema version of the legacy file into a hint and a broken one into an error", () => {
    expect(cardsOf("/r", { kind: "unsupported", schemaVersion: 2 })[0]!.state).toEqual({
      kind: "unsupported",
      version: "2",
    });
    expect(cardsOf("/r", { kind: "unsupported" })[0]!.state).toEqual({
      kind: "unsupported",
      version: "none",
    });
    expect(cardsOf("/r", { kind: "invalid", error: "boom" })[0]!.state).toEqual({
      kind: "invalid",
      error: "boom",
    });
  });

  it("maps a legacy status file onto the core shape", () => {
    const [card] = cardsOf("/r", { kind: "ok", status: json("status-running") });
    expect(card!.state.kind).toBe("ok");
  });
});

describe("legacy status.json mapped onto the core shape", () => {
  it("shows the milestone as subject and progress, the current PBI as an open task", () => {
    const s = legacy("running");
    expect(s).toMatchObject({
      status: "running",
      subject: { kind: "github.milestone", title: expect.stringContaining("M4") },
      progress: { done: 3, total: 9 },
    });
    expect(s.tasks[0]).toMatchObject({
      task: "#332",
      step: { name: "implement: tests green", n: 3, total: 7 },
    });
    expect(s.finished.some((t) => t.task === "#325")).toBe(true);
  });

  it("keeps blocked and paused items as attentions and asks for the user once", () => {
    const s = legacy("blocked");
    expect(s.tone).toBe("danger");
    expect(s.tasks).toEqual([]);
    expect(s.attentions.some((a) => a.reason.includes("CI red on the PR"))).toBe(true);
    expect(s.attentions.filter((a) => a.needsYou)).toHaveLength(1);
    expect(s.needsYou).toBe(true);
  });

  it("maps a done run to a succeeded finish with the release PR as a link", () => {
    const s = legacy("done");
    expect(s).toMatchObject({ status: "finished", outcome: "succeeded", tone: "success" });
    expect(s.links[0]).toMatchObject({ rel: "pull-request", title: "Release" });
  });

  it("stays running between two iterations (#537) without a release ask", () => {
    const raw: Record<string, unknown> = {
      ...json("status-done"),
      closed: 2,
      total: 9,
      blocked: [],
      paused: [],
      release: {},
      needsYou: [{ number: 1, url: "u", reason: "Release PR is ready" }],
    };
    delete raw["source"];
    const s = must(fromLegacy(raw));
    expect(s.status).toBe("running");
    expect(s.attentions).toEqual([]);
    expect(s.links).toEqual([]);
  });

  it("drops a pull request URL that is the issue URL (#538)", () => {
    const issue = "https://github.com/GravionLabs/poimen/issues/334";
    const s = must(
      fromLegacy({
        ...json("status-running"),
        delivered: [{ pbi: 334, url: issue, prUrl: issue }],
      }),
    );
    expect(s.finished[0]!.links).toEqual([]);
  });

  it.each([
    ["not an object", 7],
    ["a result outside the enum", { ...json("status-running"), result: "paused" }],
    ["an updatedAt that is no date", { ...json("status-running"), updatedAt: "yesterday" }],
  ])("reports %s instead of throwing", (_name, value) => {
    expect(typeof fromLegacy(value)).toBe("string");
  });
});

describe("run state, age and progress", () => {
  it("is stale only when running for over an hour without an update, or abandoned", () => {
    expect(runState(MILESTONE, NOW)).toBe("running");
    expect(runState(MILESTONE, Date.parse(MILESTONE.updated) + STALE_MS + 1)).toBe("stale");
    expect(runState(ABANDONED, Date.parse(ABANDONED.updated))).toBe("stale");
  });

  it("follows the outcome of a finished run", () => {
    expect(runState(FAILED, NOW)).toBe("failed");
    expect(runState({ ...FAILED, outcome: "cancelled" }, NOW)).toBe("cancelled");
    expect(runState({ ...FAILED, outcome: "succeeded" }, NOW)).toBe("succeeded");
  });

  it("words the age", () => {
    const now = Date.parse("2026-11-08T12:00:00Z");
    expect(ageText("2026-11-08T12:00:00Z", now)).toBe("just now");
    expect(ageText("2026-11-08T11:48:00Z", now)).toBe("12 min ago");
    expect(ageText("2026-11-08T09:00:00Z", now)).toBe("3 h ago");
    expect(ageText("2026-11-06T12:00:00Z", now)).toBe("2 d ago");
    expect(ageText("nope", now)).toBe("");
  });

  it("says running, stale and the end of a run in words", () => {
    expect(stateText(MILESTONE, NOW)).toBe("running, updated 2 min ago");
    expect(stateText(MILESTONE, Date.parse(MILESTONE.updated) + 61 * 60_000)).toContain("stale");
    expect(stateText(FAILED, Date.parse(FAILED.updated) + 60_000)).toBe(
      "failed 1 min ago: CI stayed red",
    );
    expect(stateText({ ...FAILED, outcome: "succeeded", reason: null }, NOW)).toContain("finished");
  });

  it("computes the progress, also for an empty one", () => {
    expect(percent({ done: 3, total: 9 })).toBe(33);
    expect(percent({ done: 0, total: 0 })).toBe(0);
    expect(percent({ done: 12, total: 9 })).toBe(100);
    expect(progressText({ done: 3, total: 9, label: "issues" })).toBe("3/9 issues");
    expect(progressText({ done: 1, total: 2, label: null })).toBe("1/2");
  });

  it("asks for a look on a warning or danger tone", () => {
    expect(needsAttention(MILESTONE)).toBe(true);
    expect(needsAttention(SESSION)).toBe(false);
    expect(needsAttention(FAILED)).toBe(true);
  });
});

describe("barSummary", () => {
  it("shows one running run as its subject and progress", () => {
    const bar = barSummary([ok(MILESTONE)], NOW);
    expect(bar?.text).toBe("⚠ M4 Deliver loop 3/9");
    expect(bar?.title).toContain("Second implement");
    expect(bar?.title).toContain("(needs you)");
  });

  it("names a run without subject by its id and counts several runs", () => {
    expect(barSummary([ok(SESSION)], Date.parse(SESSION.updated))?.text).toBe(`▶ ${SESSION.run}`);
    expect(barSummary([ok(MILESTONE), ok(SESSION)], Date.parse(SESSION.updated))?.text).toBe(
      "⚠ 2 runs",
    );
  });

  it("keeps a fresh run that ended with a warning tone, hides finished, stale and unreadable ones", () => {
    const fresh = Date.parse(FAILED.updated) + 60_000;
    expect(barSummary([ok(FAILED)], fresh)?.warning).toBe(true);
    expect(barSummary([ok({ ...FAILED, tone: "success" })], fresh)).toBeNull();
    expect(barSummary([ok(FAILED)], Date.parse(FAILED.updated) + STALE_MS + 1)).toBeNull();
    expect(barSummary([ok(ABANDONED)], Date.parse(ABANDONED.updated))).toBeNull();
    expect(
      barSummary([{ root: "/r", run: "", state: { kind: "invalid", error: "x" } }], NOW),
    ).toBeNull();
  });
});

describe("transitions", () => {
  it("has nothing to say without a previous state or for another run", () => {
    expect(transitions(undefined, MILESTONE)).toEqual([]);
    expect(transitions(SESSION, MILESTONE)).toEqual([]);
  });

  it("reports a newly failed task and a new decision once", () => {
    const before: DeliverStatus = {
      ...MILESTONE,
      finished: MILESTONE.finished.filter((t) => t.outcome !== "failed"),
      attentions: [],
    };
    const events = transitions(before, MILESTONE);
    expect(events.map((e) => e.kind)).toEqual(["blocked", "needs-you"]);
    expect(events[0]!.body).toBe("Zero failed: review rejected");
    expect(events[1]!.body).toContain("decide whether the field names are final");
    expect(events[0]!.title).toBe("Run M4 Deliver loop");
    expect(transitions(MILESTONE, MILESTONE)).toEqual([]);
  });

  it("reports the end of a run once: done, cancelled, or failed without a failed task", () => {
    const running: DeliverStatus = { ...SESSION };
    const done: DeliverStatus = { ...SESSION, status: "finished", outcome: "succeeded" };
    expect(transitions(running, done)).toEqual([
      { kind: "ended", title: `Run ${SESSION.run}`, body: "The run is done" },
    ]);
    expect(transitions(done, done)).toEqual([]);
    expect(transitions(running, { ...done, outcome: "cancelled" })[0]!.body).toBe(
      "The run ended: cancelled",
    );
    const failed = transitions(running, { ...done, outcome: "failed", reason: "boom" });
    expect(failed).toEqual([
      { kind: "blocked", title: `Run ${SESSION.run}`, body: "The run failed: boom" },
    ]);
  });
});
