// @vitest-environment jsdom
import { readFileSync } from "node:fs";
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { Machines } from "../core/machines";
import { SpacesStore } from "../core/spaces-store";
import type { Terminal } from "../core/snapshot";
import { tauri } from "../core/tauri.mock";
import { UI_STORAGE } from "../core/ui-state";
import { Deliver } from "./deliver";
import { DeliverView } from "./deliver-view";
import type { DeliverRead } from "./deliver-status";

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

beforeEach(() => {
  tauri.reset();
});

const events = (name: string): Extract<DeliverRead, { kind: "events" }> => ({
  kind: "events",
  runs: [JSON.parse(readFileSync(`src/app/deliver/fixtures/events-${name}.json`, "utf8"))],
});
/** Two minutes after the last event of the running milestone run. */
const AT = Date.parse("2026-11-08T12:00:00Z");
const text = (el: Element | null) => el?.textContent?.replace(/\s+/g, " ").trim() ?? "";

async function render() {
  TestBed.resetTestingModule();
  TestBed.configureTestingModule({
    providers: [
      { provide: UI_STORAGE, useValue: { getItem: () => null, setItem: () => undefined } },
    ],
  });
  TestBed.overrideComponent(DeliverView, {
    set: {
      templateUrl: undefined,
      styleUrl: undefined,
      template: readFileSync("src/app/deliver/deliver-view.html", "utf8"),
    },
  });
  const fixture = TestBed.createComponent(DeliverView);
  const deliver = TestBed.inject(Deliver);
  const el = fixture.nativeElement as HTMLElement;
  const show = async () => {
    await fixture.whenStable();
    fixture.detectChanges();
  };
  await show();
  return { fixture, deliver, el, show };
}

describe("Runs view: show terminal of a task (#745)", () => {
  const RUN = "20261108-100000-loop";
  const term = (id: string, extra: Partial<Terminal> = {}): Terminal => ({
    terminal_id: id,
    tab_id: "tab",
    space_id: "s",
    agent_status: "working",
    ...extra,
  });
  const withTerminals = (terminals: Terminal[]) =>
    vi
      .spyOn(TestBed.inject(SpacesStore), "all")
      .mockReturnValue({ spaces: [], tabs: [], terminals });
  const buttons = (el: HTMLElement) => [
    ...el.querySelectorAll<HTMLButtonElement>("[data-testid=task-terminal]"),
  ];

  async function runningView(terminals: Terminal[]) {
    const view = await render();
    withTerminals(terminals);
    const chosen: string[] = [];
    view.fixture.componentInstance.paneChosen.subscribe((id) => chosen.push(id));
    view.deliver.apply("/r", events("milestone"));
    view.deliver.now.set(AT);
    await view.show();
    return { ...view, chosen };
  }

  it("shows the action on a task with a terminal of its run and focuses it", async () => {
    const { el, chosen, show } = await runningView([
      term("t3", { run: RUN, task: "t12" }),
      term("t4", { run: RUN, task: "other" }),
    ]);
    const current = el.querySelector("[data-testid=current]")!;
    const button = current.querySelector<HTMLButtonElement>("[data-testid=task-terminal]")!;
    expect(text(button)).toBe("Show terminal");
    button.click();
    await show();
    expect(chosen).toEqual(["t3"]);
    expect(el.querySelector("app-context-menu")).toBeNull();
    // t13 has no terminal: one action in the whole view for t12 only.
    expect(
      buttons(el).every((b) => !b.closest("[data-testid=task]")?.textContent?.includes("t13")),
    ).toBe(true);
  });

  it("shows no action for a session poimen did not start or a terminal of another run", async () => {
    const { el } = await runningView([
      term("t3"),
      term("t4", { run: "another-run", task: "t12" }),
      term("t5", { label: `${RUN} t12` }),
    ]);
    expect(buttons(el)).toHaveLength(0);
  });

  it("drops the action when its terminal is gone, and keeps it while the terminal exists ended", async () => {
    const { el, show } = await runningView([
      term("t3", { run: RUN, task: "t12", agent_status: "done", ended: true }),
    ]);
    expect(buttons(el).length).toBeGreaterThan(0);
    withTerminals([]);
    TestBed.inject(Deliver).now.set(AT + 1);
    await show();
    expect(buttons(el)).toHaveLength(0);
  });

  it("becomes a menu of the terminals, newest first, for a retried task", async () => {
    const { el, chosen, show } = await runningView([
      term("t3", { run: RUN, task: "t12", agent_status: "error", label: "first" }),
      term("t9", { run: RUN, task: "t12", label: "retry" }),
    ]);
    const button = el.querySelector<HTMLButtonElement>(
      "[data-testid=current] [data-testid=task-terminal]",
    )!;
    expect(text(button)).toBe("Show terminal (2)");
    button.click();
    await show();
    const entries = [...document.querySelectorAll("[data-testid=task-terminal-entry]")];
    expect(entries.map((e) => text(e))).toEqual(["retry · working", "first · error"]);
    (entries[0] as HTMLElement).click();
    await show();
    expect(chosen).toEqual(["t9"]);
    expect(document.querySelector("[data-testid=task-terminal-entry]")).toBeNull();
  });

  it("matches a remote run only against the terminals of its machine, none when offline", async () => {
    tauri.answer("list_machines", () => [
      { name: "box", ssh: "box.lan", reposDir: "~/Source/{owner}/{repo}", state: "connected" },
    ]);
    const { el, deliver, fixture, show } = await render();
    const machines = TestBed.inject(Machines);
    await machines.start();
    const chosen: string[] = [];
    fixture.componentInstance.paneChosen.subscribe((id) => chosen.push(id));
    withTerminals([
      term("t3", { run: RUN, task: "t12" }),
      term("box::t3", { run: RUN, task: "t12", machine: "box" }),
    ]);
    deliver.apply("/r", { ...events("milestone") });
    deliver.apply("/r", events("milestone"), "box");
    deliver.now.set(AT);
    await show();
    // Both runs show the action, each focusing the terminal of its own machine.
    const current = () => [
      ...el.querySelectorAll<HTMLButtonElement>(
        "[data-testid=current] [data-testid=task-terminal]",
      ),
    ];
    expect(current()).toHaveLength(2);
    current().forEach((b) => b.click());
    expect([...chosen].sort()).toEqual(["box::t3", "t3"]);
    tauri.emit("machine:state_changed", {
      name: "box",
      ssh: "box.lan",
      reposDir: "~/Source/{owner}/{repo}",
      state: "offline",
      error: "ssh: timeout",
    });
    await show();
    expect(current()).toHaveLength(1);
  });
});
