// @vitest-environment jsdom
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { beforeAll, describe, expect, it, vi } from "vitest";
import { AgentGrants, durationText, toActiveGrant, toGrantRequest } from "./agent-grants";
import { TauriBridge } from "./tauri";

const asked = {
  kind: "agent.grant_requested",
  data: {
    id: "grant1",
    lead_name: "lead",
    plan: "ship M016",
    spaces: [
      { id: "s1", name: "api", path: "/w/api" },
      { id: "s2", name: "web" },
    ],
    tools: ["start_agent", "prompt_agent"],
    max_agents: 3,
    duration_s: 28800,
  },
};

const active = {
  id: "grant1",
  lead_name: "lead",
  spaces: [{ id: "s1", name: "api" }],
  tools: ["start_agent"],
  max_agents: 3,
  running: 1,
  expires_at: 1_700_000_000_000,
};

function setup(invoke = vi.fn().mockResolvedValue(undefined)) {
  TestBed.resetTestingModule();
  TestBed.configureTestingModule({
    providers: [{ provide: TauriBridge, useValue: { invoke, listen: vi.fn() } }],
  });
  return { grants: TestBed.inject(AgentGrants), invoke };
}

describe("agent grants", () => {
  beforeAll(() => {
    getTestBed().initTestEnvironment(BrowserTestingModule, platformBrowserTesting(), {
      teardown: { destroyAfterEach: true },
    });
  });

  it("reads a request and ignores one that lacks what the dialog shows", () => {
    expect(toGrantRequest(asked.data as never)).toEqual({
      id: "grant1",
      leadName: "lead",
      plan: "ship M016",
      spaces: asked.data.spaces,
      tools: ["start_agent", "prompt_agent"],
      maxAgents: 3,
      durationS: 28800,
    });
    expect(toGrantRequest({ id: "x", lead_name: "lead", tools: ["start_agent"] })).toBeNull();
    expect(toGrantRequest(null)).toBeNull();
    expect(toActiveGrant({})).toBeNull();
    expect(toActiveGrant(active as never)?.running).toBe(1);
  });

  it("names the duration", () => {
    expect(durationText(8 * 3600)).toBe("8 hours");
    expect(durationText(3600)).toBe("1 hour");
    expect(durationText(5400)).toBe("1.5 hours");
    expect(durationText(1800)).toBe("30 minutes");
  });

  it("shows the oldest request and a resolved one disappears", () => {
    const { grants } = setup();
    grants.handle(asked as never);
    grants.handle({ ...asked, data: { ...asked.data, id: "grant2" } } as never);
    grants.handle(asked as never);
    expect(grants.current()?.id).toBe("grant1");
    expect(grants.waiting()).toBe(2);
    grants.handle({ kind: "agent.grant_resolved", data: { id: "grant1" } } as never);
    expect(grants.current()?.id).toBe("grant2");
  });

  it("answers no, or yes with the narrowed spaces and limit", async () => {
    const { grants, invoke } = setup();
    grants.handle(asked as never);
    await grants.answer(true, ["s1"], 2);
    expect(invoke).toHaveBeenCalledWith("agent_grant_answer", {
      id: "grant1",
      approve: true,
      spaces: ["s1"],
      maxAgents: 2,
    });
    expect(grants.current()).toBeNull();
    grants.handle(asked as never);
    await grants.answer(false, ["s1"], 2);
    expect(invoke).toHaveBeenLastCalledWith("agent_grant_answer", {
      id: "grant1",
      approve: false,
      spaces: undefined,
      maxAgents: undefined,
    });
  });

  it("keeps the request open and shows the reason when the server refuses a yes", async () => {
    const { grants } = setup(vi.fn().mockRejectedValue("space s9 was not asked for"));
    grants.handle(asked as never);
    await grants.answer(true);
    expect(grants.current()?.id).toBe("grant1");
    expect(grants.error()).toBe("space s9 was not asked for");
    await grants.answer(false);
    expect(grants.current()).toBeNull();
  });

  it("lists the active grants from the first read and from every change, and revokes one", async () => {
    const invoke = vi.fn(async (command: string) =>
      command === "agent_grants" ? { grants: [active] } : undefined,
    );
    const { grants } = setup(invoke as never);
    grants.start();
    await vi.waitFor(() => expect(grants.active()).toHaveLength(1));
    expect(grants.active()[0]).toMatchObject({ leadName: "lead", maxAgents: 3, running: 1 });
    grants.handle({ kind: "agent.grants_changed", data: { grants: [] } } as never);
    expect(grants.active()).toEqual([]);
    await grants.revoke("grant1");
    expect(invoke).toHaveBeenCalledWith("agent_grant_revoke", { id: "grant1" });
  });
});
