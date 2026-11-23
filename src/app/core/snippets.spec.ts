import { Injector } from "@angular/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { AgentPrompts } from "./agent-prompt";
import { Machines } from "./machines";
import type { Terminal } from "./snapshot";
import { SnippetRunner } from "./snippets";
import { offeredFor, parseSnippets, type Snippet } from "./snippets-model";
import { SpaceActions } from "./space-actions";
import { TauriBridge } from "./tauri";
import { tauri } from "./tauri.mock";

vi.mock("@tauri-apps/api/core", async () => ({
  invoke: (await import("./tauri.mock")).tauri.invoke,
}));
vi.mock("@tauri-apps/api/event", async () => ({
  listen: (await import("./tauri.mock")).tauri.listen,
}));

const shell = { terminal_id: "t1" } as Terminal;
const agent = { terminal_id: "t2", agent: "claude" } as Terminal;
const tests: Snippet = { label: "tests", text: "cargo test", enter: true, target: "terminal" };
const deliver: Snippet = { label: "deliver", text: "/deliver-loop", enter: false, target: "agent" };

describe("parseSnippets", () => {
  it("defaults enter to true and target to terminal, skips bad entries and repeated labels", () => {
    expect(
      parseSnippets([
        { label: " tests ", text: "cargo test" },
        { label: "tests", text: "again" },
        { label: "", text: "x" },
        { label: "empty", text: "  " },
        { label: "a", text: "/x", enter: false, target: "agent" },
        7,
      ]),
    ).toEqual([tests, { label: "a", text: "/x", enter: false, target: "agent" }]);
    expect(parseSnippets({})).toEqual([]);
  });
});

describe("offeredFor", () => {
  it("offers an agent snippet for agent panes only", () => {
    expect(offeredFor(deliver, agent)).toBe(true);
    expect(offeredFor(deliver, shell)).toBe(false);
    expect(offeredFor(tests, shell)).toBe(true);
    expect(offeredFor(tests, undefined)).toBe(false);
  });
});

describe("SnippetRunner", () => {
  let runner: SnippetRunner;
  let blocked: string | null;
  const send = vi.fn();
  beforeEach(() => {
    tauri.reset();
    blocked = null;
    send.mockReset().mockResolvedValue({ kind: "started", status: "working" });
    const injector = Injector.create({
      providers: [
        TauriBridge,
        SnippetRunner,
        { provide: AgentPrompts, useValue: { send } },
        { provide: Machines, useValue: { blockedReason: () => blocked } },
        { provide: SpaceActions, useValue: { notify: vi.fn(), fail: vi.fn() } },
      ],
    });
    runner = injector.get(SnippetRunner);
  });

  it("types a terminal snippet with its enter flag", async () => {
    await runner.run(tests, shell);
    await runner.run({ ...tests, enter: false }, shell);
    expect(tauri.calls("terminal_type")).toEqual([
      { target: "t1", text: "cargo test", enter: true },
      { target: "t1", text: "cargo test", enter: false },
    ]);
  });

  it("sends an agent snippet through the prompt path, never as typed input", async () => {
    await runner.run(deliver, agent);
    expect(send).toHaveBeenCalledWith("t2", "/deliver-loop");
    expect(tauri.calls("terminal_type")).toEqual([]);
  });

  it("sends an agent snippet to no plain terminal", async () => {
    await runner.run(deliver, shell);
    expect(send).not.toHaveBeenCalled();
    expect(tauri.calls("terminal_type")).toEqual([]);
  });

  it("writes nothing to a pane of an offline machine", async () => {
    blocked = "Machine box is offline";
    await runner.run(tests, shell);
    expect(tauri.calls("terminal_type")).toEqual([]);
  });
});
