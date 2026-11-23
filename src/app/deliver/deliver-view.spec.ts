// @vitest-environment jsdom
import { readFileSync } from "node:fs";
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { ACTIVITY_ENTRIES } from "../core/activity-items";
import { POIMEN_COMMANDS } from "../core/command-palette";
import { DELIVER_TAB, EditorTabs, tabKind } from "../core/editor-tabs";
import type { Terminal } from "../core/snapshot";
import { tauri } from "../core/tauri.mock";
import { UI_STATE_KEY, UI_STORAGE, UiState } from "../core/ui-state";
import { SpacesStore } from "../core/spaces-store";
import { Machines } from "../core/machines";
import { Deliver, loopPane } from "./deliver";
import { DeliverLauncher } from "./deliver-launcher";
import { Overview } from "./overview";
import { DeliverView, linkLabel, prLabel, stepText } from "./deliver-view";
import { cardsOf, type DeliverCard, type DeliverRead, type DeliverStatus } from "./deliver-status";

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
  try {
    getTestBed().initTestEnvironment(BrowserTestingModule, platformBrowserTesting());
  } catch {
    // already initialized by the shared setup
  }
});

const events = (...names: string[]): Extract<DeliverRead, { kind: "events" }> => ({
  kind: "events",
  runs: names.map((n) =>
    JSON.parse(readFileSync(`src/app/deliver/fixtures/events-${n}.json`, "utf8")),
  ),
});

const read = (name: string): DeliverRead => ({
  kind: "ok",
  status: JSON.parse(readFileSync(`src/app/deliver/fixtures/status-${name}.json`, "utf8")),
});

/** The UI state storage of a test: empty at each render. */
const storage = new Map<string, string>();

