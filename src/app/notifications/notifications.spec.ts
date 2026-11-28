// @vitest-environment jsdom
import { readFileSync } from "node:fs";
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { signal } from "@angular/core";
import { beforeAll, describe, expect, it, vi } from "vitest";
import { NotificationCenter, type CheckResult } from "../core/notification-center";
import { AgentGrants, type ActiveGrant } from "../core/agent-grants";
import { PrWatch } from "../core/pr-watch";
import { Notifications } from "./notifications";

describe("Notifications", () => {
  beforeAll(() => {
    getTestBed().initTestEnvironment(BrowserTestingModule, platformBrowserTesting());
  });

  const setup = (watch?: Pick<PrWatch, "fixChecks">) => {
    TestBed.resetTestingModule();
    if (watch) TestBed.overrideProvider(PrWatch, { useValue: watch });
    TestBed.overrideComponent(Notifications, {
      set: {
        templateUrl: undefined,
        styleUrl: undefined,
        template: readFileSync("src/app/notifications/notifications.html", "utf8"),
      },
    });
    const fixture = TestBed.createComponent(Notifications);
    const center = TestBed.inject(NotificationCenter);
    const chosen: string[] = [];
    fixture.componentInstance.paneChosen.subscribe((id) => chosen.push(id));
    return { fixture, center, chosen, el: fixture.nativeElement as HTMLElement };
  };

  it("shows a hint while empty", async () => {
    const { fixture, el } = setup();
    await fixture.whenStable();
    expect(el.textContent).toContain("No notifications yet");
  });

  it("lists entries newest first", async () => {
    const { fixture, center, el } = setup();
    center.record([{ paneId: "p1", status: "done" }], () => "one", 1);
    center.record([{ paneId: "p2", status: "blocked" }], () => "two", 2);
    await fixture.whenStable();
    const messages = [...el.querySelectorAll(".message")].map((m) => m.textContent);
    expect(messages).toEqual(["two is waiting for you", "one is done"]);
  });

  it("a click marks the entry read and chooses its pane", async () => {
    const { fixture, center, chosen, el } = setup();
    center.record([{ paneId: "p1", status: "done" }], () => "one", 1);
    await fixture.whenStable();
    el.querySelector<HTMLButtonElement>(".entry")!.click();
    expect(chosen).toEqual(["p1"]);
    expect(center.unread()).toBe(0);
  });

  it("mark all read clears the unread count", async () => {
    const { fixture, center, el } = setup();
    center.record([{ paneId: "p1", status: "done" }], () => "one", 1);
    await fixture.whenStable();
    el.querySelector<HTMLButtonElement>(".mark")!.click();
    expect(center.unread()).toBe(0);
  });

  it("a click on a run entry reveals its repository instead of a pane", async () => {
    const { fixture, center, chosen, el } = setup();
    const runs: { root: string; machine?: string }[] = [];
    fixture.componentInstance.runChosen.subscribe((r) => runs.push(r));
    center.recordRuns([{ root: "/r", label: "M1", message: "M1: merge" }], 1);
    await fixture.whenStable();
    el.querySelector<HTMLButtonElement>(".entry")!.click();
    expect(runs).toEqual([{ root: "/r" }]);
    expect(chosen).toEqual([]);
    expect(center.unread()).toBe(0);
  });

  it("a failed check result offers fix-checks, a passed one does not (#995)", async () => {
    const fixChecks = vi.fn((r: CheckResult) => Promise.resolve(void r.number));
    const { fixture, center, el } = setup({ fixChecks });
    const result = (passed: boolean): CheckResult => ({
      passed,
      number: 12,
      title: "T",
      url: "u",
      sha: "a",
      label: "space",
      paneId: "t1",
      dir: "/a",
      branch: "feat/a",
      base: "main",
      crossRepo: false,
    });
    center.recordChecks([result(true)], 1);
    await fixture.whenStable();
    expect(el.querySelector(".fix button")).toBeNull();
    center.recordChecks([result(false)], 2);
    await fixture.whenStable();
    const buttons = el.querySelectorAll<HTMLButtonElement>(".fix button");
    expect(buttons).toHaveLength(1);
    buttons[0].click();
    expect(fixChecks).toHaveBeenCalledWith(expect.objectContaining({ passed: false, number: 12 }));
    expect(center.entries()[0].read).toBe(true);
  });

  it("lists the active write grants and revokes one", async () => {
    const grant: ActiveGrant = {
      id: "g1",
      leadName: "lead",
      spaces: [
        { id: "s1", name: "api" },
        { id: "s2", name: "web" },
      ],
      tools: ["start_agent"],
      maxAgents: 3,
      running: 1,
      worktrees: false,
      expiresAt: Date.now() + 3600_000,
    };
    const grants = { active: signal([grant]), revoke: vi.fn() };
    TestBed.resetTestingModule();
    TestBed.overrideProvider(AgentGrants, { useValue: grants });
    TestBed.overrideComponent(Notifications, {
      set: {
        templateUrl: undefined,
        styleUrl: undefined,
        template: readFileSync("src/app/notifications/notifications.html", "utf8"),
      },
    });
    const fixture = TestBed.createComponent(Notifications);
    await fixture.whenStable();
    const el = fixture.nativeElement as HTMLElement;
    expect(el.querySelector(".grant .message")?.textContent).toContain(
      "lead may write in api, web",
    );
    expect(el.querySelector(".grant .message")?.textContent).not.toContain("worktrees");
    expect(el.querySelector(".grant .meta")?.textContent).toContain("1 of 3 agents running");
    el.querySelector<HTMLButtonElement>(".revoke")!.click();
    expect(grants.revoke).toHaveBeenCalledWith("g1");
    grants.active.set([{ ...grant, worktrees: true }]);
    await fixture.whenStable();
    expect(el.querySelector(".grant .message")?.textContent?.replace(/\s+/g, " ")).toContain(
      "lead may write in api, web and start agents in new worktrees of them",
    );
  });
});
