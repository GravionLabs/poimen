import { describe, expect, it } from "vitest";
import {
  clockText,
  parseStatus,
  runTitle,
  type DeliverCard,
  type RunEventRow,
} from "../deliver/deliver-status";
import {
  eventTime,
  foldSkipped,
  groupBySubject,
  mergedItems,
  pickHolds,
  pickRunOf,
  runLabel,
  runsOfSpace,
  selectRun,
  shorten,
  toneStyle,
  type RunChoice,
} from "./run-events";

function card(
  run: string,
  updated: string,
  terminal?: string,
  root = "/repo",
  started = updated,
  state: "running" | "finished" = "running",
): DeliverCard {
  const status = parseStatus({
    v: 1,
    run,
    status: state,
    tasks: [],
    finished: {},
    attentions: [],
    needsYou: false,
    links: [],
    updated,
    tone: "neutral",
    host: terminal ? { "poimen.terminal": terminal } : {},
    events: [
      { type: "run.started", tone: "neutral", ts: started },
      { type: "step.finished", tone: "info", task: "t5", step: "implement", ts: updated },
      { type: "bogus-row-without-tone" },
      "not a row",
    ],
  });
  if (typeof status === "string") throw new Error(status);
  return { root, run, state: { kind: "ok", status } };
}

const NOW = Date.parse("2026-11-07T13:00:00.000Z");

describe("runs of a space", () => {
  const cards = [
    card("old", "2026-11-07T10:00:00.000Z", "t1"),
    card("new", "2026-11-07T11:00:00.000Z", "t2"),
    card("other", "2026-11-07T12:00:00.000Z", undefined, "/elsewhere"),
  ];

  it("lists only the runs of the repository around the directory", () => {
    expect(runsOfSpace(cards, "/repo/sub").map((r) => r.status.run)).toEqual(["new", "old"]);
    expect(runsOfSpace(cards, undefined)).toEqual([]);
    expect(runsOfSpace(cards, "/repository")).toEqual([]);
  });

  it("selects the run whose host terminal is the active terminal", () => {
    const runs = runsOfSpace(cards, "/repo");
    expect(selectRun(runs, "t1", null, NOW)?.status.run).toBe("old");
  });

  it("falls back to the latest run when no terminal matches", () => {
    const runs = runsOfSpace(cards, "/repo");
    expect(selectRun(runs, "t9", null, NOW)?.status.run).toBe("new");
  });

  it("lets the picker win and ignores a stale pick", () => {
    const runs = runsOfSpace(cards, "/repo");
    expect(selectRun(runs, "t2", pickRunOf(runs, runs[1]!.key), NOW)?.status.run).toBe("old");
    expect(selectRun(runs, "t2", { key: "gone", seen: 0 }, NOW)?.status.run).toBe("new");
  });

  it("lists the runs newest first by start, updated breaking the tie", () => {
    const list = [
      card("a", "2026-11-07T12:30:00.000Z", "t7", "/repo", "2026-11-07T10:00:00.000Z"),
      card("b", "2026-11-07T11:00:00.000Z", "t7", "/repo", "2026-11-07T11:00:00.000Z"),
      card("c", "2026-11-07T11:30:00.000Z", "t7", "/repo", "2026-11-07T11:00:00.000Z"),
    ];
    for (const order of [list, [...list].reverse()]) {
      expect(runsOfSpace(order, "/repo").map((r) => r.status.run)).toEqual(["c", "b", "a"]);
    }
  });

  it("shows the running run of the terminal with the latest update, then its latest run", () => {
    const stale = card(
      "stale",
      "2026-11-07T08:00:00.000Z",
      "t7",
      "/repo",
      "2026-11-07T07:00:00.000Z",
    );
    const done = card(
      "done",
      "2026-11-07T11:30:00.000Z",
      "t7",
      "/repo",
      "2026-11-07T11:00:00.000Z",
      "finished",
    );
    const run1 = card(
      "run1",
      "2026-11-07T12:10:00.000Z",
      "t7",
      "/repo",
      "2026-11-07T09:00:00.000Z",
    );
    const run2 = card(
      "run2",
      "2026-11-07T12:40:00.000Z",
      "t7",
      "/repo",
      "2026-11-07T08:30:00.000Z",
    );
    const other = card(
      "other",
      "2026-11-07T12:50:00.000Z",
      "t8",
      "/repo",
      "2026-11-07T12:50:00.000Z",
    );
    const list = [stale, done, run1, run2, other];
    for (const order of [list, [...list].reverse()]) {
      const run = (id: string | null) =>
        selectRun(runsOfSpace(order, "/repo"), id, null, NOW)?.status.run;
      expect(run("t7")).toBe("run2");
      // No running run left: the latest run of the terminal, not the stale one before it.
      const noRunning = order.filter(
        (c) => c.run === "stale" || c.run === "done" || c.run === "other",
      );
      expect(selectRun(runsOfSpace(noRunning, "/repo"), "t7", null, NOW)?.status.run).toBe("done");
      // Another terminal, or none: the latest run of the repository.
      expect(run("t9")).toBe("other");
      expect(run(null)).toBe("other");
    }
  });

  it("drops a pick when a run starts after it, when it is gone or the repository changes", () => {
    const a = card("a", "2026-11-07T10:00:00.000Z", "t1");
    const b = card("b", "2026-11-07T11:00:00.000Z", "t1");
    const runs = runsOfSpace([a, b], "/repo");
    const older = pickRunOf(runs, runs[1]!.key);
    expect(pickHolds(runs, older)).toBe(true);
    expect(selectRun(runs, "t1", older, NOW)?.status.run).toBe("a");
    const later = runsOfSpace([a, b, card("c", "2026-11-07T12:00:00.000Z", "t1")], "/repo");
    expect(pickHolds(later, older)).toBe(false);
    expect(selectRun(later, "t1", older, NOW)?.status.run).toBe("c");
    expect(pickHolds(runsOfSpace([b], "/repo"), older)).toBe(false);
    expect(pickHolds(runsOfSpace([a, b], "/elsewhere"), older)).toBe(false);
    expect(pickHolds(runs, null)).toBe(false);
  });

  it("has no run for a space without runs", () => {
    expect(selectRun([], "t1", null, NOW)).toBeUndefined();
  });

  it("keeps valid event rows and drops malformed ones", () => {
    const events = runsOfSpace(cards, "/repo")[1]!.status.events;
    expect(events.map((e) => e.type)).toEqual([
      "run.started",
      "step.finished",
      "bogus-row-without-tone",
    ]);
    expect(events[1]).toMatchObject({ tone: "info", task: "t5", step: "implement" });
    expect(events[2]!.tone).toBe("neutral");
  });
});