async function render() {
  TestBed.resetTestingModule();
  storage.clear();
  TestBed.configureTestingModule({
    providers: [
      {
        provide: UI_STORAGE,
        useValue: {
          getItem: (k: string) => storage.get(k) ?? null,
          setItem: (k: string, v: string) => storage.set(k, v),
        },
      },
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

const text = (el: Element | null) => el?.textContent?.replace(/\s+/g, " ").trim() ?? "";
/** The texts of the child elements, joined by a space (the template drops the whitespace). */
const parts = (el: Element) => [...el.children].map((c) => text(c)).join(" ");
const button = (el: HTMLElement, label: string) =>
  [...el.querySelectorAll("button")].find(
    (b) => !b.closest("[data-testid=overview]") && text(b).includes(label),
  )!;

beforeEach(() => {
  tauri.reset();
  opener.openUrl.mockReset();
});

/** Four runs of M014.1 (one abandoned, one with a merging task after `run.finished`). */
const FOUR_RUNS = JSON.parse(
  readFileSync("src/app/deliver/fixtures/runs-milestone.json", "utf8"),
) as Record<string, unknown>[];
const FOUR: DeliverRead = { kind: "events", runs: FOUR_RUNS };
/** Two minutes after the last event of the running M4 run. */
const AT = Date.parse("2026-11-08T12:00:00Z");

const groups = (el: HTMLElement, section: string) => [
  ...el.querySelectorAll(`[data-testid=${section}] [data-testid=group]`),
];
const lines = (el: Element) => [...el.querySelectorAll("[data-testid=line]")];
const key = (el: HTMLElement, name: string) =>
  el.dispatchEvent(new KeyboardEvent("keydown", { key: name, bubbles: true }));

describe("Runs view", () => {
  it("names the page and says how a run starts while there is none", async () => {
    const { el } = await render();
    expect(text(el.querySelector("h1"))).toBe("Runs");
    expect(text(el.querySelector("header .hint"))).toBe(
      "Agent runs (deliver loops and sessions) of the open repositories.",
    );
    const empty = text(el.querySelector("[data-testid=empty]"));
    expect(empty).toContain(".agents/events/<run>.jsonl");
    expect(empty).toContain("Deliver milestone");
    expect(el.querySelectorAll("[data-testid=group]")).toHaveLength(0);
    expect(el.querySelector("[data-testid=needs-you]")).toBeNull();
  });

  it("shows a running run under Running now: subject, current task, step bar, age, focus", async () => {
    const { el, deliver, fixture, show } = await render();
    deliver.apply("/r", events("milestone"));
    deliver.now.set(AT);
    vi.spyOn(deliver, "paneOf").mockReturnValue("t7");
    const chosen: string[] = [];
    fixture.componentInstance.paneChosen.subscribe((id) => chosen.push(id));
    await show();
    const [group] = groups(el, "running");
    expect(group).toBeDefined();
    expect(text(group!.querySelector(".group-name"))).toBe("M4 Deliver loop");
    expect(text(group!.querySelector("[data-testid=state]"))).toBe("running, updated 2 min ago");
    expect(text(group!.querySelector("[data-testid=progress]"))).toBe("3/9 issues");
    const current = group!.querySelector("[data-testid=current]")!;
    expect(text(current)).toContain("Second");
    expect(text(current.querySelector("[data-testid=step]"))).toBe("implement 2/3");
    const bar = current.querySelector("[aria-label=Step]")!;
    expect([bar.getAttribute("aria-valuenow"), bar.getAttribute("aria-valuemax")]).toEqual([
      "2",
      "3",
    ]);
    expect(text(current.querySelector("[data-testid=step-text]"))).toBe("tests green");
    expect(text(group!.querySelector("[data-testid=age]"))).toBe("last event 2 min ago");
    expect(text(group!.querySelector("footer .hint"))).toBe("claude sonnet · /r");
    const focus = group!.querySelector<HTMLButtonElement>("[data-testid=focus]")!;
    expect(focus.classList).toContain("primary");
    focus.click();
    expect(chosen).toEqual(["t7"]);
    // A running loop has the action too: its dialog refuses a second start (#746).
    expect(group!.querySelector("[data-testid=start]")).not.toBeNull();
    expect(el.querySelector("[data-testid=history]")).toBeNull();
  });

  it("lists what needs the user once, with its group and links", async () => {
    const { el, deliver, show } = await render();
    deliver.apply("/r", {
      kind: "events",
      runs: [...FOUR_RUNS, { ...FOUR_RUNS[2], run: "again" }],
    });
    deliver.apply("/s", events("milestone"));
    deliver.now.set(AT);
    await show();
    const needs = [...el.querySelectorAll("[data-testid=need]")].map(parts);
    expect(needs).toEqual([
      "M4 Deliver loop decide whether the field names are final log",
      "M014.1 Runs view Decide whether #104 ships in M014.1 issue",
    ]);
    // Sections in order: Running now, Needs you, History.
    expect(
      [...el.querySelectorAll(":not([data-testid=overview]) > .section-title")].map((h) => text(h)),
    ).toEqual(["Running now", "Needs you", "History"]);
  });

  it("collapses a subject's runs into one history group, the head with its newest run", async () => {
    const { el, deliver, show } = await render();
    deliver.apply("/r", FOUR);
    deliver.now.set(AT);
    await show();
    const all = groups(el, "history");
    expect(all).toHaveLength(1);
    const [group] = all;
    expect(text(group!.querySelector(".group-name"))).toBe("M014.1 Runs view");
    expect(text(group!.querySelector("[data-testid=group-toggle]"))).toContain("4 runs");
    expect(text(group!.querySelector("[data-testid=state]"))).toContain(
      "failed 2 h ago: deliver.partial",
    );
    // The color follows the tone of the newest run.
    expect(group!.getAttribute("data-tone")).toBe("warning");
    expect(group!.querySelector("[data-testid=timeline]")).toBeNull();
    expect(el.querySelector("[data-testid=running]")).toBeNull();
  });

  it("shows the timeline of a group newest first, one line per run", async () => {
    const { el, deliver, show } = await render();
    deliver.apply("/r", FOUR);
    deliver.now.set(AT);
    await show();
    button(el, "M014.1 Runs view").click();
    await show();
    const rows = lines(el);
    expect(rows.map((l) => text(l.querySelector("[data-testid=outcome]")))).toEqual([
      "failed: deliver.partial",
      "abandoned",
      "failed: 2 of 4 PBIs delivered",
      "failed: deliver.partial",
    ]);
    expect(text(rows[2]!)).toContain("PR #201");
    expect(text(rows[2]!)).toContain("PR #202");
    expect(text(rows[2]!)).toContain("1 skipped");
    expect(text(rows[2]!)).toContain("1 waiting for merge");
    expect(text(rows[1]!)).toContain("1 left open");
    // #101 was taken up by a later run: nothing left open in the first run.
    expect(text(rows[3]!)).not.toContain("left open");
    // A failed run with tone neutral is not shown as an error.
    expect(rows.map((l) => l.getAttribute("data-tone"))).toEqual([
      "warning",
      "neutral",
      "neutral",
      "neutral",
    ]);
    expect(el.querySelectorAll(".tone-danger")).toHaveLength(0);
  });

  it("expands a line: left open, waiting for merge with its PR, finished, links, events", async () => {
    const { el, deliver, show } = await render();
    deliver.apply("/r", FOUR);
    deliver.now.set(AT);
    await show();
    button(el, "M014.1 Runs view").click();
    await show();
    lines(el)[2]!.querySelector<HTMLButtonElement>("[data-testid=line-toggle]")!.click();
    await show();
    const details = el.querySelector("[data-testid=details]")!;
    expect(details.querySelector("[data-testid=tasks]")).toBeNull();
    const left = [...details.querySelectorAll("[data-testid=left-task]")].map(parts);
    expect(left).toEqual(["#102 Timeline waiting for merge PR #202"]);
    const finished = [...details.querySelectorAll("[data-testid=finished] li")].map(parts);
    expect(finished).toEqual(["#101 Group runs succeeded PR #201"]);
    const folds = [...details.querySelectorAll("[data-testid=skipped-fold]")];
    expect(folds.map((f) => text(f.querySelector("summary")))).toEqual(["1 skipped (blocked)"]);
    expect(folds.map((f) => text(f.querySelector("li")))).toEqual(["#103 Keyboard"]);
    expect(text(details.querySelector("[data-testid=links]"))).toBe("Links PR #201 PR #202");
    const rows = [...details.querySelectorAll("[data-testid=event]")].map((e) => e.className);
    expect(rows).toEqual([
      "event tone-neutral",
      "event tone-success",
      "event tone-neutral",
      "event tone-neutral",
    ]);
    button(details as HTMLElement, "PR #202").click();
    expect(opener.openUrl).toHaveBeenCalledWith("https://github.com/GravionLabs/demo/pull/202");
  });

  it("lists the tasks of a running run as open and of an abandoned run as left open", async () => {
    const { el, deliver, show } = await render();
    deliver.apply("/r", { kind: "events", runs: [...FOUR_RUNS, ...events("milestone").runs] });
    deliver.now.set(AT);
    await show();
    for (const toggle of el.querySelectorAll<HTMLButtonElement>("[data-testid=group-toggle]"))
      toggle.click();
    await show();
    for (const toggle of el.querySelectorAll<HTMLButtonElement>("[data-testid=line-toggle]"))
      toggle.click();
    await show();
    const [running] = groups(el, "running");
    const open = [...running!.querySelectorAll("[data-testid=task]")].map(parts);
    expect(open[0]).toBe("Second implement 2/3 1/3 steps");
    expect(running!.querySelector("[data-testid=left-open]")).toBeNull();
    const [history] = groups(el, "history");
    expect(history!.querySelectorAll("[data-testid=tasks]")).toHaveLength(0);
    const left = [...history!.querySelectorAll("[data-testid=left-task]")].map(parts);
    expect(left).toEqual([
      "#104 Needs you section left open at review 1/2",
      "#102 Timeline waiting for merge PR #202",
    ]);
  });

  it("lists only warning, danger and needs-you attentions; success and info go in the line", async () => {
    const { el, deliver, show } = await render();
    const raw = events("milestone").runs[0] as Record<string, unknown>;
    deliver.apply("/r", {
      kind: "events",
      runs: [
        {
          ...raw,
          attentions: [
            ...(raw["attentions"] as unknown[]),
            { id: "a3", task: "t11", reason: "pr.opened", text: "PR opened", tone: "success" },
            { id: "a4", reason: "CI red", tone: "danger" },
          ],
        },
      ],
    });
    deliver.now.set(AT);
    await show();
    button(el, "M4 Deliver loop").click();
    await show();
    const [line] = lines(el);
    expect(text(line!.querySelector("[data-testid=note]"))).toBe("PR opened");
    line!.querySelector<HTMLButtonElement>("[data-testid=line-toggle]")!.click();
    await show();
    const alerts = [...el.querySelectorAll("[data-testid=alerts] li")].map((a) => text(a));
    expect(alerts).toEqual([
      "decide whether the field names are final log",
      "run-wide notice",
      "CI red",
    ]);
  });

  it("offers Deliver milestone once per local milestone group and opens the dialog (#746)", async () => {
    tauri.answer("github_milestones", () => [
      {
        root: "/s",
        repo: "o/s",
        status: "ok",
        milestones: [{ title: "M4 (0.2.0)" }, { title: "Roadmap" }],
      },
    ]);
    const { el, deliver, show } = await render();
    deliver.apply("/r", FOUR);
    deliver.apply("/s", events("milestone"));
    deliver.now.set(AT);
    await show();
    const starts = [...el.querySelectorAll<HTMLButtonElement>("[data-testid=start]")];
    // The group of a running loop has the action too: the dialog refuses a second start.
    expect(starts).toHaveLength(2);
    expect(starts[0]!.textContent).toContain("Deliver milestone");
    const mine = starts.find((b) =>
      b.closest("[data-testid=group]")!.textContent!.includes("M4 Deliver loop"),
    )!;
    mine.click();
    const launcher = TestBed.inject(DeliverLauncher);
    await vi.waitFor(() => expect(launcher.opened()).not.toBeNull());
    expect(launcher.opened()).toMatchObject({
      root: "/s",
      target: { kind: "milestone", name: "M4" },
    });
  });

  it("keeps expanded groups and lines in the UI state", async () => {
    const { el, deliver, show } = await render();
    deliver.apply("/r", FOUR);
    deliver.now.set(AT);
    await show();
    button(el, "M014.1 Runs view").click();
    await show();
    lines(el)[0]!.querySelector<HTMLButtonElement>("[data-testid=line-toggle]")!.click();
    await show();
    const ui = TestBed.inject(UiState);
    expect([...ui.expandedRunGroups()]).toEqual(["/r\ngithub.milestone\nGravionLabs/demo#14"]);
    expect([...ui.expandedRuns()]).toEqual(["/r\n20261108-080000-loop"]);
    expect(JSON.parse(storage.get(UI_STATE_KEY)!)).toMatchObject({
      expandedRunGroups: ["/r\ngithub.milestone\nGravionLabs/demo#14"],
      expandedRuns: ["/r\n20261108-080000-loop"],
    });
  });

  it("moves over groups and lines with Up/Down, expands with Enter, collapses with Escape", async () => {
    const { el, deliver, show } = await render();
    deliver.apply("/r", FOUR);
    deliver.apply("/s", events("milestone"));
    deliver.now.set(AT);
    await show();
    const ui = TestBed.inject(UiState);
    const focused = () => (document.activeElement as HTMLElement | null)?.dataset["row"];
    document.body.appendChild(el);
    key(el, "ArrowDown");
    await show();
    expect(focused()).toBe("/s\ngithub.milestone\nGravionLabs/demo#4");
    key(el, "ArrowDown");
    await show();
    expect(focused()).toBe("/r\ngithub.milestone\nGravionLabs/demo#14");
    key(el, "Enter");
    await show();
    expect(lines(el)).toHaveLength(4);
    key(el, "ArrowDown");
    await show();
    expect(focused()).toBe("/r\n20261108-080000-loop");
    key(el, "Enter");
    await show();
    expect(el.querySelectorAll("[data-testid=details]")).toHaveLength(1);
    key(el, "Escape");
    await show();
    expect(el.querySelectorAll("[data-testid=details]")).toHaveLength(0);
    key(el, "Escape");
    await show();
    expect(lines(el)).toHaveLength(0);
    expect(focused()).toBe("/r\ngithub.milestone\nGravionLabs/demo#14");
    expect(ui.expandedRunGroups().size).toBe(0);
    key(el, "ArrowUp");
    await show();
    expect(focused()).toBe("/s\ngithub.milestone\nGravionLabs/demo#4");
    el.remove();
  });

  it("acts on the row toggle that has the focus, not on the row clicked before", async () => {
    const { el, deliver, show } = await render();
    deliver.apply("/r", FOUR);
    deliver.apply("/s", events("milestone"));
    deliver.now.set(AT);
    await show();
    const ui = TestBed.inject(UiState);
    const toggle = (row: string) =>
      [...el.querySelectorAll<HTMLElement>("[data-row]")].find((e) => e.dataset["row"] === row)!;
    const m4 = "/s\ngithub.milestone\nGravionLabs/demo#4";
    const m014 = "/r\ngithub.milestone\nGravionLabs/demo#14";
    toggle(m4).click();
    await show();
    expect([...ui.expandedRunGroups()]).toEqual([m4]);
    // Tab moves the focus to the next toggle without a click; Enter there expands that group.
    const enter = new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true });
    toggle(m014).dispatchEvent(enter);
    await show();
    expect([...ui.expandedRunGroups()].sort()).toEqual([m014, m4]);
    expect(enter.defaultPrevented).toBe(true);
    // Down from that toggle goes to its first line, not to the line after the clicked group.
    document.body.appendChild(el);
    toggle(m014).dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true }));
    await show();
    expect((document.activeElement as HTMLElement | null)?.dataset["row"]).toBe(
      "/r\n20261108-080000-loop",
    );
    el.remove();
  });

  it("leaves Enter and Escape on a link or action button to that button", async () => {
    const { el, deliver, show } = await render();
    deliver.apply("/r", FOUR);
    deliver.now.set(AT);
    await show();
    const ui = TestBed.inject(UiState);
    key(el, "ArrowDown");
    const open = button(el, "Open milestone");
    open.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
    await show();
    expect(ui.expandedRunGroups().size).toBe(0);
    // From the view itself Enter expands the group under the cursor.
    key(el, "Enter");
    await show();
    expect(ui.expandedRunGroups().size).toBe(1);
  });

  it("opens with the group of a repository's run expanded, as a deliverRoot notification asks", async () => {
    const { el, deliver, show } = await render();
    deliver.apply("/r", FOUR);
    deliver.apply("/s", events("session"));
    deliver.now.set(AT);
    expect(deliver.reveal("/r")).toBe(true);
    await show();
    expect(
      TestBed.inject(EditorTabs)
        .tabs()
        .map((t) => t.paneId),
    ).toContain(DELIVER_TAB);
    const [group] = groups(el, "history").filter((g) => g.textContent?.includes("M014.1"));
    expect(group!.classList).toContain("cursor");
    expect(lines(group!)).toHaveLength(4);
    expect(deliver.reveal("/nowhere")).toBe(false);
  });

  it("hints at events of a version it does not know, keeping the readable runs", async () => {
    const { el, deliver, show } = await render();
    deliver.apply("/r", { ...events("failed"), unknownVersion: 2 } as DeliverRead);
    await show();
    expect(text(el.querySelector("[data-testid=events-version]"))).toContain("version 2");
    expect(el.querySelectorAll("[data-testid=group]")).toHaveLength(1);
  });

  it("shows a hint for an unknown legacy schema version and for a broken file", async () => {
    const { el, deliver, show } = await render();
    deliver.apply("/a", { kind: "unsupported", schemaVersion: 2 });
    deliver.apply("/b", { kind: "invalid", error: "expected value at line 1" });
    await show();
    expect(text(el.querySelector("[data-testid=unsupported]"))).toContain("schema version 2");
    expect(text(el.querySelector("[data-testid=invalid]"))).toContain("expected value");
    expect(el.querySelectorAll("[data-testid=problem]")).toHaveLength(2);
  });

  it("shows a legacy status file like any other run and drops it when it disappears", async () => {
    const { el, deliver, show } = await render();
    deliver.apply("/src/skills", read("running"));
    await show();
    expect(text(el.querySelector(".group-name"))).toContain("M4");
    expect(text(el.querySelector("[data-testid=progress]"))).toBe("3/9");
    deliver.apply("/src/skills", { kind: "missing" });
    await show();
    expect(el.querySelectorAll("[data-testid=group]")).toHaveLength(0);
  });

  it("opens subject and link URLs in the browser, GitHub only", async () => {
    const { el, deliver, show } = await render();
    deliver.apply("/r", events("milestone"));
    deliver.now.set(AT);
    await show();
    button(el, "Open milestone").click();
    expect(opener.openUrl).toHaveBeenCalledWith("https://github.com/GravionLabs/demo/milestone/4");
    button(el, "Second").click();
    expect(opener.openUrl).toHaveBeenLastCalledWith(
      "https://github.com/GravionLabs/demo/issues/12",
    );
    opener.openUrl.mockClear();
    await deliver.openUrl("https://evil.example/x");
    await deliver.openUrl(null);
    expect(opener.openUrl).not.toHaveBeenCalled();
  });
});

