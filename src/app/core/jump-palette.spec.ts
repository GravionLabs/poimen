import { Injector, signal } from "@angular/core";
import { describe, expect, it } from "vitest";
import { Attention } from "./attention";
import { JumpPalette, shortAge } from "./jump-palette";
import { Machines } from "./machines";
import type { Snapshot, Terminal } from "./snapshot";
import { SpacesStore } from "./spaces-store";
import { UiState } from "./ui-state";

const term = (id: string, space: string, extra: Partial<Terminal> = {}): Terminal => ({
  terminal_id: id,
  tab_id: `${space}:t`,
  space_id: space,
  agent_status: "idle",
  ...extra,
});

function setup(
  opts: {
    unread?: string[];
    recent?: Record<string, number>;
    extra?: number;
  } = {},
) {
  const snapshot: Snapshot = {
    spaces: [
      {
        space_id: "s1",
        label: "alpha",
        number: 1,
        agent_status: "idle",
        cwd: "/src/alpha",
      },
      {
        space_id: "s2",
        label: "beta",
        number: 2,
        agent_status: "idle",
        cwd: "/src/beta",
      },
      {
        space_id: "s3",
        label: "gamma",
        number: 3,
        agent_status: "idle",
        cwd: "/src/gamma",
      },
      {
        space_id: "m::r1",
        label: "remote",
        number: 1,
        agent_status: "idle",
        machine: "m",
      },
    ],
    tabs: [],
    terminals: [
      term("a1", "s1", {
        agent: "claude",
        agent_status: "done",
        cwd: "/src/alpha",
      }),
      term("b1", "s2", {
        agent: "claude",
        agent_status: "blocked",
        cwd: "/src/beta",
      }),
      term("g1", "s3", { cwd: "/src/gamma" }),
      term("m::p1", "m::r1", { agent: "claude", machine: "m" }),
    ],
  };
  for (let i = 0; i < (opts.extra ?? 0); i++)
    snapshot.spaces.push({
      space_id: `x${i}`,
      label: `x${i}`,
      number: 10 + i,
      agent_status: "idle",
    });
  const injector = Injector.create({
    providers: [
      {
        provide: SpacesStore,
        useValue: {
          all: signal(snapshot),
          snapshot: signal(snapshot),
          repos: signal(new Map([["/src/gamma", { branch: "feat/42-thing", worktree: false }]])),
          pulls: signal(new Map([["/src/beta", { number: 99 }]])),
        },
      },
      {
        provide: UiState,
        useValue: { recentSpaces: signal(opts.recent ?? {}) },
      },
      {
        provide: Attention,
        useValue: {
          unread: signal(new Set(opts.unread ?? [])),
          changedAt: signal(new Map([["b1", Date.now() - 3 * 60_000]])),
        },
      },
      {
        provide: Machines,
        useValue: {
          blockedReason: (id: string) => (id.startsWith("m::") ? "m is offline" : null),
        },
      },
      JumpPalette,
    ],
  });
  return injector.get(JumpPalette);
}

const labels = (p: JumpPalette) =>
  p.rows().map((r) => (r.type === "heading" ? `# ${r.label}` : r.entry.key));

