// @vitest-environment jsdom
import { readFileSync } from "node:fs";
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { UI_STORAGE, UiState } from "../core/ui-state";
import { tauri } from "../core/tauri.mock";
import { GithubIssues, type Issue, type RepoIssues } from "./issues";
import {
  GithubMilestones,
  dueText,
  progressPercent,
  readyToClose,
  type Milestone,
  type RepoMilestones,
} from "./milestones";
import { MilestonesSection } from "./milestones-section";

const opener = vi.hoisted(() => ({ openUrl: vi.fn() }));
vi.mock("@tauri-apps/api/core", async () => ({
  invoke: (await import("../core/tauri.mock")).tauri.invoke,
}));
vi.mock("@tauri-apps/api/event", async () => ({
  listen: (await import("../core/tauri.mock")).tauri.listen,
}));
vi.mock("@tauri-apps/plugin-opener", () => opener);
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));

beforeAll(() => {
  getTestBed().initTestEnvironment(BrowserTestingModule, platformBrowserTesting());
});

function ms(number: number, title: string, over: Partial<Milestone> = {}): Milestone {
  return {
    number,
    title,
    url: `https://github.com/o/n/milestone/${number}`,
    dueOn: null,
    open: 0,
    closed: 0,
    current: false,
    ...over,
  };
}

function issue(number: number, milestone: string | null): Issue {
  return {
    number,
    title: `title ${number}`,
    url: `https://github.com/o/n/issues/${number}`,
    state: "OPEN",
    labels: [],
    assignees: [],
    milestone,
    parent: null,
    subTotal: 0,
    subCompleted: 0,
    prs: [],
  };
}

const MILESTONES: RepoMilestones[] = [
  {
    root: "/src/n",
    repo: "o/n",
    status: "ok",
    milestones: [
      ms(1, "1.1.5", { open: 1, closed: 3, current: true, dueOn: "2026-11-20T08:00:00Z" }),
      ms(2, "1.2.0", { open: 3, closed: 0 }),
      ms(3, "Backlog", { open: 0, closed: 2 }),
    ],
  },
  { root: "/src/m", repo: "o/m", status: "error", error: { kind: "not-logged-in" } },
];

const ISSUES: RepoIssues[] = [
  {
    root: "/src/n",
    repo: "o/n",
    status: "ok",
    truncated: false,
    issues: [issue(1, "1.1.5"), issue(2, "1.2.0"), issue(3, null)],
  },
];

describe("milestone helpers", () => {
  it("computes the progress, 0 without issues", () => {
    expect(progressPercent({ open: 1, closed: 3 })).toBe(75);
    expect(progressPercent({ open: 2, closed: 1 })).toBe(33);
    expect(progressPercent({ open: 0, closed: 0 })).toBe(0);
    expect(progressPercent({ open: 0, closed: 4 })).toBe(100);
  });

  it("flags a milestone without open issues as ready to close", () => {
    expect(readyToClose({ open: 0, closed: 2 })).toBe(true);
    expect(readyToClose({ open: 0, closed: 0 })).toBe(false);
    expect(readyToClose({ open: 1, closed: 2 })).toBe(false);
  });

  it("formats the due date", () => {
    expect(dueText("2026-11-20T08:00:00Z")).toBe("Due 2026-11-20");
    expect(dueText(null)).toBe("No due date");
  });
});

