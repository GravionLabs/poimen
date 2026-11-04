import { Injector } from "@angular/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { AgentPrompts, promptMessage, promptSucceeded } from "./agent-prompt";
import { TauriBridge } from "./tauri";
import { tauri } from "./tauri.mock";

vi.mock("@tauri-apps/api/core", async () => ({
  invoke: (await import("./tauri.mock")).tauri.invoke,
}));
vi.mock("@tauri-apps/api/event", async () => ({
  listen: (await import("./tauri.mock")).tauri.listen,
}));

describe("AgentPrompts", () => {
  let prompts: AgentPrompts;
  beforeEach(() => {
    tauri.reset();
    prompts = Injector.create({ providers: [TauriBridge, AgentPrompts] }).get(AgentPrompts);
  });

  it("invokes agent_prompt with the pane and the text", async () => {
    tauri.answer("agent_prompt", () => ({ kind: "started", status: "working" }));
    const outcome = await prompts.send("w1:p2", "fix the test");
    expect(tauri.calls("agent_prompt")).toEqual([{ target: "w1:p2", text: "fix the test" }]);
    expect(outcome).toEqual({ kind: "started", status: "working" });
  });

  it("passes stalled and timeout results through", async () => {
    tauri.answer("agent_prompt", () => ({ kind: "stalled" }));
    expect(await prompts.send("w1:p2", "x")).toEqual({ kind: "stalled" });
    tauri.answer("agent_prompt", () => ({ kind: "timeout" }));
    expect(await prompts.send("w1:p2", "x")).toEqual({ kind: "timeout" });
  });

  it("turns a rejected call into a failed outcome", async () => {
    tauri.answer("agent_prompt", () => {
      throw "herdr socket: gone";
    });
    expect(await prompts.send("w1:p2", "x")).toEqual({
      kind: "failed",
      message: "herdr socket: gone",
    });
  });
});

describe("promptMessage", () => {
  it("names each outcome", () => {
    expect(promptMessage({ kind: "started", status: "working" })).toBe(
      "Sent. The agent is working.",
    );
    expect(promptMessage({ kind: "started", status: "blocked" })).toBe(
      "Sent. The agent now waits for an answer.",
    );
    expect(promptMessage({ kind: "stalled" })).toContain("stalled");
    expect(promptMessage({ kind: "timeout" })).toContain("Timed out");
    expect(promptMessage({ kind: "blocked" })).toContain("nothing was sent");
    expect(promptMessage({ kind: "failed", message: "boom" })).toBe("boom");
  });

  it("counts only a started prompt as success", () => {
    expect(promptSucceeded({ kind: "started", status: "working" })).toBe(true);
    expect(promptSucceeded({ kind: "stalled" })).toBe(false);
  });
});
