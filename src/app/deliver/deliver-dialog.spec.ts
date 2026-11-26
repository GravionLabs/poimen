// @vitest-environment jsdom
import { readFileSync } from "node:fs";
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { SpaceActions } from "../core/space-actions";
import { SpacesStore } from "../core/spaces-store";
import { EditorTabs } from "../core/editor-tabs";
import { tauri } from "../core/tauri.mock";
import { expectFocusInside, focusOpener, press } from "../ui/dialog-shell/dialog-contract";
import { Deliver } from "./deliver";
import { DeliverDialog } from "./deliver-dialog";
import { DeliverLauncher } from "./deliver-launcher";
import type { DeliverPlan } from "./deliver-start";
import { choose, optionLabels, shown } from "../../testing/select";

vi.mock("@tauri-apps/api/core", async () => ({
  invoke: (await import("../core/tauri.mock")).tauri.invoke,
}));
vi.mock("@tauri-apps/api/event", async () => ({
  listen: (await import("../core/tauri.mock")).tauri.listen,
}));
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));

beforeAll(() => {
  try {
    getTestBed().initTestEnvironment(BrowserTestingModule, platformBrowserTesting());
  } catch {
    // already initialized by the shared setup
  }
});

const PLAN: DeliverPlan = {
  available: true,
  base: null,
  command: "deliver-loop --milestone M016",
  error: null,
  tab: "deliver M016",
};
const MILESTONES = [
  {
    root: "/work/app",
    repo: "o/app",
    status: "ok",
    milestones: [{ title: "M017 (0.35.0)" }, { title: "M016 (0.34.0)" }, { title: "Ideas" }],
  },
];
const TRUST = {
  root: "/work/app",
  files: ["/work/app/.poimen/config.json"],
  hash: "h1",
  commands: [{ what: "setup", command: "pnpm install" }],
};

async function setup(plan: (args?: Record<string, unknown>) => DeliverPlan = () => PLAN) {
  tauri.reset();
  tauri.answer("deliver_plan", plan);
  tauri.answer("github_milestones", () => MILESTONES);
  tauri.answer("create_space", () => ({ space_id: "s1", created: false }));
  tauri.answer("deliver_start", () => ({ kind: "started", terminal_id: "t9" }));
  TestBed.resetTestingModule();
  TestBed.overrideComponent(DeliverDialog, {
    set: {
      templateUrl: undefined,
      styleUrl: undefined,
      template: readFileSync("src/app/deliver/deliver-dialog.html", "utf8"),
    },
  });
  // The terminal a start opens exists in the snapshot.
  vi.spyOn(TestBed.inject(SpacesStore), "all").mockReturnValue({
    spaces: [],
    tabs: [],
    terminals: [{ terminal_id: "t9" }],
  } as never);
  const launcher = TestBed.inject(DeliverLauncher);
  const fixture = TestBed.createComponent(DeliverDialog);
  const root = fixture.nativeElement as HTMLElement;
  const settle = async () => {
    for (let i = 0; i < 4; i++) {
      fixture.detectChanges();
      await fixture.whenStable();
    }
  };
  const field = async (testId: string, value: string) => {
    const el = root.querySelector<HTMLElement>(`[data-testid=${testId}]`)!;
    if (el instanceof HTMLInputElement) {
      el.value = value;
      el.dispatchEvent(new Event("input"));
      await settle();
    } else {
      await choose(el, value, settle);
      expect(shown(el)).toBe(value);
    }
  };
  const click = async (testId: string) => {
    root.querySelector<HTMLButtonElement>(`[data-testid=${testId}]`)!.click();
    await settle();
  };
  const text = (testId: string) =>
    root.querySelector(`[data-testid=${testId}]`)?.textContent?.trim() ?? null;
  return { launcher, root, settle, field, click, text };
}

