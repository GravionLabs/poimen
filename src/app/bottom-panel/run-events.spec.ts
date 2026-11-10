import { describe, expect, it } from "vitest";
import { clockText, parseStatus, runTitle, type DeliverCard } from "../deliver/deliver-status";
import {
  eventTime,
  runLabel,
  runsOfSpace,
  selectRun,
  shorten,
  toneStyle,
  type RunChoice,
} from "./run-events";

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
