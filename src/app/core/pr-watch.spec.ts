import { describe, expect, it } from "vitest";
import type { Checks } from "../github/pulls";
import { NotificationCenter, type CheckResult } from "./notification-center";
import {
  anyPending,
  finishedChecks,
  isFinal,
  watchedPulls,
  type Seen,
  type WatchedPull,
} from "./pr-watch";
import type { BranchPull, SpaceNode } from "./snapshot";

const checks = (state: Checks["state"], passed = 0, failed = 0, pending = 0): Checks => ({
  state,
  passed,
  failed,
  pending,
});
const pull = (sha: string, c: Checks, key = "u/1"): WatchedPull => ({ key, sha, checks: c });
const pending = pull("a", checks("pending", 1, 0, 2));

describe("isFinal", () => {
  it("is final with an outcome and nothing pending", () => {
    expect(isFinal(checks("passing", 3))).toBe(true);
    expect(isFinal(checks("failing", 2, 1))).toBe(true);
    expect(isFinal(checks("failing", 2, 1, 1))).toBe(false);
    expect(isFinal(checks("pending", 0, 0, 1))).toBe(false);
    expect(isFinal(checks("none"))).toBe(false);
  });
});

describe("finishedChecks", () => {
  const seenPending = new Map<string, Seen>([["u/1", { sha: "a", pending: true }]]);

  it("reports a pull request that was pending and is final now", () => {
    const { finished, next } = finishedChecks(seenPending, new Set(), [
      pull("a", checks("passing", 3)),
    ]);
    expect(finished.map((f) => f.key)).toEqual(["u/1"]);
    expect(next.get("u/1")).toEqual({ sha: "a", pending: false });
  });

  it("reports a failed result", () => {
    const { finished } = finishedChecks(seenPending, new Set(), [
      pull("a", checks("failing", 2, 1)),
    ]);
    expect(finished).toHaveLength(1);
  });

  it("is silent for a result seen for the first time", () => {
    const { finished } = finishedChecks(new Map(), new Set(), [pull("a", checks("passing", 3))]);
    expect(finished).toEqual([]);
  });

  it("is silent while checks are still pending, and remembers that they are", () => {
    const { finished, next } = finishedChecks(new Map(), new Set(), [pending]);
    expect(finished).toEqual([]);
    expect(next.get("u/1")).toEqual({ sha: "a", pending: true });
  });

  it("does not report a new head commit that is already final", () => {
    const { finished } = finishedChecks(seenPending, new Set(), [pull("b", checks("passing", 3))]);
    expect(finished).toEqual([]);
  });

  it("reports each head commit once", () => {
    const reported = new Set(["u/1@a"]);
    expect(
      finishedChecks(seenPending, reported, [pull("a", checks("passing", 3))]).finished,
    ).toEqual([]);
    const again = finishedChecks(new Map([["u/1", { sha: "b", pending: true }]]), reported, [
      pull("b", checks("failing", 0, 1)),
    ]);
    expect(again.finished).toHaveLength(1);
  });

  it("forgets a pull request that is no longer watched", () => {
    expect(finishedChecks(seenPending, new Set(), []).next.size).toBe(0);
  });

  it("ignores a pull request without a head commit", () => {
    const { finished, next } = finishedChecks(seenPending, new Set(), [
      pull("", checks("passing", 3)),
    ]);
    expect(finished).toEqual([]);
    expect(next.size).toBe(0);
  });
});

describe("watchedPulls", () => {
  const space = (id: string, cwd?: string): SpaceNode => ({
    workspace: { space_id: id, label: id, number: 1, agent_status: "idle", cwd },
    rows: [],
  });
  const bp = (
    url: string,
    state: "open" | "merged",
    c = checks("pending", 0, 0, 1),
  ): BranchPull => ({
    number: 1,
    title: "t",
    url,
    state,
    isDraft: false,
    checks: c,
  });

  it("keeps the open pull requests of spaces, one per pull request", () => {
    const pulls = new Map([
      ["/a", bp("u1", "open")],
      ["/b", bp("u1", "open")],
      ["/c", bp("u2", "merged")],
    ]);
    const spaces = [space("a", "/a"), space("b", "/b"), space("c", "/c"), space("d")];
    expect(watchedPulls(spaces, pulls).map((w) => w.space.workspace.space_id)).toEqual(["a"]);
  });

  it("asks for a read only while a check is pending", () => {
    const open = (c: Checks) => [{ pull: bp("u", "open", c) }];
    expect(anyPending(open(checks("pending", 1, 0, 1)))).toBe(true);
    expect(anyPending(open(checks("passing", 2)))).toBe(false);
    expect(anyPending([])).toBe(false);
  });
});

describe("NotificationCenter.recordChecks", () => {
  const result = (passed: boolean): CheckResult => ({
    passed,
    number: 12,
    title: "Add the thing",
    url: "u",
    sha: "a",
    label: "my space",
    paneId: "t1",
    dir: "/a",
    branch: "feat/a",
    base: "main",
    crossRepo: false,
  });

  it("adds an entry with the pull request and the space", () => {
    const center = new NotificationCenter();
    center.recordChecks([result(true), result(false)], 5);
    const [failed, passed] = center.entries();
    expect(passed).toMatchObject({ status: "done", label: "my space", paneId: "t1", read: false });
    expect(passed.message).toBe("Checks of #12 passed: Add the thing");
    expect(failed).toMatchObject({ status: "error" });
    expect(failed.checks?.passed).toBe(false);
  });
});
