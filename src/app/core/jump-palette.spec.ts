import { Injector, signal } from "@angular/core";
import { describe, expect, it } from "vitest";
import { Attention } from "./attention";
import { JumpPalette } from "./jump-palette";
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

function setup(opts: { unread?: string[]; recent?: Record<string, number>; extra?: number } = {}) {
  const snapshot: Snapshot = {
    spaces: [
      { space_id: "s1", label: "alpha", number: 1, agent_status: "idle", cwd: "/src/alpha" },
      { space_id: "s2", label: "beta", number: 2, agent_status: "idle", cwd: "/src/beta" },
      { space_id: "s3", label: "gamma", number: 3, agent_status: "idle", cwd: "/src/gamma" },
      { space_id: "m::r1", label: "remote", number: 1, agent_status: "idle", machine: "m" },
    ],
    tabs: [],
    terminals: [
      term("a1", "s1", { agent: "claude", agent_status: "done", cwd: "/src/alpha" }),
      term("b1", "s2", { agent: "claude", agent_status: "blocked", cwd: "/src/beta" }),
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
      { provide: UiState, useValue: { recentSpaces: signal(opts.recent ?? {}) } },
      { provide: Attention, useValue: { unread: signal(new Set(opts.unread ?? [])) } },
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
});
