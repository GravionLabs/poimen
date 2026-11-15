import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import {
  STALE_MS,
  ageText,
  attentionTone,
  barSummary,
  cardKey,
  cardsOf,
  fromLegacy,
  foldFinished,
  type FinishedTask,
  type Outcome,
  groupKey,
  groupRuns,
  isAlert,
  leftOpen,
  needsAttention,
  needsYouItems,
  outcomeText,
  parseStatus,
  percent,
  progressText,
  runSections,
  runState,
  runningRun,
  spanText,
  startedAt,
  stateText,
  timelineLine,
  type Attention,
  type DeliverCard,
  type DeliverStatus,
  type RunGroup,
  type Tone,
  withMachines,
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

describe("remote runs (#692)", () => {
  const read = { kind: "events" as const, runs: [json("events-session")] };

  it("puts the machine on each card and qualifies the terminal of its run", () => {
    const [local] = cardsOf("/r", read);
    const [remote] = cardsOf("/r", read, "box");
    expect(local!.machine).toBeUndefined();
    expect(remote!.machine).toBe("box");
    const host = (c: DeliverCard) => (c.state as { status: DeliverStatus }).status.host;
    expect(host(local!)["poimen.terminal"]).toBe("t9");
    expect(host(remote!)["poimen.terminal"]).toBe("box::t9");
    // The same run id on this Mac and on two machines: three keys.
    const keys = new Set([local!, remote!, cardsOf("/r", read, "gpu")[0]!].map(cardKey));
    expect(keys.size).toBe(3);
    expect(cardKey(local!)).toBe(`/r\n${local!.run}`);
  });

  it("marks a hint card of a machine with the machine too", () => {
    const [hint] = cardsOf("/r", { kind: "events", runs: [], unknownVersion: 2 }, "box");
    expect(hint).toMatchObject({ machine: "box", state: { kind: "events-version" } });
  });

  it("shows remote cards of configured machines only, offline while not connected", () => {
    const cards = [
      ...cardsOf("/r", read),
      ...cardsOf("/r", read, "box"),
      ...cardsOf("/r", read, "gone"),
    ];
    const shown = withMachines(cards, new Map([["box", "offline"]]));
    expect(shown.map((c) => [c.machine, c.offline])).toEqual([
      [undefined, undefined],
      ["box", true],
    ]);
    const back = withMachines(shown, new Map([["box", "connected"]]));
    expect(back[1]!.offline).toBe(false);
    // The last known state stays.
    expect(back[1]!.state).toEqual(shown[1]!.state);
  });

  it("names the machine of a remote run in the status bar", () => {
    const [remote] = cardsOf("/r", { kind: "events", runs: [json("events-milestone")] }, "box");
    const now = Date.parse(MILESTONE.updated) + 60_000;
    expect(barSummary([remote!], now)!.text).toMatch(/^. box: /);
  });
});

describe("Runs view: groups by subject (#584)", () => {
  /** Four runs of M014.1: failed, failed with a merging task, abandoned, failed (newest). */
  const FOUR = JSON.parse(
    readFileSync("src/app/deliver/fixtures/runs-milestone.json", "utf8"),
  ) as Record<string, unknown>[];
  const runIds = FOUR.map((r) => r["run"] as string);
  /** Shortly after the last event of the running M4 run. */
  const AT = Date.parse("2026-11-08T12:00:00Z");
  const cards = (root: string, runs: unknown[], machine?: string) =>
    cardsOf(root, { kind: "events", runs }, machine);
  const m014 = () => groupRuns(cards("/r", FOUR))[0]!;
  const indexOf = (group: RunGroup, run: string) =>
    group.runs.findIndex((r) => r.status.run === run);

  it("puts every run of one subject of one repository in one group, newest first", () => {
    const groups = groupRuns([
      ...cards("/r", [FOUR[1], json("events-milestone"), FOUR[3], FOUR[0], FOUR[2]]),
      ...cards("/r", [json("events-session")]),
    ]);
    expect(groups).toHaveLength(3);
    const [group] = groups;
    expect(group!.subject).toMatchObject({ kind: "github.milestone", id: "GravionLabs/demo#14" });
    expect(group!.runs.map((r) => r.status.run)).toEqual([
      "20261108-080000-loop",
      "20261107-120000-loop",
      "20261107-100000-loop",
      "20261107-080000-loop",
    ]);
    expect(group!.updated).toBe(Date.parse("2026-11-08T09:30:00.000Z"));
    expect(groups[1]!.runs).toHaveLength(1);
    expect(groups[1]!.subject?.title).toBe("M4 Deliver loop");
  });

  it("keeps runs without a subject, other repositories and machines in groups of their own", () => {
    const session = json("events-session");
    const other = { ...session, run: "r2" };
    const groups = groupRuns([
      ...cards("/r", [session, other]),
      ...cards("/r", [FOUR[0]]),
      ...cards("/other", [FOUR[1]]),
      ...cards("/r", [FOUR[2]], "box"),
      { root: "/r", run: "", state: { kind: "invalid", error: "boom" } },
    ]);
    expect(groups.map((g) => [g.root, g.machine, g.runs.length])).toEqual([
      ["/r", undefined, 1],
      ["/r", undefined, 1],
      ["/r", undefined, 1],
      ["/other", undefined, 1],
      ["/r", "box", 1],
    ]);
  });

  it("sorts the groups into running now and history, each by last update", () => {
    const groups = groupRuns([
      ...cards("/r", FOUR),
      ...cards("/r", [json("events-milestone"), json("events-failed")]),
    ]);
    const { running, history } = runSections(groups, AT);
    expect(running.map((g) => g.subject?.title)).toEqual(["M4 Deliver loop"]);
    expect(history.map((g) => g.subject?.title)).toEqual(["M014.1 Runs view", "M5 Roadmap"]);
    expect(runningRun(running[0]!, AT)?.status.run).toBe("20261108-100000-loop");
    // An hour later the M4 run is stale: no group runs now.
    expect(runSections(groups, AT + STALE_MS).running).toEqual([]);
  });

  it("shows the tasks of a finished or abandoned run as left open, a merge step as waiting", () => {
    const group = m014();
    const merging = leftOpen(group, indexOf(group, runIds[1]!), AT);
    expect(merging).toHaveLength(1);
    expect(merging[0]).toMatchObject({
      task: { task: "t102" },
      waiting: true,
      pr: { url: "https://github.com/GravionLabs/demo/pull/202" },
    });
    const abandoned = leftOpen(group, indexOf(group, runIds[2]!), AT);
    expect(abandoned.map((t) => [t.task.task, t.waiting, t.pr])).toEqual([["t104", false, null]]);
  });

  it("hides a left-open task that a later run of the group took up", () => {
    const group = m014();
    // #101 was left open by the first run and delivered by the second.
    expect(leftOpen(group, indexOf(group, runIds[0]!), AT)).toEqual([]);
    // A later run that starts #102 again hides the merging entry of the second run.
    const retry = {
      ...FOUR[3],
      run: "20261108-100000-loop",
      status: "running",
      updated: "2026-11-08T11:59:00.000Z",
      tasks: [(FOUR[1]!["tasks"] as unknown[])[0]],
      events: [{ seq: 1, ts: "2026-11-08T10:00:00.000Z", type: "run.started", tone: "neutral" }],
    };
    const later = groupRuns(cards("/r", [...FOUR, retry]))[0]!;
    expect(leftOpen(later, indexOf(later, runIds[1]!), AT)).toEqual([]);
    // The running run's own tasks are open, not left open.
    expect(leftOpen(later, 0, AT)).toEqual([]);
  });

  it("makes one timeline line per run: outcome with text, else reason, PRs and counts", () => {
    const group = m014();
    const line = (run: string) => timelineLine(group, indexOf(group, run), AT);
    expect(line(runIds[1]!)).toMatchObject({
      outcome: "failed: 2 of 4 PBIs delivered",
      skipped: 1,
      leftOpen: 0,
      waiting: 1,
      notes: [],
    });
    expect(line(runIds[1]!).prs.map((l) => l.url)).toEqual([
      "https://github.com/GravionLabs/demo/pull/201",
      "https://github.com/GravionLabs/demo/pull/202",
    ]);
    expect(line(runIds[0]!)).toMatchObject({ outcome: "failed: deliver.partial", leftOpen: 0 });
    expect(line(runIds[2]!)).toMatchObject({ outcome: "abandoned", leftOpen: 1, waiting: 0 });
    expect(line(runIds[3]!).prs.map((l) => l.url)).toEqual([
      "https://github.com/GravionLabs/demo/pull/205",
    ]);
  });

  it("writes start and end of a run in local time, the date when it is not today", () => {
    const local = (d: number, h: number, m = 0) => new Date(2026, 10, d, h, m).toISOString();
    const status = (start: string, end: string, state: "running" | "finished") =>
      must(
        parseStatus({
          run: "r",
          status: state,
          outcome: state === "finished" ? "succeeded" : undefined,
          updated: end,
          events: [{ seq: 1, ts: start, type: "run.started", tone: "neutral" }],
        }),
      );
    const today = Date.parse(local(8, 12));
    expect(spanText(status(local(8, 8), local(8, 9, 30), "finished"), today)).toBe("08:00–09:30");
    expect(spanText(status(local(7, 23), local(8, 1), "finished"), today)).toBe(
      "11-07 23:00–11-08 01:00",
    );
    expect(spanText(status(local(7, 8), local(7, 9), "finished"), today)).toBe("11-07 08:00–09:00");
    expect(spanText(status(local(8, 11), local(8, 11, 58), "running"), today)).toBe("11:00–now");
  });

  it("starts a run at its first event with a date, skipping a malformed ts", () => {
    const local = (d: number, h: number, m = 0) => new Date(2026, 10, d, h, m).toISOString();
    const run = (id: string, events: { ts: string }[], updated: string) =>
      must(
        parseStatus({
          run: id,
          status: "finished",
          outcome: "succeeded",
          updated,
          subject: { kind: "github.milestone", id: "o/r#1", title: "M1" },
          events: events.map((e, i) => ({ seq: i + 1, type: "run.started", ...e })),
        }),
      );
    const today = Date.parse(local(8, 12));
    const broken = run("broken", [{ ts: "yesterday" }, { ts: local(8, 8) }], local(8, 9, 30));
    expect(startedAt(broken)).toBe(local(8, 8));
    expect(spanText(broken, today)).toBe("08:00–09:30");
    // Only malformed ts: the run starts at its last update, nothing throws.
    const none = run("none", [{ ts: "not a date" }], local(8, 10));
    expect(startedAt(none)).toBeNull();
    expect(spanText(none, today)).toBe("10:00–10:00");
    // The sort sees the valid start, never NaN: the newest start (10:00, 08:00, 07:00) first.
    const older = run("older", [{ ts: local(8, 7) }], local(8, 11));
    const order = groupRuns(
      [older, broken, none].map((status) => ({
        root: "/r",
        run: status.run,
        state: { kind: "ok" as const, status },
      })),
    )[0]!.runs.map((r) => r.status.run);
    expect(order).toEqual(["none", "broken", "older"]);
  });

  it("lists needs-you attentions of all runs once per task and reason", () => {
    const again = { ...FOUR[2], run: "20261107-130000-loop", updated: "2026-11-07T13:30:00.000Z" };
    const groups = groupRuns([
      ...cards("/r", [...FOUR, again]),
      ...cards("/r", [json("events-milestone")]),
    ]);
    const items = needsYouItems(groups);
    expect(items.map((i) => outcomeText(i.attention))).toEqual([
      "Decide whether #104 ships in M014.1",
      "decide whether the field names are final",
    ]);
    expect(items[0]!.group.subject?.title).toBe("M014.1 Runs view");
    expect(items[0]!.attention.links[0]!.url).toBe(
      "https://github.com/GravionLabs/demo/issues/104",
    );
  });

  it("keeps success and info attentions out of Attention, for the timeline line instead", () => {
    const raw = json("events-milestone");
    const status = must(
      parseStatus({
        ...raw,
        attentions: [
          ...(raw["attentions"] as unknown[]),
          { id: "a3", task: "t11", reason: "pr.opened", text: "PR opened", tone: "success" },
          { id: "a4", reason: "fyi", tone: "info" },
          { id: "a5", reason: "CI red", tone: "danger" },
        ],
      }),
    );
    expect(status.attentions.filter((a) => isAlert(a, status)).map((a) => a.id)).toEqual([
      "a1",
      "a2",
      "a5",
    ]);
    const group = groupRuns([{ root: "/r", run: status.run, state: { kind: "ok", status } }])[0]!;
    expect(timelineLine(group, 0, AT).notes).toEqual(["PR opened", "fyi"]);
  });

  it("takes the tone of an attention from itself, else its events, else warning", () => {
    const status = must(
      parseStatus({
        ...json("events-milestone"),
        events: [
          { seq: 1, type: "attention", tone: "info", task: "t1" },
          { seq: 2, type: "attention", tone: "danger", task: "t2" },
          { seq: 3, type: "attention", tone: "success", task: "t2" },
        ],
      }),
    );
    const attention = (task: string | null, tone: Tone | null = null): Attention => ({
      id: "x",
      task,
      reason: "r",
      text: null,
      tone,
      needsYou: false,
      links: [],
    });
    expect(attentionTone(attention("t1", "danger"), status)).toBe("danger");
    expect(attentionTone(attention("t1"), status)).toBe("info");
    // Two events of the task with different tones: unknown, so it is listed.
    expect(attentionTone(attention("t2"), status)).toBe("warning");
    expect(attentionTone(attention(null), status)).toBe("warning");
    expect(isAlert(attention("t1"), status)).toBe(false);
  });

  it("reads text of run, finished task and attention, and falls back to reason", () => {
    const failed = must(parseStatus(FOUR[3]));
    expect(failed.text).toBeNull();
    expect(outcomeText(failed)).toBe("deliver.partial");
    expect(failed.finished.find((t) => t.task === "t106")).toMatchObject({
      reason: "ci.red",
      text: "CI stayed red",
    });
    const merging = must(parseStatus(FOUR[1]));
    expect(outcomeText(merging)).toBe("2 of 4 PBIs delivered");
    expect(stateText(merging, AT)).toMatch(/^failed .*: 2 of 4 PBIs delivered$/);
    const abandoned = must(parseStatus(FOUR[2]));
    expect(abandoned.attentions[0]).toMatchObject({
      reason: "decision.open",
      text: "Decide whether #104 ships in M014.1",
      tone: null,
    });
    expect(outcomeText({ text: null, reason: null })).toBeNull();
  });

  it("names a group by repository and subject, a run without one by its run", () => {
    const [a] = cards("/r", [FOUR[0]]);
    const [b] = cards("/r", [FOUR[1]], "box");
    const [c] = cards("/r", [json("events-session")]);
    expect(groupKey(a!)).toBe("/r\ngithub.milestone\nGravionLabs/demo#14");
    expect(groupKey(b!)).toBe("box::/r\ngithub.milestone\nGravionLabs/demo#14");
    expect(groupKey(c!)).toBe(`/r\n\n\n${SESSION.run}`);
  });
});

describe("foldFinished (#782)", () => {
  const t = (task: string, outcome: Outcome, text: string | null) =>
    ({
      task,
      outcome,
      subject: { kind: "github.issue", id: task, title: task, url: null },
      reason: null,
      text,
      links: [],
    }) as FinishedTask;

  it("counts skipped tasks per reason and keeps the others", () => {
    const folds = foldFinished([
      t("a", "succeeded", null),
      t("b", "skipped", "blocked"),
      t("c", "skipped", "blocked"),
      t("d", "skipped", null),
    ]);
    expect(folds.done.map((x) => x.task)).toEqual(["a"]);
    expect(folds.skipped.map((f) => [f.reason, f.tasks.length])).toEqual([
      ["blocked", 2],
      ["no reason", 1],
    ]);
  });
});
