import { describe, expect, it } from "vitest";
import { cardsOf, type DeliverCard, type DeliverRead } from "./deliver-status";
import {
  activeLoop,
  milestoneName,
  parseMax,
  requestOf,
  targetError,
  targetText,
} from "./deliver-start";

const MILESTONE = { kind: "milestone", name: "M016" } as const;
const FORM = { harness: "", model: "", max: "" };

describe("the target of a loop (#746)", () => {
  it("takes the M<nnn> a milestone title starts with, never a prefix of another name", () => {
    expect(milestoneName("M016")).toBe("M016");
    expect(milestoneName("M016 (0.34.0)")).toBe("M016");
    expect(milestoneName("M014.1 Runs view")).toBeNull();
    expect(milestoneName("Roadmap")).toBeNull();
    expect(milestoneName("M1234567")).toBeNull();
  });

  it("rejects a bad milestone name or issue number", () => {
    expect(targetError(MILESTONE)).toBeNull();
    for (const name of ["", "016", "M", "M1;ls", "m16"]) {
      expect(targetError({ kind: "milestone", name }), name).not.toBeNull();
    }
    expect(targetError({ kind: "issues", numbers: [12, 15] })).toBeNull();
    for (const numbers of [[], [0], [1.5], [-3], [Number.NaN]]) {
      expect(targetError({ kind: "issues", numbers }), String(numbers)).not.toBeNull();
    }
  });

  it("names the target for the user", () => {
    expect(targetText(MILESTONE)).toBe("M016");
    expect(targetText({ kind: "issues", numbers: [12, 15] })).toBe("#12, #15");
  });

  it("reads --max as a positive whole number, empty meaning the default", () => {
    expect(parseMax("")).toEqual({ ok: true });
    expect(parseMax(" 3 ")).toEqual({ ok: true, value: 3 });
    for (const text of ["0", "-1", "1.5", "x", "1e3", "3 4", "1234567"]) {
      expect(parseMax(text), text).toEqual({ ok: false });
    }
  });
});

describe("the request of the dialog", () => {
  it("leaves out an empty harness, model and max", () => {
    expect(requestOf(MILESTONE, FORM)).toEqual({ target: MILESTONE });
    expect(requestOf(MILESTONE, { harness: "copilot", model: " gpt 5 ", max: "2" })).toEqual({
      target: MILESTONE,
      harness: "copilot",
      model: "gpt 5",
      max: 2,
    });
  });

  it("drops a harness outside the fixed list and refuses bad values", () => {
    expect(requestOf(MILESTONE, { ...FORM, harness: "claude; rm -rf ~" })).toEqual({
      target: MILESTONE,
    });
    expect(requestOf(MILESTONE, { ...FORM, max: "0" })).toBeNull();
    expect(requestOf({ kind: "milestone", name: "x" }, FORM)).toBeNull();
  });
});

describe("a second start for a repository", () => {
  const NOW = Date.parse("2026-11-23T12:00:00Z");
  const run = (status: string, updated: string, started: string): DeliverCard =>
    cardsOf("/r", {
      kind: "events",
      runs: [
        {
          run: "x",
          status,
          updated,
          started,
          subject: null,
          progress: null,
          tasks: [],
          finished: [],
          attentions: [],
          needsYou: false,
          links: [],
          gaps: [],
          tone: "neutral",
          host: {},
          events: [],
        },
      ],
    } as unknown as DeliverRead)[0]!;
  const terminals = [{ terminal_id: "t1" }];
  const launched = { terminalId: "t1", at: NOW - 10_000 };

  it("is refused for a running, not stale run", () => {
    const running = run("running", "2026-11-23T11:59:00Z", "2026-11-23T11:00:00Z");
    expect(activeLoop([running], undefined, [], "/r", NOW)).toEqual({ kind: "running" });
  });

  it("is not refused for a stale run or a run of another repository", () => {
    const stale = run("running", "2026-11-23T07:00:00Z", "2026-11-23T06:00:00Z");
    expect(activeLoop([stale], undefined, [], "/r", NOW)).toBeUndefined();
    const running = run("running", "2026-11-23T11:59:00Z", "2026-11-23T11:00:00Z");
    expect(activeLoop([running], undefined, [], "/other", NOW)).toBeUndefined();
  });

  it("is refused while the terminal of the start exists and no run file has appeared", () => {
    expect(activeLoop([], launched, terminals, "/r", NOW)).toEqual({
      kind: "pending",
      terminalId: "t1",
    });
    // The terminal is gone: nothing waits for a run file.
    expect(activeLoop([], launched, [], "/r", NOW)).toBeUndefined();
  });

  it("is not refused once the run of the start has appeared and finished", () => {
    const done = run("finished", "2026-11-23T11:59:55Z", "2026-11-23T11:59:52Z");
    expect(activeLoop([done], launched, terminals, "/r", NOW)).toBeUndefined();
    // A run that ended before the start is not the loop's own.
    const before = run("finished", "2026-11-23T10:00:00Z", "2026-11-23T09:00:00Z");
    expect(activeLoop([before], launched, terminals, "/r", NOW)).toEqual({
      kind: "pending",
      terminalId: "t1",
    });
  });
});