describe("JumpPalette", () => {
  it("lists nothing without agents needing you or recent spaces", () => {
    const p = setup();
    p.open(null);
    expect(p.rows().map((r) => r.key)).toEqual(["heading:Needs you", "pane:b1"]);
  });

  it("shows Needs you (blocked, then done and unread) and Recent without the active space", () => {
    const p = setup({ unread: ["a1"], recent: { s1: 1, s2: 3, s3: 2 } });
    p.open("s3");
    expect(labels(p)).toEqual([
      "# Needs you",
      "pane:b1",
      "pane:a1",
      "# Recent",
      "space:s2",
      "space:s1",
    ]);
  });

  it("keeps at most 8 recent spaces, newest first", () => {
    const recent = Object.fromEntries(Array.from({ length: 12 }, (_, i) => [`x${i}`, i + 1]));
    const p = setup({ recent, extra: 12 });
    p.open(null);
    const keys = p.entries().map((e) => e.key);
    expect(keys.slice(1)).toEqual(
      ["x11", "x10", "x9", "x8", "x7", "x6", "x5", "x4"].map((id) => `space:${id}`),
    );
  });

  it("filters agents, terminals and spaces by label, directory and branch", () => {
    const p = setup();
    p.open(null);
    p.setQuery("gamma");
    expect(p.entries().map((e) => e.key)).toContain("space:s3");
    expect(p.entries().map((e) => e.key)).not.toContain("space:s1");
    p.setQuery("feat/42");
    expect(p.entries().map((e) => e.key)).toContain("pane:g1");
  });

  it("matches #123 against label, branch and pull request number", () => {
    const p = setup();
    p.open(null);
    p.setQuery("#42");
    expect(p.entries().map((e) => e.key)).toEqual(["pane:g1", "space:s3"]);
    p.setQuery("#99");
    expect(p.entries().map((e) => e.key)).toEqual(["pane:b1", "space:s2"]);
    p.setQuery("#4");
    expect(p.entries()).toEqual([]);
  });

  it("disables a remote entry with the reason and names its machine", () => {
    const p = setup();
    p.open(null);
    p.setQuery("remote");
    const remote = p.entries().find((e) => e.key === "space:m::r1");
    expect(remote?.disabled).toBe("m is offline");
    expect(remote?.title).toBe("m: remote");
    expect(p.entries().find((e) => e.key === "space:s1")).toBeUndefined();
  });

  it("targets the first agent pane of a space, none for a space without one", () => {
    const p = setup();
    p.open(null);
    p.setQuery("beta");
    expect(p.entries().find((e) => e.key === "space:s2")?.pane?.terminal_id).toBe("b1");
    p.setQuery("gamma");
    expect(p.entries().find((e) => e.key === "space:s3")?.pane).toBeUndefined();
  });

  it("moves with wrap-around", () => {
    const p = setup({ unread: ["a1"] });
    p.open(null);
    p.move(-1);
    expect(p.current()?.key).toBe("pane:a1");
    p.move(1);
    expect(p.current()?.key).toBe("pane:b1");
  });

  it("shows the age since the last status change", () => {
    const p = setup();
    p.open(null);
    const row = p.rows().find((r) => r.type === "item" && r.key === "pane:b1");
    expect(row?.type === "item" && row.age).toBe("3m");
    const none = p.rows().find((r) => r.type === "item" && r.key === "pane:a1");
    expect(none).toBeUndefined();
    p.setQuery("alpha");
    const a = p.rows().find((r) => r.type === "item" && r.key === "pane:a1");
    expect(a?.type === "item" && a.age).toBeNull();
  });

  it("names the field and the match when it is not in the name", () => {
    const p = setup();
    p.open(null);
    p.setQuery("feat/42");
    const row = p.rows().find((r) => r.type === "item" && r.key === "pane:g1");
    expect(row?.type === "item" && row.context).toEqual({
      field: "branch",
      text: "feat/42-thing",
      positions: [0, 1, 2, 3, 4, 5, 6],
    });
    p.setQuery("beta");
    const byName = p.rows().find((r) => r.type === "item" && r.key === "space:s2");
    expect(byName?.type === "item" && byName.context).toBeNull();
  });

  it("names the directory when only the directory matches", () => {
    const p = setup();
    p.open(null);
    p.setQuery("src/gam");
    const row = p.rows().find((r) => r.type === "item" && r.key === "space:s3");
    expect(row?.type === "item" && row.context?.field).toBe("directory");
  });

  it("names the pull request or branch number for a #123 query", () => {
    const p = setup();
    p.open(null);
    p.setQuery("#99");
    const pr = p.rows().find((r) => r.type === "item" && r.key === "pane:b1");
    expect(pr?.type === "item" && pr.context).toEqual({
      field: "pull request",
      text: "#99",
      positions: [0, 1, 2],
    });
    p.setQuery("#42");
    const branch = p.rows().find((r) => r.type === "item" && r.key === "pane:g1");
    expect(branch?.type === "item" && branch.context).toEqual({
      field: "branch",
      text: "feat/42-thing",
      positions: [5, 6],
    });
  });

  it("announces the result count", () => {
    const p = setup();
    p.open(null);
    expect(p.announcement()).toBe("1 result");
    p.setQuery("zzz");
    expect(p.announcement()).toBe("No results");
    p.setQuery("a");
    expect(p.announcement()).toMatch(/^\d+ results$/);
  });
});

describe("shortAge", () => {
  it("is short", () => {
    const now = 10_000_000_000;
    expect(shortAge(now - 5_000, now)).toBe("now");
    expect(shortAge(now - 3 * 60_000, now)).toBe("3m");
    expect(shortAge(now - 2 * 3_600_000, now)).toBe("2h");
    expect(shortAge(now - 49 * 3_600_000, now)).toBe("2d");
    expect(shortAge(now + 5_000, now)).toBe("now");
  });
});