describe("MilestonesSection", () => {
  beforeEach(() => {
    tauri.reset();
    opener.openUrl.mockClear();
    TestBed.resetTestingModule();
    TestBed.configureTestingModule({ providers: [{ provide: UI_STORAGE, useValue: null }] });
  });
  afterEach(() => TestBed.resetTestingModule());

  async function render(result: RepoMilestones[] = MILESTONES) {
    tauri.answer("github_milestones", () => result);
    TestBed.overrideComponent(MilestonesSection, {
      set: {
        templateUrl: undefined,
        styleUrl: undefined,
        template: readFileSync("src/app/github/milestones-section.html", "utf8"),
      },
    });
    const fixture = TestBed.createComponent(MilestonesSection);
    await fixture.whenStable();
    fixture.detectChanges();
    return fixture;
  }

  const items = (el: HTMLElement) => [...el.querySelectorAll("li.item")];

  it("lists the milestones in the given order with current, due date, counts and progress", async () => {
    const el: HTMLElement = (await render()).nativeElement;
    expect(tauri.calls("github_milestones")).toEqual([{ dirs: expect.any(Array) }]);
    expect(items(el).map((i) => i.getAttribute("data-title"))).toEqual([
      "1.1.5",
      "1.2.0",
      "Backlog",
    ]);
    const first = items(el)[0];
    expect(first.querySelector("[data-testid=current]")).not.toBeNull();
    expect(items(el)[1].querySelector("[data-testid=current]")).toBeNull();
    expect(first.textContent).toContain("Due 2026-11-20");
    expect(first.querySelector("[data-testid=counts]")!.textContent).toContain("1 open / 3 closed");
    expect(first.querySelector("progress")!.value).toBe(75);
    expect(first.classList.contains("current")).toBe(true);
    expect(first.querySelector("[data-testid=ready]")).toBeNull();
    // 1.2.0 has open issues; Backlog has none open and some closed: dimmed with the hint.
    expect(items(el)[1].classList.contains("dim")).toBe(false);
    expect(items(el)[2].classList.contains("dim")).toBe(true);
    expect(items(el)[2].querySelector("[data-testid=ready]")!.textContent).toBe("ready to close");
    expect(el.querySelector("[data-testid=milestone-error]")!.textContent).toContain("o/m");
  });

  it("hides a repository without open milestones unless empty ones are shown", async () => {
    const fixture = await render([{ root: "/r", repo: "o/n", status: "ok", milestones: [] }]);
    const el: HTMLElement = fixture.nativeElement;
    expect(el.querySelector("[data-testid=milestones-empty]")).toBeNull();
    expect(el.querySelector("h3")).toBeNull();
    TestBed.inject(UiState).setGithubShowEmpty(true);
    fixture.detectChanges();
    expect(el.querySelector("[data-testid=milestones-empty]")).not.toBeNull();
  });

  it("opens a milestone in the browser, GitHub links only", async () => {
    const el: HTMLElement = (await render()).nativeElement;
    el.querySelector<HTMLButtonElement>("[aria-label='Open milestone 1.2.0 in browser']")!.click();
    await TestBed.inject(GithubMilestones).openUrl("https://evil.example/x");
    expect(opener.openUrl.mock.calls.map((c) => c[0])).toEqual([
      "https://github.com/o/n/milestone/2",
    ]);
  });

  it("shows a failed command", async () => {
    tauri.answer("github_milestones", () => {
      throw new Error("boom");
    });
    TestBed.overrideComponent(MilestonesSection, {
      set: {
        templateUrl: undefined,
        styleUrl: undefined,
        template: readFileSync("src/app/github/milestones-section.html", "utf8"),
      },
    });
    const fixture = TestBed.createComponent(MilestonesSection);
    await fixture.whenStable();
    fixture.detectChanges();
    expect(fixture.nativeElement.querySelector("[role=alert]").textContent).toContain("boom");
  });

  it("selecting a milestone filters the issues tree, selecting it again clears the filter", async () => {
    const fixture = await render();
    const el: HTMLElement = fixture.nativeElement;
    tauri.answer("github_issues", () => ISSUES);
    const issues = TestBed.inject(GithubIssues);
    await issues.load();
    const shown = () =>
      issues
        .trees()[0]
        .roots.map((n) => n.issue.number)
        .sort();
    expect(shown()).toEqual([1, 2, 3]);

    const pick = () =>
      el.querySelector<HTMLButtonElement>("[aria-label='Filter issues by milestone 1.2.0']")!;
    pick().click();
    expect(shown()).toEqual([2]);
    fixture.detectChanges();
    expect(pick().getAttribute("aria-pressed")).toBe("true");
    pick().click();
    fixture.detectChanges();
    expect(shown()).toEqual([1, 2, 3]);
    expect(pick().getAttribute("aria-pressed")).toBe("false");
  });

  it("a selection only filters the repository it belongs to, and is dropped when the milestone is gone", async () => {
    await render();
    const milestones = TestBed.inject(GithubMilestones);
    tauri.answer("github_issues", () => [
      ...ISSUES,
      { ...ISSUES[0], root: "/src/k", repo: "o/k" } as RepoIssues,
    ]);
    const issues = TestBed.inject(GithubIssues);
    await issues.load();
    milestones.toggle("o/n", "1.2.0");
    expect(issues.trees().map((t) => t.count)).toEqual([1, 3]);

    tauri.answer("github_milestones", () => [
      { root: "/src/n", repo: "o/n", status: "ok", milestones: [ms(1, "1.1.5")] },
    ]);
    await milestones.load();
    expect(milestones.selected()).toBeNull();
  });
});
