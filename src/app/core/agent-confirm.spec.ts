// @vitest-environment jsdom
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { beforeAll, describe, expect, it, vi } from "vitest";
import { AgentConfirm, toRequest, writeMessage } from "./agent-confirm";
import { NotificationCenter } from "./notification-center";
import { TauriBridge } from "./tauri";

const asked = {
  kind: "agent.confirm_requested",
  data: {
    id: "confirm1",
    caller_name: "lead",
    action: "prompt_agent",
    space_name: "api",
    text: "fix the tests",
  },
};

function setup(invoke = vi.fn().mockResolvedValue(undefined)) {
  TestBed.resetTestingModule();
  TestBed.configureTestingModule({
    providers: [{ provide: TauriBridge, useValue: { invoke, listen: vi.fn() } }],
  });
  return {
    confirm: TestBed.inject(AgentConfirm),
    center: TestBed.inject(NotificationCenter),
    invoke,
  };
}

describe("agent confirmations", () => {
  beforeAll(() => {
    getTestBed().initTestEnvironment(BrowserTestingModule, platformBrowserTesting(), {
      teardown: { destroyAfterEach: true },
    });
  });

  it("reads a request and ignores one that lacks what the dialog shows", () => {
    expect(toRequest(asked.data as never)).toEqual({
      id: "confirm1",
      callerName: "lead",
      action: "prompt_agent",
      spaceName: "api",
      text: "fix the tests",
    });
    expect(toRequest({ id: "x" })).toBeNull();
    expect(toRequest(null)).toBeNull();
  });

  it("shows the oldest question and sends the answer for it", async () => {
    const { confirm, invoke } = setup();
    confirm.handle(asked as never);
    confirm.handle({ ...asked, data: { ...asked.data, id: "confirm2" } } as never);
    expect(confirm.current()?.id).toBe("confirm1");
    expect(confirm.waiting()).toBe(2);
    await confirm.answer(true);
    expect(invoke).toHaveBeenCalledWith("agent_confirm_answer", { id: "confirm1", approve: true });
    expect(confirm.current()?.id).toBe("confirm2");
    await confirm.answer(false);
    expect(invoke).toHaveBeenLastCalledWith("agent_confirm_answer", {
      id: "confirm2",
      approve: false,
    });
    expect(confirm.current()).toBeNull();
  });

  it("counts a repeated request once and drops one the server settled", () => {
    const { confirm } = setup();
    confirm.handle(asked as never);
    confirm.handle(asked as never);
    expect(confirm.waiting()).toBe(1);
    confirm.handle({ kind: "agent.confirm_resolved", data: { id: "confirm1" } } as never);
    expect(confirm.current()).toBeNull();
  });

  it("keeps the error of an answer that came too late and still closes the question", async () => {
    const { confirm } = setup(vi.fn().mockRejectedValue("confirmation c1 not found"));
    confirm.handle(asked as never);
    await confirm.answer(true);
    expect(confirm.error()).toContain("not found");
    expect(confirm.current()).toBeNull();
  });

  it("lists every write in the notification center with the calling terminal", () => {
    const { confirm, center } = setup();
    confirm.handle(
      {
        kind: "agent.wrote",
        data: {
          caller_name: "lead",
          action: "start_agent",
          space_name: "web",
          text: "claude",
          confirmed: true,
        },
      } as never,
      5,
    );
    const [entry] = center.entries();
    expect(entry).toMatchObject({
      label: "lead",
      message: "lead started an agent in web (you allowed it)",
      detail: "claude",
      time: 5,
      read: false,
    });
    confirm.handle(
      { kind: "terminal.resume_failed", data: { terminal_id: "t1", reason: "no report" } } as never,
      6,
    );
    expect(center.entries()[0]).toMatchObject({
      label: "Hibernated agent",
      message: "Resume failed: no report",
      status: "error",
      time: 6,
    });
    expect(writeMessage("lead", "prompt_agent", "web", false)).toBe(
      "lead prompted an agent in web",
    );
    expect(writeMessage("lead", "start_shell", "web", true)).toBe(
      "lead started a command in web (you allowed it)",
    );
    expect(writeMessage("lead", "stop_terminal", "web", false)).toBe(
      "lead stopped a terminal in web",
    );
  });

  it("ignores other events", () => {
    const { confirm, center } = setup();
    confirm.handle({ kind: "space.created", data: null });
    expect(confirm.current()).toBeNull();
    expect(center.entries()).toEqual([]);
  });
});