describe("Deliver service ordering", () => {
  it("keeps the order of the runs on an update and drops the runs that are gone", async () => {
    const { deliver } = await render();
    deliver.apply("/r", events("failed", "session"));
    const order = () =>
      deliver.cards().map((c) => (c.state.kind === "ok" ? c.state.status.tone : c.state.kind));
    expect(order()).toEqual(["danger", "neutral"]);
    deliver.apply("/r", events("session", "failed"));
    expect(order()).toEqual(["danger", "neutral"]);
    deliver.apply("/r", events("session"));
    expect(order()).toEqual(["neutral"]);
  });
});

describe("Deliver service", () => {
  it("receives deliver events once started, and stops listening", async () => {
    const { deliver } = await render();
    const stop = deliver.start();
    await vi.waitFor(() => expect(tauri.listen).toHaveBeenCalledWith("deliver", expect.anything()));
    tauri.emit("deliver", { root: "/r", read: read("running") });
    expect(deliver.cards()).toHaveLength(1);
    stop();
    await vi.waitFor(() => expect(tauri.unlisten).toHaveBeenCalledWith("deliver"));
    tauri.emit("deliver", { root: "/r2", read: read("running") });
    expect(deliver.cards()).toHaveLength(1);
  });

  it("watches the directories once and forgets cards of repositories that are gone", async () => {
    tauri.answer("deliver_watch", () => ["/a"]);
    const { deliver } = await render();
    deliver.apply("/a", read("running"));
    deliver.apply("/gone", read("done"));
    await deliver.watch(["/a/x", "/a", "/a/x"]);
    await deliver.watch(["/a", "/a/x"]);
    expect(tauri.calls("deliver_watch")).toEqual([{ dirs: ["/a/x", "/a"] }]);
    expect(deliver.cards().map((c) => c.root)).toEqual(["/a"]);
  });

  it("tries again when the watch fails", async () => {
    tauri.answer("deliver_watch", () => {
      throw new Error("boom");
    });
    vi.spyOn(console, "warn").mockImplementation(() => undefined);
    const { deliver } = await render();
    await deliver.watch(["/a"]);
    await deliver.watch(["/a"]);
    expect(tauri.calls("deliver_watch")).toHaveLength(2);
  });
});

