// @vitest-environment jsdom
import { readFileSync } from "node:fs";
import { Component, signal } from "@angular/core";
import { TestBed, getTestBed } from "@angular/core/testing";
import { HxStatusBar, HxStatusItem, HxStatusPopover } from "@gravionlabs/helix-desktop";
import { HxButton, HxTooltip } from "@gravionlabs/helix-ui";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { beforeAll, describe, expect, it } from "vitest";
import { Icon } from "../ui/icon/icon";
import { SERVER_RESTART_NOTE, type UpdateState } from "../core/updater";
import type { BarSummary } from "../deliver/deliver-status";
import type { LimitsItem } from "../core/claude-limits";
import { scheduleHint, type ScheduleHint, type ScheduleList } from "../core/schedules-model";
import type { ConnectionState } from "../core/spaces-store";
import { densityFlags, shortRuns, type DensityLevel } from "./status-density";
import { CHANNEL_TITLE, connectionLabel, modeLabel, modeTitle, sizeLabel } from "./status-bar";

beforeAll(() => {
  try {
    getTestBed().initTestEnvironment(BrowserTestingModule, platformBrowserTesting());
  } catch {
    // already initialized by the shared setup
  }
});

describe("connectionLabel", () => {
  it("names a connected server", () => {
    expect(connectionLabel({ state: "connected" })).toBe("poimen connected");
  });
  it("shows the reason when reconnecting", () => {
    expect(connectionLabel({ state: "reconnecting", reason: "socket closed" })).toBe(
      "poimen reconnecting: socket closed",
    );
  });
  it("shows the reason when unreachable, and copes with an empty one", () => {
    expect(connectionLabel({ state: "unreachable", reason: "no socket" })).toBe(
      "poimen unreachable: no socket",
    );
    expect(connectionLabel({ state: "unreachable", reason: "" })).toBe("poimen unreachable");
  });
});

describe("sizeLabel", () => {
  it("formats cols by rows", () => {
    expect(sizeLabel({ cols: 120, rows: 30 })).toBe("120×30");
  });
  it("is empty until measured", () => {
    expect(sizeLabel(undefined)).toBe("");
  });
});

describe("modeLabel", () => {
  it("names input and plain read-only", () => {
    expect(modeLabel("control", null)).toBe("Input");
    expect(modeLabel("observe", null)).toBe("Read-only");
  });
  it("says when another controller is the reason", () => {
    expect(modeLabel("observe", "controller held")).toBe("Read-only (controlled elsewhere)");
  });
  it("explains the disabled switch in the tooltip", () => {
    expect(modeTitle("controller held")).toContain("controller held");
    expect(modeTitle(null)).toBe("Switch between read-only and input");
  });
});