describe("runs of a remote space (#692)", () => {
  const local = card("r1", "2026-11-07T10:00:00.000Z", "t1");
  const remote: DeliverCard = {
    ...card("r1", "2026-11-07T11:00:00.000Z", "box::t1"),
    machine: "box",
  };

  it("keeps a local and a remote run with the same run id and path apart", () => {
    const cards = [local, remote];
    expect(runsOfSpace(cards, "/repo").map((r) => r.machine)).toEqual([undefined]);
    const onBox = runsOfSpace(cards, "/repo", "box");
    expect(onBox.map((r) => r.machine)).toEqual(["box"]);
    expect(onBox[0]!.key).not.toBe(runsOfSpace(cards, "/repo")[0]!.key);
    expect(runsOfSpace(cards, "/repo", "gpu")).toEqual([]);
    // The remote run is found by the qualified terminal of the machine's snapshot.
    expect(selectRun(onBox, "box::t1", null, NOW)?.status.updated).toBe("2026-11-07T11:00:00.000Z");
  });

  it("labels a remote run with its machine and an offline one as offline", () => {
    const now = Date.parse("2026-11-07T11:05:00.000Z");
    const [run] = runsOfSpace([{ ...remote, offline: true }], "/repo", "box");
    expect(run!.offline).toBe(true);
    expect(runLabel(run!, now)).toMatch(/^box: .*\(running, 5 min ago, offline\)$/);
    expect(runLabel(runsOfSpace([local], "/repo")[0]!, now)).not.toContain("offline");
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

describe("run label", () => {
  const NOW = Date.parse("2026-11-07T10:02:00.000Z");
  const UPDATED = "2026-11-07T10:00:00.000Z";
  const choice = (extra: Record<string, unknown>, root = "/src/poimen"): RunChoice => {
    const status = parseStatus({
      v: 1,
      run: "20261110-042412-3a7a",
      status: "running",
      tasks: [],
      finished: {},
      attentions: [],
      needsYou: false,
      links: [],
      updated: UPDATED,
      tone: "neutral",
      host: {},
      events: [],
      ...extra,
    });
    if (typeof status === "string") throw new Error(status);
    return { key: "k", root, status };
  };
  const task = (step: unknown) => ({
    task: "t177",
    subject: { kind: "github.issue", id: "177", title: "#177" },
    step,
  });

  it("shows subject, progress, current task with step, state and age", () => {
    const run = choice({
      subject: { kind: "github.milestone", id: "M014", title: "M014" },
      progress: { done: 3, total: 18 },
      tasks: [task({ name: "review", n: 4, total: 7 })],
    });
    expect(runLabel(run, NOW)).toBe("M014 3/18 · #177 review 4/7 (running, 2 min ago)");
  });

  it("leaves out progress and task when the run has none", () => {
    const run = choice({ subject: { kind: "x", id: "1", title: "M014" } });
    expect(runLabel(run, NOW)).toBe("M014 (running, 2 min ago)");
  });

  it("shows the step name alone without n and total", () => {
    const run = choice({
      subject: { kind: "x", id: "1", title: "M014" },
      tasks: [task({ name: "review" })],
    });
    expect(runLabel(run, NOW)).toBe("M014 · #177 review (running, 2 min ago)");
  });

  it("shortens a long task subject", () => {
    expect(shorten("a".repeat(50), 10)).toBe("aaaaaaaaa…");
    expect(shorten("short", 10)).toBe("short");
  });

  it("titles a run without subject by folder, harness and start time, not the id", () => {
    const ts = "2026-11-07T10:00:00.000Z";
    const clock = clockText(ts);
    const run = choice({
      agent: { harness: "claude" },
      events: [{ type: "run.started", tone: "info", ts }],
    });
    expect(runTitle(run.status, run.root)).toBe(`poimen · claude · ${clock}`);
    expect(runLabel(run, NOW)).toContain(`poimen · claude · ${clock} (running`);
  });

  it("falls back to the run id when nothing else is known", () => {
    const run = choice({});
    expect(runTitle(run.status)).toBe("20261110-042412-3a7a");
    expect(runTitle(run.status, "/src/poimen\\")).toBe("poimen");
  });
});

describe("foldSkipped", () => {
  const row = (over: Partial<RunEventRow>): RunEventRow => ({
    seq: null,
    ts: null,
    type: "progress",
    tone: "neutral",
    task: null,
    step: null,
    outcome: null,
    link: null,
    ...over,
  });
  const skip = (
    task: string,
    tone: RunEventRow["tone"] = "neutral",
    reason = "blocked",
    text?: string,
  ) => [
    row({ type: "task.started", task, subject: `PBI ${task}`, text: `what ${task}`, tone }),
    row({
      type: "task.finished",
      task,
      outcome: "skipped",
      reason,
      ...(text ? { text } : {}),
      tone,
    }),
  ];

  it("folds consecutive skipped pairs into one item with count and reason", () => {
    const items = foldSkipped([
      row({ type: "run.started" }),
      ...skip("a"),
      ...skip("b"),
      row({ type: "task.started", task: "c" }),
    ]);
    expect(items.map((i) => i.kind)).toEqual(["row", "skipped", "row"]);
    const fold = items[1]!;
    if (fold.kind !== "skipped") throw new Error("fold");
    expect(fold.label).toBe("2 skipped (blocked)");
    expect(fold.tasks).toEqual([
      { task: "a", subject: "PBI a", text: "what a", reason: "blocked" },
      { task: "b", subject: "PBI b", text: "what b", reason: "blocked" },
    ]);
  });

  it("shows the text of the finished task instead of its reason code (#622)", () => {
    const fold = foldSkipped([...skip("a", "neutral", "blocked", "Blocked by #12")])[0]!;
    if (fold.kind !== "skipped") throw new Error("fold");
    expect(fold.tasks[0]!.reason).toBe("Blocked by #12");
    expect(fold.label).toBe("1 skipped (Blocked by #12)");
  });

  it("lists several reasons and none", () => {
    const [a, b] = [skip("a", "neutral", "blocked"), skip("b", "neutral", "no repro")];
    const fold = foldSkipped([...a, ...b])[0]!;
    expect(fold.kind === "skipped" && fold.label).toBe("2 skipped (blocked, no repro)");
    const none = foldSkipped(skip("a", "neutral", ""))[0]!;
    expect(none.kind === "skipped" && none.label).toBe("1 skipped");
  });

  it("does not fold when an event sits between, the task differs or the outcome is not skipped", () => {
    const [s, f] = skip("a");
    expect(foldSkipped([s!, row({ type: "step.started", task: "a" }), f!]).length).toBe(3);
    expect(foldSkipped([s!, { ...f!, task: "b" }]).length).toBe(2);
    expect(foldSkipped([s!, { ...f!, outcome: "succeeded" }]).length).toBe(2);
  });

  it("never folds warning or danger rows", () => {
    expect(foldSkipped(skip("a", "warning")).length).toBe(2);
    expect(foldSkipped(skip("a", "danger")).length).toBe(2);
    const mixed = foldSkipped([...skip("a"), ...skip("b", "warning"), ...skip("c")]);
    expect(mixed.map((i) => i.kind)).toEqual(["skipped", "row", "row", "skipped"]);
  });

  it("keeps the key of a fold stable while it grows", () => {
    const start = { ...skip("a")[0]!, seq: 4 };
    const one = foldSkipped([start, skip("a")[1]!]);
    const two = foldSkipped([start, skip("a")[1]!, ...skip("b")]);
    expect(one[0]!.kind === "skipped" && one[0]!.key).toBe(
      two[0]!.kind === "skipped" && two[0]!.key,
    );
  });
});

describe("runs by subject (#780)", () => {
  const withSubject = (c: DeliverCard, id: string | null, outcome?: string): DeliverCard => {
    if (c.state.kind !== "ok") throw new Error("not ok");
    return {
      ...c,
      state: {
        kind: "ok",
        status: {
          ...c.state.status,
          subject: id ? { kind: "github.milestone", id, title: `M${id}`, url: null } : null,
          ...(outcome ? { status: "finished" as const, outcome } : {}),
        },
      },
    };
  };
  const choices = (cards: DeliverCard[]) => runsOfSpace(cards, "/repo");
  const cards = [
    withSubject(card("a1", "2026-11-07T10:00:00.000Z"), "1"),
    withSubject(card("b1", "2026-11-07T11:00:00.000Z"), "2"),
    withSubject(card("a2", "2026-11-07T12:00:00.000Z"), "1"),
    withSubject(card("solo", "2026-11-07T09:00:00.000Z"), null),
  ];

  it("groups by subject, newest group first, newest run first, no subject alone", () => {
    const groups = groupBySubject(choices(cards));
    expect(groups.map((g) => g.runs.map((r) => r.status.run))).toEqual([
      ["a2", "a1"],
      ["b1"],
      ["solo"],
    ]);
    expect(groups[0]!.subject?.id).toBe("1");
    expect(groups[2]!.subject).toBeNull();
  });

  it("merges the events of a group oldest run first behind a separator per run", () => {
    const [group] = groupBySubject(
      choices([cards[0]!, withSubject(card("a2", "2026-11-07T12:00:00.000Z"), "1", "succeeded")]),
    );
    const items = mergedItems(group!, NOW);
    const perRun = foldSkipped(group!.runs[0]!.status.events).length;
    expect(items.length).toBe(2 * (perRun + 1));
    expect(items[0]!.kind).toBe("separator");
    expect(items[perRun + 1]!.kind).toBe("separator");
    const seps = items.filter((i) => i.kind === "separator");
    expect(seps.map((s) => s.key)).toEqual([group!.runs[1]!.key, group!.runs[0]!.key]);
    expect(seps[1]).toMatchObject({ state: "succeeded", outcome: "succeeded" });
    expect(seps[0]).toMatchObject({ state: "stale", outcome: "" });
    expect(seps[0]!.start).toMatch(/^\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}$/);
  });
});