describe("remote runs (#692)", () => {
  const machine = (state: string) => ({
    name: "box",
    ssh: "box.lan",
    reposDir: "~/Source/{owner}/{repo}",
    state,
    ...(state === "connected" ? { serverVersion: "0.18.0" } : { error: "ssh: timeout" }),
  });

  async function withBox() {
    tauri.answer("list_machines", () => [machine("connected")]);
    const view = await render();
    const machines = TestBed.inject(Machines);
    await machines.start();
    return { ...view, machines };
  }

  it("merges a local and a remote run with the same run id, labelled with the machine", async () => {
    const { deliver, el, show } = await withBox();
    const stop = deliver.start();
    await vi.waitFor(() => expect(tauri.listen).toHaveBeenCalledWith("deliver", expect.anything()));
    tauri.emit("deliver", { root: "/r", read: events("session") });
    tauri.emit("deliver", { root: "/r", machine: "box", read: events("session") });
    // A replay after a reconnect updates the same card.
    tauri.emit("deliver", { root: "/r", machine: "box", read: events("session") });
    await show();
    expect(deliver.cards().map((c) => c.machine)).toEqual([undefined, "box"]);
    const cards = el.querySelectorAll("[data-testid=group]");
    expect(cards).toHaveLength(2);
    expect(cards[0]!.querySelector("[data-testid=machine]")).toBeNull();
    expect(text(cards[1]!.querySelector("[data-testid=machine]"))).toBe("box");
    expect(text(cards[1]!.querySelector("footer"))).toContain("box:/r");
    expect(cards[1]!.querySelector("[data-testid=offline]")).toBeNull();
    stop();
  });

  it("keeps the last state of an offline machine marked, and drops a removed one", async () => {
    const { deliver, el, show } = await withBox();
    deliver.apply("/r", events("milestone", "failed"), "box");
    tauri.emit("machine:state_changed", machine("offline"));
    await show();
    const card = el.querySelector("[data-testid=group]")!;
    expect(text(card.querySelector("[data-testid=offline]"))).toContain("offline");
    expect(card.classList).toContain("offline");
    expect(deliver.cards()).toHaveLength(2);
    // Remote runs are not started from here (#693).
    expect(el.querySelector("[data-testid=start]")).toBeNull();
    tauri.answer("list_machines", () => []);
    await TestBed.inject(Machines).refresh();
    await show();
    expect(deliver.cards()).toEqual([]);
  });

  it("keeps the remote cards when the local watch changes", async () => {
    tauri.answer("deliver_watch", () => ["/a"]);
    const { deliver } = await withBox();
    deliver.apply("/a", read("running"));
    deliver.apply("/remote", events("session"), "box");
    await deliver.watch(["/a"]);
    expect(deliver.cards().map((c) => c.root)).toEqual(["/a", "/remote"]);
    // A remote run has no start action and no launcher state of this Mac (#746).
    expect(deliver.activeLoop("/remote")).toBeUndefined();
  });
});