describe("mode button", () => {
  const render = (
    mode: "control" | "observe",
    blocked: string | null,
    panelOpen = true,
    deliver: BarSummary | null = null,
    server: string | null = null,
    update: UpdateState = { kind: "idle" },
    channel: "stable" | "rc" = "stable",
    awake = false,
    level: DensityLevel = 0,
    limits: LimitsItem | null = null,
    waiting = 0,
    connection: ConnectionState = { state: "connected" },
    schedules: ScheduleHint | null = null,
  ) => {
    const emitted: boolean[] = [];
    const serverClicks: string[] = [];
    let deliverClicks = 0;
    let shown = 0;
    let opened = 0;
    let scheduleClicks = 0;
    @Component({
      selector: "app-host",
      imports: [HxStatusBar, HxStatusItem, HxStatusPopover, HxButton, HxTooltip, Icon],
      template: readFileSync("src/app/status-bar/status-bar.html", "utf8"),
    })
    class Host {
      connection = () => connection;
      waiting = () => waiting;
      density = () => densityFlags(level);
      shortRuns = shortRuns;
      awake = () => awake;
      paneId = () => "p1";
      mode = () => mode;
      blocked = () => blocked;
      size = () => undefined;
      appVersion = () => null;
      channel = () => channel;
      channelTitle = CHANNEL_TITLE;
      update = () => update;
      detailsOpen = signal(update.kind === "available" || update.kind === "error");
      serverNotice = () => server;
      restartServer = { emit: () => serverClicks.push("restart") };
      laterServer = { emit: () => serverClicks.push("later") };
      restartNote = SERVER_RESTART_NOTE;
      errTitle = () => "";
      checkUpdate = { emit: () => {} };
      installUpdate = { emit: () => {} };
      info = () => null;
      notice = () => "layout.json: invalid JSON";
      jump = { emit: () => {} };
      panelOpen = () => panelOpen;
      panelShortcut = () => "⌘J";
      showPanel = { emit: () => shown++ };
      shortcutsShortcut = () => "⌘/";
      showShortcuts = { emit: () => opened++ };
      control = { emit: (v: boolean) => emitted.push(v) };
      label = connectionLabel;
      modeText = modeLabel;
      modeHint = modeTitle;
      sizeText = sizeLabel;
      appLabel = () => "";
      versionLabel = () => "";
      warning = () => "";
      deliver = () => deliver;
      limits = () => limits;
      showDeliver = { emit: () => deliverClicks++ };
      schedules = () => schedules;
      showSchedules = { emit: () => scheduleClicks++ };
    }
    TestBed.resetTestingModule();
    const fixture = TestBed.createComponent(Host);
    fixture.detectChanges();
    const button = (fixture.nativeElement as HTMLElement).querySelector<HTMLButtonElement>(
      ".mode-button button",
    )!;
    return {
      button,
      emitted,
      serverClicks,
      root: fixture.nativeElement as HTMLElement,
      shown: () => shown,
      opened: () => opened,
      deliverClicks: () => deliverClicks,
      scheduleClicks: () => scheduleClicks,
    };
  };

  it("shows version, notes, restart note and Install and restart in the popover", () => {
    const { root } = render("control", null, true, null, null, {
      kind: "available",
      info: { version: "1.2.3", notes: "n", date: null },
    });
    const popover = root.querySelector(".update-details")!;
    expect(popover.textContent).toContain("poimen 1.2.3");
    expect(popover.textContent).toContain("Install and restart");
    expect(popover.querySelector("[data-testid=restart-note]")?.textContent).toContain(
      "restarting ends all terminals",
    );
  });

  it("offers Try again after a failed check and keeps Install after a failed install", () => {
    const check = render("control", null, true, null, null, {
      kind: "error",
      step: "check",
      message: "offline",
      info: null,
    });
    expect(check.root.querySelector(".update-details")?.textContent).toContain("Try again");
    expect(check.root.querySelector(".update-details")?.textContent).toContain("offline");
    const install = render("control", null, true, null, null, {
      kind: "error",
      step: "install",
      message: "bad signature",
      info: { version: "1.2.3", notes: null, date: null },
    });
    expect(install.root.querySelector(".update-details")?.textContent).toContain(
      "Install and restart",
    );
  });

  describe("schedule hint", () => {
    // Local times, so the text does not depend on the time zone of the test run.
    const now = new Date(2026, 0, 10, 8, 0).getTime();
    const schedule = (next: Date | undefined, mode = "nightly", enabled = true) => ({
      key: mode,
      cron: "0 9 * * *",
      timezone: "UTC",
      dir: "/repo",
      mode,
      enabled,
      next_ms: next?.getTime(),
    });
    const bar = (list: ScheduleList) =>
      render(
        "control",
        null,
        true,
        null,
        null,
        { kind: "idle" },
        "stable",
        false,
        0,
        null,
        0,
        { state: "connected" },
        scheduleHint(list, now),
      );
    const button = (root: HTMLElement) =>
      root.querySelector<HTMLButtonElement>("[data-testid=schedules] button");

    it("shows the earliest next run as a text button that opens the schedules", () => {
      const view = bar({
        paused: false,
        schedules: [
          schedule(new Date(2026, 0, 10, 17, 30), "later"),
          schedule(new Date(2026, 0, 10, 9, 5)),
          schedule(new Date(2026, 0, 10, 8, 30), "off", false),
        ],
      });
      const hint = button(view.root)!;
      expect(hint.textContent?.trim()).toBe("Schedules · next 09:05");
      expect(hint.getAttribute("variant")).toBe("text");
      hint.click();
      expect(view.scheduleClicks()).toBe(1);
    });

    it("names the day of a run that is not today, and the next schedule in the tooltip", () => {
      const list = { paused: false, schedules: [schedule(new Date(2026, 0, 11, 9, 0))] };
      expect(button(bar(list).root)?.textContent?.trim()).toBe("Schedules · next Jan 11 09:00");
      expect(scheduleHint(list, now)?.tooltip).toBe("Next: nightly in /repo");
    });

    it("says that schedules are paused", () => {
      const view = bar({ paused: true, schedules: [schedule(undefined)] });
      expect(button(view.root)?.textContent?.trim()).toBe("Schedules paused");
    });

    it("is absent without a schedule that would run", () => {
      for (const paused of [false, true]) {
        const off = schedule(undefined, "off", false);
        expect(button(bar({ paused, schedules: [off] }).root)).toBeNull();
        expect(button(bar({ paused, schedules: [] }).root)).toBeNull();
      }
      expect(button(bar({ paused: false, schedules: [schedule(undefined)] }).root)).toBeNull();
    });
  });

  it("shows the running deliver loops and opens the panel on a click", () => {
    const { root, deliverClicks } = render("control", null, true, {
      text: "▶ M5 7/19",
      title: "o/n M5 7/19",
      warning: false,
    });
    const item = root.querySelector<HTMLElement>("hx-status-item.deliver")!;
    expect(item.textContent?.trim()).toBe("▶ M5 7/19");
    expect(item.classList.contains("hx-status-warn")).toBe(false);
    item.querySelector("button")!.click();
    expect(deliverClicks()).toBe(1);
  });

  it("marks the deliver item as a warning and hides it without a run", () => {
    const warned = render("control", null, true, {
      text: "⚠ M5 7/19",
      title: "",
      warning: true,
    });
    expect(warned.root.querySelector(".deliver")!.classList.contains("hx-status-warn")).toBe(true);
    expect(render("control", null).root.querySelector(".deliver")).toBeNull();
  });

  it("offers to show the closed output panel, with its shortcut", () => {
    const { root, shown } = render("control", null, false);
    const panel = root.querySelector<HTMLButtonElement>(".panel-button button")!;
    expect(panel.textContent?.trim()).toBe("Output");
    expect(panel.title).toBe("Show the output panel (⌘J)");
    panel.click();
    expect(shown()).toBe(1);
  });

  it("has an accessible ? button that opens the shortcuts overview", () => {
    const { root, opened } = render("control", null);
    const help = root.querySelector<HTMLButtonElement>(".help-button button")!;
    expect(help.textContent?.trim()).toBe("?");
    expect(help.getAttribute("aria-label")).toBe("Keyboard shortcuts");
    expect(help.title).toBe("Keyboard shortcuts (⌘/)");
    help.click();
    expect(opened()).toBe(1);
  });

  it("hides the server notice while the server is fine", () => {
    expect(render("control", null).root.querySelector("[data-testid=server-notice]")).toBeNull();
  });

  it("shows the server notice with Restart server and Later", () => {
    const text = "poimen-server 0.1.0 is running, 0.2.0 is installed";
    const { root, serverClicks } = render("control", null, true, null, text);
    expect(root.querySelector("[data-testid=server-notice]")?.textContent).toContain(text);
    root.querySelector<HTMLButtonElement>("[data-testid=restart-server] button")!.click();
    root.querySelector<HTMLButtonElement>("[data-testid=later-server] button")!.click();
    expect(serverClicks).toEqual(["restart", "later"]);
  });

  it("says before the install that restarting the server ends the terminals", () => {
    const info = { version: "9.9.9", currentVersion: "0.1.0", notes: null, date: null };
    const { root } = render("control", null, true, null, null, {
      kind: "available",
      info,
    });
    const note = root.querySelector("[data-testid=restart-note]")?.textContent ?? "";
    expect(note).toContain("restarting ends all terminals");
  });

  it("ends with the versions, the update link and the ? button, in this order", () => {
    const { root } = render("control", null);
    const items = Array.from(root.querySelectorAll("hx-status-item"));
    const kinds = items.map((el) =>
      el.classList.contains("help-button")
        ? "help"
        : el.classList.contains("update-button")
          ? "update"
          : el.classList.contains("version")
            ? "version"
            : "other",
    );
    expect(kinds.slice(-4)).toEqual(["version", "version", "update", "help"]);
    expect(items.at(-1)?.classList.contains("help-button")).toBe(true);
    expect(items.at(-2)?.textContent?.trim()).toBe("Check for updates");
  });

  it("shows the RC badge and no self-update in an RC build", () => {
    const { root } = render("control", null, true, null, null, { kind: "idle" }, "rc");
    const badge = root.querySelector<HTMLElement>("[data-testid=channel]")!;
    expect(badge.textContent?.trim()).toBe("RC");
    expect(badge.querySelector("[title]")?.getAttribute("title")).toContain(
      "does not update itself",
    );
    expect(root.querySelector(".update-button")).toBeNull();
    expect(root.textContent).not.toContain("Check for updates");
  });

  it("shows that the machine is kept awake only while it is", () => {
    const off = render("control", null);
    expect(off.root.querySelector("[data-testid=awake]")).toBeNull();
    const on = render("control", null, true, null, null, { kind: "idle" }, "stable", true);
    expect(on.root.querySelector("[data-testid=awake]")?.textContent?.trim()).toBe("awake");
  });

  it("has no RC badge in a stable build", () => {
    const { root } = render("control", null);
    expect(root.querySelector("[data-testid=channel]")).toBeNull();
    expect(root.querySelector(".update-button")?.textContent?.trim()).toBe("Check for updates");
  });

  it("puts the pane id, mode and size before the versions", () => {
    const { root } = render("control", null);
    const items = Array.from(root.querySelectorAll("hx-status-item"));
    const mode = items.findIndex((el) => el.classList.contains("mode-button"));
    const firstVersion = items.findIndex((el) => el.classList.contains("version"));
    expect(mode).toBeGreaterThan(-1);
    expect(mode).toBeLessThan(firstVersion);
  });

  it("is no live region as a whole; only changing items announce", () => {
    const { root } = render("control", null);
    expect(root.querySelector("hx-status-bar")?.getAttribute("role")).toBeNull();
    const announcing = (cls: string) =>
      root.querySelector(`hx-status-item.${cls}`)?.getAttribute("role");
    expect(announcing("conn")).toBe("status");
    expect(announcing("waiting")).toBe("status");
    expect(announcing("app-version")).toBeNull();
    expect(announcing("server-version")).toBeNull();
    expect(announcing("help-button")).toBeNull();
  });

  it("shows warnings with the tone of the item", () => {
    const { root } = render("control", null);
    expect(root.querySelector("hx-status-item.notice")?.classList.contains("hx-status-warn")).toBe(
      true,
    );
  });

  it("hides the panel button while the panel is open", () => {
    const { root } = render("control", null, true);
    expect(root.querySelector(".panel-button")).toBeNull();
  });

  it("shows the notice (a layout problem of a new space)", () => {
    const { root } = render("control", null);
    expect(root.querySelector(".notice")?.textContent).toContain("layout.json: invalid JSON");
  });

  it("shows Input with a pencil and asks for read-only on click", () => {
    const { button, emitted } = render("control", null);
    expect(button.textContent?.trim()).toBe("Input");
    expect(button.querySelector(".pencil")).not.toBeNull();
    expect(button.disabled).toBe(false);
    button.click();
    expect(emitted).toEqual([false]);
  });
  it("shows Read-only with a lock and asks for input on click", () => {
    const { button, emitted } = render("observe", null);
    expect(button.textContent?.trim()).toBe("Read-only");
    expect(button.querySelector(".lock")).not.toBeNull();
    button.click();
    expect(emitted).toEqual([true]);
  });
  it("is disabled with an explaining tooltip while another client controls", () => {
    const { button, emitted } = render("observe", "controller held");
    expect(button.disabled).toBe(true);
    expect(button.title).toContain("controller held");
    button.click();
    expect(emitted).toEqual([]);
  });

  describe("status bar density (#1034)", () => {
    const LIMITS: LimitsItem = {
      text: "5h 62 %",
      full: "5h 62 % · 7d 20 %",
      title: "Claude limits\n5h: 62 %\n7d: 20 %",
      warning: false,
    };
    const RUNS: BarSummary = {
      text: "▶ M5 7/19",
      title: "M5: 7/19",
      warning: false,
    };
    const text = (root: HTMLElement, sel: string) =>
      root.querySelector(sel)?.textContent?.replace(/\s+/g, " ").trim();
    const at = (level: DensityLevel, connection?: ConnectionState) =>
      render(
        "control",
        null,
        false,
        RUNS,
        null,
        { kind: "idle" },
        "stable",
        true,
        level,
        LIMITS,
        3,
        connection,
      ).root;

    it("steps down in the order limits, runs, low-priority items", () => {
      const l0 = at(0);
      expect(text(l0, ".limits")).toBe("5h 62 % · 7d 20 %");
      expect(text(l0, ".deliver")).toBe("▶ M5 7/19");
      expect(text(l0, ".panel-button")).toBe("Output");
      expect(text(l0, ".mode-button")).toContain("Input");
      expect(text(l0, ".awake")).toBe("awake");

      const l1 = at(1);
      expect(text(l1, ".limits")).toBe("5h 62 %");
      expect(text(l1, ".deliver")).toBe("▶ M5 7/19");
      expect(text(l1, ".panel-button")).toBe("Output");

      const l2 = at(2);
      expect(text(l2, ".limits")).toBe("5h 62 %");
      expect(text(l2, ".deliver")).toBe("▶ 7/19");
      expect(text(l2, ".panel-button")).toBe("Output");

      const l3 = at(3);
      expect(text(l3, ".deliver")).toBe("▶ 7/19");
      expect(text(l3, ".panel-button")).toBe("");
      expect(text(l3, ".mode-button")).toBe("");
      expect(text(l3, ".awake")).toBe("");
    });

    it("never shortens the waiting count or the connection warning", () => {
      const unreachable: ConnectionState = { state: "unreachable", reason: "no socket" };
      for (const level of [0, 1, 2, 3] as const) {
        const root = at(level, unreachable);
        expect(text(root, ".waiting")).toBe("3 waiting");
        expect(text(root, "[data-testid=connection]")).toBe("poimen unreachable: no socket");
      }
    });

    it("keeps what was dropped in the tooltip", () => {
      const root = at(3);
      const title = (sel: string) =>
        root.querySelector(`${sel} [title], ${sel}[title]`)?.getAttribute("title") ?? "";
      expect(title(".limits")).toContain("7d: 20 %");
      expect(title(".deliver")).toBe("M5: 7/19");
      expect(title(".panel-button")).toContain("output panel");
      expect(title(".mode-button")).toContain("Input");
      expect(title(".awake")).toContain("kept awake");
    });
  });
});