describe("the Deliver dialog (#746)", () => {
  beforeEach(() => tauri.reset());

  it("takes the focus, Escape closes it and the focus goes back", async () => {
    const opener = focusOpener();
    const { launcher, root, settle } = await setup();
    await launcher.openMilestone("/work/app", "M016");
    await settle();
    expectFocusInside(root);
    press("Escape");
    await settle();
    expect(launcher.opened()).toBeNull();
    expect(document.activeElement).toBe(opener);
  });

  it("shows the target, the loop's defaults, the base and the exact command", async () => {
    const { launcher, root, settle, text } = await setup();
    await launcher.openMilestone("/work/app", "M016");
    await settle();
    const milestone = root.querySelector("[data-testid=milestone]")!;
    expect(await optionLabels(milestone, settle)).toEqual(["M016", "M017"]);
    expect(shown(milestone)).toBe("M016");
    expect(text("base")).toBe("Base branch: loop default");
    expect(text("command")).toBe("deliver-loop --milestone M016");
    const labels = [...root.querySelectorAll("button")].map((b) => b.textContent!.trim());
    // Cancel first, the one primary last.
    expect(labels).toContain("Cancel");
    expect(labels.at(-1)).toBe("Start loop");
  });

  it("shows deliver.base when the repository sets it", async () => {
    const { launcher, settle, text } = await setup(() => ({ ...PLAN, base: "release/next" }));
    await launcher.openMilestone("/work/app", "M016");
    await settle();
    expect(text("base")).toBe("Base branch: release/next");
  });

  it("sends the validated values of the form to the plan and to the start", async () => {
    const { launcher, field, click, settle } = await setup();
    await launcher.openMilestone("/work/app", "M016");
    await settle();
    await field("harness", "copilot");
    await field("model", "my model");
    await field("max", "2");
    expect(tauri.calls("deliver_plan").at(-1)).toMatchObject({
      dir: "/work/app",
      request: {
        target: { kind: "milestone", name: "M016" },
        harness: "copilot",
        model: "my model",
        max: 2,
      },
    });
    await click("start");
    await vi.waitFor(() => expect(tauri.calls("deliver_start")).toHaveLength(1));
    expect(tauri.calls("deliver_start")[0]).toMatchObject({
      spaceId: "s1",
      dir: "/work/app",
      decision: null,
      request: { harness: "copilot", model: "my model", max: 2 },
    });
    expect(launcher.opened()).toBeNull();
    expect(TestBed.inject(Deliver).activeLoop("/work/app")).toEqual({
      kind: "pending",
      terminalId: "t9",
    });
  });

  it("does not start with a bad --max", async () => {
    const { launcher, field, root, settle, text } = await setup();
    await launcher.openMilestone("/work/app", "M016");
    await settle();
    await field("max", "0");
    expect(text("invalid")).toContain("--max");
    expect(root.querySelector<HTMLButtonElement>("[data-testid=start]")!.disabled).toBe(true);
    expect(tauri.calls("deliver_start")).toHaveLength(0);
  });

  it("starts for the marked issues", async () => {
    const { launcher, click, settle, root } = await setup(() => ({
      ...PLAN,
      command: "deliver-loop --issues 12,15",
    }));
    launcher.openIssues("/work/app", [12, 15]);
    await settle();
    expect(root.querySelector<HTMLInputElement>("[data-testid=issues]")!.value).toBe("#12, #15");
    await click("start");
    await vi.waitFor(() => expect(tauri.calls("deliver_start")).toHaveLength(1));
    expect(tauri.calls("deliver_start")[0]).toMatchObject({
      request: { target: { kind: "issues", numbers: [12, 15] } },
    });
  });

  it("says what is missing and starts nothing without deliver-loop", async () => {
    const { launcher, root, settle, text } = await setup(() => ({ ...PLAN, available: false }));
    await launcher.openMilestone("/work/app", "M016");
    await settle();
    expect(text("missing")).toContain("/skills-manage delivery");
    expect(root.querySelector("[data-testid=start]")).toBeNull();
    expect(tauri.calls("deliver_start")).toHaveLength(0);
  });

  it("refuses a second start while a loop runs and points to the run", async () => {
    const { launcher, root, settle, click, text } = await setup();
    const deliver = TestBed.inject(Deliver);
    vi.spyOn(deliver, "activeLoop").mockReturnValue({ kind: "running" });
    const reveal = vi.spyOn(deliver, "reveal").mockReturnValue(true);
    await launcher.openMilestone("/work/app", "M016");
    await settle();
    expect(text("refusal")).toContain("already running");
    expect(root.querySelector("[data-testid=start]")).toBeNull();
    await click("show-run");
    expect(reveal).toHaveBeenCalledWith("/work/app");
    expect(launcher.opened()).toBeNull();
    // Not even a direct call starts it.
    expect(await launcher.start("/work/app", { target: { kind: "milestone", name: "M016" } })).toBe(
      false,
    );
    expect(tauri.calls("deliver_start")).toHaveLength(0);
  });

  it("refuses a start whose terminal exists and whose run file has not appeared", async () => {
    const { launcher, settle, text } = await setup();
    const deliver = TestBed.inject(Deliver);
    vi.spyOn(TestBed.inject(SpacesStore), "all").mockReturnValue({
      spaces: [],
      tabs: [],
      terminals: [{ terminal_id: "t9" }],
    } as never);
    deliver.markLaunched("/work/app", "t9");
    await launcher.openMilestone("/work/app", "M016");
    await settle();
    expect(text("refusal")).toContain("just started");
  });

  it("opens no start when the repository has no open milestone", async () => {
    const { launcher } = await setup();
    tauri.answer("github_milestones", () => []);
    expect(await launcher.openMilestone("/work/app")).toBe(false);
    expect(launcher.opened()).toBeNull();
  });
});