describe("helpers", () => {
  const pane = (id: string, cwd: string, agent?: string): Terminal =>
    ({ terminal_id: id, tab_id: "t", space_id: "s", agent_status: "idle", cwd, agent }) as Terminal;

  it("finds the agent pane of the loop inside the repository, preferring its harness", () => {
    const panes = [
      pane("shell", "/src/skills"),
      pane("other", "/src/skills/sub", "copilot"),
      pane("loop", "/src/skills", "claude"),
      pane("elsewhere", "/src/skills-other", "claude"),
    ];
    expect(loopPane(panes, "/src/skills", "claude")).toBe("loop");
    expect(loopPane(panes, "/src/skills", "codex")).toBe("other");
    expect(loopPane([panes[0], panes[3]], "/src/skills", "claude")).toBeUndefined();
  });

  it("names a link by its title, a pull request number or its rel", () => {
    expect(linkLabel({ rel: "log", url: "https://x", title: "CI log" })).toBe("CI log");
    expect(
      linkLabel({ rel: "pull-request", url: "https://github.com/o/n/pull/3", title: null }),
    ).toBe("PR #3");
    expect(linkLabel({ rel: "log", url: "https://x", title: null })).toBe("log");
    expect(stepText({ name: "implement", n: 2, total: 3, text: null })).toBe("implement 2/3");
    expect(stepText({ name: "review", n: null, total: null, text: null })).toBe("review");
  });

  it("finds the terminal of a run: the host terminal when open, else an agent pane", () => {
    const { deliver } = (() => {
      TestBed.resetTestingModule();
      return { deliver: TestBed.inject(Deliver) };
    })();
    const store = TestBed.inject(SpacesStore);
    vi.spyOn(store, "all").mockReturnValue({
      spaces: [],
      tabs: [],
      terminals: [
        pane("t7", "/elsewhere"),
        pane("t2", "/r", "claude"),
        { ...pane("box::t3", "/r", "claude"), machine: "box" },
      ],
    });
    const card = (host: Record<string, string>): DeliverCard => ({
      root: "/r",
      run: "x",
      state: {
        kind: "ok",
        status: {
          ...(cardsOf("/r", events("session"))[0]!.state as { status: DeliverStatus }).status,
          host,
        },
      },
    });
    expect(deliver.paneOf(card({ "poimen.terminal": "t7" }))).toBe("t7");
    expect(deliver.paneOf(card({ "poimen.terminal": "gone" }))).toBe("t2");
    expect(deliver.paneOf({ root: "/r", run: "", state: { kind: "invalid", error: "x" } })).toBe(
      undefined,
    );
    // A remote run finds only a terminal of its machine.
    const remote = (host: Record<string, string>) => ({ ...card(host), machine: "box" });
    expect(deliver.paneOf(remote({ "poimen.terminal": "box::t3" }))).toBe("box::t3");
    expect(deliver.paneOf(remote({ "poimen.terminal": "t7" }))).toBe("box::t3");
    expect(deliver.paneOf({ ...card({}), machine: "gpu" })).toBeUndefined();
  });

  it("labels a pull request URL", () => {
    expect(prLabel("https://github.com/o/n/pull/343")).toBe("PR #343");
    expect(prLabel("https://example.com")).toBe("https://example.com");
  });
});

