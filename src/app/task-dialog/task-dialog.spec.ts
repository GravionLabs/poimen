// @vitest-environment jsdom
import { readFileSync } from "node:fs";
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { tauri } from "../core/tauri.mock";
import { TaskDialog } from "./task-dialog";
import { TaskPicker, filterIssues, markers, type PickIssue, type TaskChoice } from "./task-picker";

vi.mock("@tauri-apps/api/core", async () => ({
  invoke: (await import("../core/tauri.mock")).tauri.invoke,
}));
vi.mock("@tauri-apps/api/event", async () => ({
  listen: (await import("../core/tauri.mock")).tauri.listen,
}));

function issue(number: number, title: string, over: Partial<PickIssue> = {}): PickIssue {
  return {
    number,
    title,
    level: null,
    labels: [],
    assignees: [],
    url: `https://github.com/o/r/issues/${number}`,
    ...over,
  };
}

const ISSUES = [
  issue(12, "Add picker", {
    level: "pbi",
    labels: ["pbi", "needs-refinement"],
    assignees: ["octo"],
  }),
  issue(10, "Tasks epic", { level: "epic", labels: ["epic"] }),
  issue(7, "Crash on start", { level: "bug", labels: ["bug", "needs-decision"] }),
  issue(3, "Typo"),
];
const MODES = {
  modes: [
    {
      name: "agent",
      worktree: true,
      harness: null,
      model: null,
      prompt: null,
      command: null,
      branch: "b",
      cleanup: null,
    },
    {
      name: "plain",
      worktree: false,
      harness: null,
      model: null,
      prompt: null,
      command: ["sh"],
      branch: "b",
      cleanup: null,
    },
  ],
  worktreeDir: "..",
};

beforeAll(() => {
  getTestBed().initTestEnvironment(BrowserTestingModule, platformBrowserTesting(), {
    teardown: { destroyAfterEach: true },
  });
});

async function setup(issuesAnswer: () => unknown = () => ISSUES) {
  tauri.reset();
  tauri.answer("task_issues", issuesAnswer);
  tauri.answer("get_task_modes", () => MODES);
  TestBed.resetTestingModule();
  TestBed.overrideComponent(TaskDialog, {
    set: {
      templateUrl: undefined,
      styleUrl: undefined,
      template: readFileSync("src/app/task-dialog/task-dialog.html", "utf8"),
    },
  });
  const picker = TestBed.inject(TaskPicker);
  const fixture = TestBed.createComponent(TaskDialog);
  const root = fixture.nativeElement as HTMLElement;
  const settle = async () => {
    for (let i = 0; i < 3; i++) {
      fixture.detectChanges();
      await fixture.whenStable();
    }
  };
  let result: TaskChoice | null | undefined;
  const opened = picker.open("/work/app").then((c) => (result = c));
  await vi.waitFor(() => expect(picker.loading()).toBe(false));
  await settle();
  const key = async (k: string, target?: Element) => {
    (target ?? root.querySelector(".dialog")!).dispatchEvent(
      new KeyboardEvent("keydown", { key: k, bubbles: true, cancelable: true }),
    );
    await settle();
  };
  const type = async (text: string) => {
    const input = root.querySelector<HTMLInputElement>("input")!;
    input.value = text;
    input.dispatchEvent(new Event("input", { bubbles: true }));
    await settle();
  };
  const rows = () => [...root.querySelectorAll(".issue")].map((r) => r.textContent!.trim());
  return { picker, root, key, type, rows, opened, result: () => result, settle };
}

describe("filterIssues and markers", () => {
  it("matches title, labels and the number with or without #", () => {
    expect(filterIssues(ISSUES, "crash", null).map((i) => i.number)).toEqual([7]);
    expect(filterIssues(ISSUES, "#12", null).map((i) => i.number)).toEqual([12]);
    expect(filterIssues(ISSUES, "needs-", null).map((i) => i.number)).toEqual([12, 7]);
    expect(filterIssues(ISSUES, "", "epic").map((i) => i.number)).toEqual([10]);
    expect(filterIssues(ISSUES, "zzz", null)).toEqual([]);
  });

  it("lists the needs markers and assignees", () => {
    expect(markers(ISSUES[0])).toEqual(["[needs-refinement]", "[@octo]"]);
    expect(markers(ISSUES[3])).toEqual([]);
  });
});

describe("TaskDialog", () => {
  beforeEach(() => tauri.reset());

  it("asks for the issues and modes of the directory and shows markers without hiding", async () => {
    const { rows } = await setup();
    expect(tauri.calls("task_issues")).toEqual([{ dir: "/work/app" }]);
    expect(tauri.calls("get_task_modes")).toEqual([{ dir: "/work/app" }]);
    expect(rows()).toHaveLength(4);
    expect(rows()[0]).toContain("[needs-refinement]");
    expect(rows()[0]).toContain("[@octo]");
    expect(rows()[2]).toContain("[needs-decision]");
  });

  it("filters by text and by level", async () => {
    const { rows, type, root, settle, picker } = await setup();
    await type("7");
    expect(rows()).toHaveLength(1);
    expect(rows()[0]).toContain("Crash on start");
    await type("");
    root.querySelectorAll<HTMLButtonElement>(".level")[1].click(); // feature: none
    await settle();
    expect(rows()).toEqual([]);
    root.querySelectorAll<HTMLButtonElement>(".level")[0].click(); // epic
    await settle();
    expect(picker.level()).toBe("epic");
    expect(rows()).toHaveLength(1);
  });

  it("moves with Up/Down (wrapping) and Enter returns issue and mode", async () => {
    const { key, opened, result } = await setup();
    await key("ArrowDown");
    await key("ArrowDown");
    await key("ArrowUp");
    await key("Enter");
    await opened;
    expect(result()).toEqual({
      number: 10,
      title: "Tasks epic",
      url: "https://github.com/o/r/issues/10",
      mode: "agent",
    });
  });

  it("wraps from the first row to the last with Up", async () => {
    const { key, opened, result } = await setup();
    await key("ArrowUp");
    await key("Enter");
    await opened;
    expect(result()?.number).toBe(3);
  });

  it("Tab switches the mode and Escape cancels", async () => {
    const { key, opened, result, root, picker } = await setup();
    await key("Tab");
    expect(picker.modeName()).toBe("plain");
    expect(root.querySelector(".mode")!.textContent).toContain("plain");
    await key("Escape");
    await opened;
    expect(result()).toBeNull();
    expect(root.querySelector(".dialog")).toBeNull();
  });

  it("a failed list shows the reason and still accepts a typed number", async () => {
    const { root, type, key, opened, result } = await setup(() => {
      throw { kind: "not-logged-in" };
    });
    expect(root.querySelector(".error")!.textContent).toContain("gh auth login");
    await type("#42");
    expect(root.textContent).toContain("Enter picks issue #42");
    await key("Enter");
    await opened;
    expect(result()).toEqual({ number: 42, title: null, url: null, mode: "agent" });
  });

  it("Enter without a match or number returns nothing", async () => {
    const { type, key, picker } = await setup();
    await type("zzz");
    await key("Enter");
    expect(picker.isOpen()).toBe(true);
  });

  it("a number that a listed issue has is not offered as typed", async () => {
    const { picker, type } = await setup();
    await type("12");
    expect(picker.typedNumber()).toBeNull();
  });
});