describe("the trust path of a loop start (#746)", () => {
  const request = { target: { kind: "milestone", name: "M016" } } as const;

  it("asks first for an untrusted project and starts only after the answer", async () => {
    const { launcher } = await setup();
    tauri.answer("deliver_start", (args) =>
      args?.["decision"] ? { kind: "started", terminal_id: "t9" } : { kind: "ask", request: TRUST },
    );
    await launcher.start("/work/app", request);
    await vi.waitFor(() => expect(TestBed.inject(SpaceActions).trust()).not.toBeNull());
    expect(tauri.calls("deliver_start")).toHaveLength(1);
    expect(tauri.calls("deliver_start")[0]).toMatchObject({ decision: null });
    expect(TestBed.inject(Deliver).activeLoop("/work/app")).toBeUndefined();
    await TestBed.inject(SpaceActions).answerTrust("trust");
    expect(tauri.calls("deliver_start")).toHaveLength(2);
    expect(tauri.calls("deliver_start")[1]).toMatchObject({
      decision: { kind: "trust", hash: "h1" },
    });
    expect(TestBed.inject(Deliver).activeLoop("/work/app")).toMatchObject({ kind: "pending" });
  });

  it("starts nothing when the trust is cancelled", async () => {
    const { launcher } = await setup();
    tauri.answer("deliver_start", () => ({ kind: "ask", request: TRUST }));
    await launcher.start("/work/app", request);
    await vi.waitFor(() => expect(TestBed.inject(SpaceActions).trust()).not.toBeNull());
    await TestBed.inject(SpaceActions).answerTrust("cancel");
    expect(tauri.calls("deliver_start")).toHaveLength(1);
    expect(TestBed.inject(Deliver).activeLoop("/work/app")).toBeUndefined();
  });

  it("asks for a new space's trust through create_space before the loop", async () => {
    const { launcher } = await setup();
    tauri.answer("create_space", (args) =>
      args?.["decision"]
        ? { space_id: "s1", created: true }
        : { space_id: "", created: false, trust: TRUST },
    );
    await launcher.start("/work/app", request);
    await vi.waitFor(() => expect(TestBed.inject(SpaceActions).trust()).not.toBeNull());
    expect(tauri.calls("deliver_start")).toHaveLength(0);
    await TestBed.inject(SpaceActions).answerTrust("trust");
    await vi.waitFor(() => expect(tauri.calls("deliver_start")).toHaveLength(1));
  });

  it("notes a missing command instead of starting", async () => {
    const { launcher } = await setup();
    tauri.answer("deliver_start", () => ({ kind: "missing" }));
    const notify = vi.spyOn(TestBed.inject(SpaceActions), "notify");
    await launcher.start("/work/app", request);
    await vi.waitFor(() => expect(notify).toHaveBeenCalled());
    expect(notify.mock.calls[0]![0]).toContain("/skills-manage delivery");
    expect(TestBed.inject(Deliver).activeLoop("/work/app")).toBeUndefined();
  });

  it("opens the Runs view for Show run", async () => {
    const { launcher } = await setup();
    launcher.showRun("/work/app");
    expect(TestBed.inject(EditorTabs).tabs().length).toBeGreaterThan(0);
  });
});