describe("entry points", () => {
  it("is a tab in the activity bar, the palette and the editor tabs", () => {
    expect(ACTIVITY_ENTRIES.find((v) => v.id === "deliver")).toMatchObject({
      label: "Runs",
      target: { kind: "tab", tab: "deliver" },
    });
    expect(POIMEN_COMMANDS).toContainEqual({ id: "openDeliver", title: "Open Runs" });
    expect(tabKind(DELIVER_TAB)).toBe("deliver");
    TestBed.resetTestingModule();
    TestBed.configureTestingModule({
      providers: [
        { provide: UI_STORAGE, useValue: { getItem: () => null, setItem: () => undefined } },
      ],
    });
    const tabs = TestBed.inject(EditorTabs);
    tabs.openDeliver();
    tabs.openDeliver();
    expect(tabs.tabs().filter((t) => t.paneId === DELIVER_TAB)).toEqual([
      { paneId: DELIVER_TAB, pinned: true },
    ]);
  });
});

describe("Runs view: milestone overview (#585)", () => {
  const MILESTONE_URL = "https://github.com/GravionLabs/demo/milestone/14";
  const milestone = (extra: Record<string, unknown> = {}) => ({
    number: 14,
    title: "M014.1 Runs view",
    url: MILESTONE_URL,
    dueOn: null,
    description: "",
    open: 3,
    closed: 1,
    current: true,
    ...extra,
  });
  const issue = (number: number, extra: Record<string, unknown> = {}) => ({
    number,
    title: `Issue ${number}`,
    url: `https://github.com/GravionLabs/demo/issues/${number}`,
    state: "OPEN",
    labels: ["pbi"],
    assignees: [],
    milestone: "M014.1 Runs view",
    parent: null,
    subTotal: 0,
    subCompleted: 0,
    prs: [],
    ...extra,
  });
  const answerGithub = (error?: unknown) => {
    tauri.answer("github_milestones", () => [
      error
        ? { root: "/r", repo: "GravionLabs/demo", status: "error", error }
        : {
            root: "/r",
            repo: "GravionLabs/demo",
            status: "ok",
            milestones: [
              milestone({ number: 15, title: "M015", url: "u15", current: false }),
              milestone(),
            ],
          },
    ]);
    tauri.answer("github_issues", (args) => [
      {
        root: "/r",
        repo: "GravionLabs/demo",
        status: "ok",
        truncated: false,
        issues: args?.["closed"]
          ? [issue(100, { state: "CLOSED" })]
          : [
              issue(101),
              issue(102, { blockedBy: [101] }),
              issue(103, { labels: ["pbi", "needs-decision"] }),
            ],
      },
    ]);
  };
  const showOverview = async () => {
    const view = await render();
    vi.spyOn(TestBed.inject(Overview) as unknown as { dirs(): string[] }, "dirs").mockReturnValue([
      "/r",
    ]);
    view.deliver.apply("/r", FOUR);
    view.deliver.now.set(AT);
    await TestBed.inject(Overview).load();
    await view.show();
    return view;
  };

  it("lists the current milestone first with progress and counts, the others collapsed", async () => {
    answerGithub();
    const { el } = await showOverview();
    const rows = [...el.querySelectorAll("[data-testid=milestone]")];
    expect(rows.map((r) => text(r.querySelector(".group-name")))).toEqual(
      ["M014.1 Runs view current", "M015"].map((t) => t.replace(" current", "")),
    );
    expect(text(rows[0]!.querySelector("[data-testid=milestone-progress-text]"))).toBe(
      "1/4 closed",
    );
    expect(text(rows[0]!.querySelector("[data-testid=milestone-counts]"))).toBe(
      "0 in progress · 1 blocked · 1 needs you · 1 open",
    );
    expect(el.querySelectorAll("[data-testid=overview-issue]")).toHaveLength(0);
  });

  it("expands a milestone to its issues with their states and keeps that in the UI state", async () => {
    answerGithub();
    const { el, show } = await showOverview();
    (el.querySelector("[data-testid=milestone-toggle]") as HTMLElement).click();
    await show();
    const states = [...el.querySelectorAll("[data-testid=overview-issue]")].map((li) => [
      li.getAttribute("data-state"),
      text(li.querySelector("[data-testid=issue-link]")),
    ]);
    expect(states).toEqual([
      ["done", "#100 Issue 100"],
      ["next", "#101 Issue 101"],
      ["blocked", "#102 Issue 102"],
      ["needs-you", "#103 Issue 103"],
    ]);
    expect(text(el.querySelector("[data-testid=issue-blockers]"))).toBe("blocked by #101");
    expect(
      TestBed.inject(UiState).expandedMilestones().has("GravionLabs/demo#M014.1 Runs view"),
    ).toBe(true);
  });

  it("shows GitHub's progress in the head of a milestone group instead of the run's own", async () => {
    answerGithub();
    const { el } = await showOverview();
    const group = [...el.querySelectorAll("[data-testid=group]")][0]!;
    expect(text(group.querySelector("[data-testid=progress]"))).toBe("1/4 closed");
  });

  it("lists the milestone of the run subjects with a hint when GitHub fails", async () => {
    answerGithub({ kind: "offline" });
    const { el } = await showOverview();
    expect(text(el.querySelector("[data-testid=overview-repo-error]"))).toContain("not reachable");
    const rows = [...el.querySelectorAll("[data-testid=milestone]")];
    expect(rows).toHaveLength(1);
    expect(text(rows[0]!.querySelector(".group-name"))).toBe("M014.1 Runs view");
    expect(el.querySelector("[data-testid=milestone-progress-text]")).toBeNull();
    // Without GitHub the group head keeps the run's own progress.
    const group = [...el.querySelectorAll("[data-testid=group]")][0]!;
    expect(text(group.querySelector("[data-testid=progress]"))).toBe("4/7 issues");
  });

  it("leaves out the group of a milestone GitHub no longer lists open", async () => {
    answerGithub();
    tauri.answer("github_milestones", () => [
      {
        root: "/r",
        repo: "GravionLabs/demo",
        status: "ok",
        milestones: [milestone({ title: "M020", url: "u20" })],
      },
    ]);
    const { el } = await showOverview();
    expect(el.querySelectorAll("[data-testid=group]")).toHaveLength(0);
  });

  it("shows the issue title for a task whose subject title is only #<nr>", async () => {
    answerGithub();
    const { el, deliver, show } = await showOverview();
    const run = JSON.parse(JSON.stringify(FOUR_RUNS[3])) as Record<string, unknown>;
    run["status"] = "running";
    run["tasks"] = [
      {
        task: "t101",
        subject: { kind: "github.issue", id: "GravionLabs/demo#101", title: "#101" },
        step: null,
      },
    ];
    deliver.apply("/r", { kind: "events", runs: [run] });
    deliver.now.set(Date.parse(run["updated"] as string) + 1000);
    await show();
    expect(text(el.querySelector("[data-testid=current]"))).toContain("#101 Issue 101");
  });
});
