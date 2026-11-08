// @vitest-environment jsdom
import { readFileSync } from "node:fs";
import { Component } from "@angular/core";
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { beforeAll, describe, expect, it } from "vitest";
import { SERVER_RESTART_NOTE, type UpdateState } from "../core/updater";
import { figures, usageTitle, type UsageReport } from "../core/usage";
import type { BarSummary } from "../deliver/deliver-status";
import { connectionLabel, modeLabel, modeTitle, sizeLabel } from "./status-bar";

beforeAll(() => {
  try {
    getTestBed().initTestEnvironment(BrowserTestingModule, platformBrowserTesting());
  } catch {
    // already initialized by the shared setup
  }
});

describe("connectionLabel", () => {
  it("names a connected herdr", () => {
    expect(connectionLabel({ state: "connected" })).toBe("herdr connected");
  });
  it("shows the reason when reconnecting", () => {
    expect(connectionLabel({ state: "reconnecting", reason: "socket closed" })).toBe(
      "herdr reconnecting: socket closed",
    );
  });
  it("shows the reason when unreachable, and copes with an empty one", () => {
    expect(connectionLabel({ state: "unreachable", reason: "no socket" })).toBe(
      "herdr unreachable: no socket",
    );
    expect(connectionLabel({ state: "unreachable", reason: "" })).toBe("herdr unreachable");
  });
});

describe("connectionLabel on the poimen backend", () => {
  it("names the backend instead of herdr", () => {
    expect(connectionLabel({ state: "connected" }, "poimen")).toBe("poimen connected");
    expect(connectionLabel({ state: "reconnecting", reason: "gone" }, "poimen")).toBe(
      "poimen reconnecting: gone",
    );
    expect(connectionLabel({ state: "unreachable", reason: "" }, "poimen")).toBe(
      "poimen unreachable",
    );
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
    usage: UsageReport | null = null,
    deliver: BarSummary | null = null,
    server: string | null = null,
    update: UpdateState = { kind: "idle" },
  ) => {
    const emitted: boolean[] = [];
    const serverClicks: string[] = [];
    let deliverClicks = 0;
    let shown = 0;
    let opened = 0;
    @Component({
      selector: "app-host",
      template: readFileSync("src/app/status-bar/status-bar.html", "utf8"),
    })
    class Host {
      connection = () => ({ state: "connected" }) as const;
      waiting = () => 0;
      paneId = () => "p1";
      mode = () => mode;
      blocked = () => blocked;
      size = () => undefined;
      appVersion = () => null;
      update = () => update;
      detailsOpen = () => update.kind === "available";
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
      herdrLabel = () => "";
      warning = () => "";
      backend = () => "herdr";
      usage = () => usage;
      deliver = () => deliver;
      showDeliver = { emit: () => deliverClicks++ };
      usageFigures = figures;
      usageText = usageTitle;
    }
    TestBed.resetTestingModule();
    const fixture = TestBed.createComponent(Host);
    fixture.detectChanges();
    const button = (fixture.nativeElement as HTMLElement).querySelector<HTMLButtonElement>(
      "button.mode-button",
    )!;
    return {
      button,
      emitted,
      serverClicks,
      root: fixture.nativeElement as HTMLElement,
      shown: () => shown,
      opened: () => opened,
      deliverClicks: () => deliverClicks,
    };
  };

  it("shows the running deliver loops and opens the panel on a click", () => {
    const { root, deliverClicks } = render("control", null, true, null, {
      text: "▶ M5 7/19",
      title: "o/n M5 7/19",
      warning: false,
    });
    const item = root.querySelector<HTMLButtonElement>("button.deliver")!;
    expect(item.textContent?.trim()).toBe("▶ M5 7/19");
    expect(item.classList.contains("warn")).toBe(false);
    item.click();
    expect(deliverClicks()).toBe(1);
  });

  it("marks the deliver item as a warning and hides it without a run", () => {
    const warned = render("control", null, true, null, {
      text: "⚠ M5 7/19",
      title: "",
      warning: true,
    });
    expect(warned.root.querySelector("button.deliver")!.classList.contains("warn")).toBe(true);
    expect(render("control", null).root.querySelector("button.deliver")).toBeNull();
  });

  it("offers to show the closed output panel, with its shortcut", () => {
    const { root, shown } = render("control", null, false);
    const panel = root.querySelector<HTMLButtonElement>("button.panel-button")!;
    expect(panel.textContent?.trim()).toBe("Output");
    expect(panel.title).toBe("Show the output panel (⌘J)");
    panel.click();
    expect(shown()).toBe(1);
  });

  it("has an accessible ? button that opens the shortcuts overview", () => {
    const { root, opened } = render("control", null);
    const help = root.querySelector<HTMLButtonElement>("button.help-button")!;
    expect(help.textContent?.trim()).toBe("?");
    expect(help.getAttribute("aria-label")).toBe("Keyboard shortcuts");
    expect(help.getAttribute("aria-haspopup")).toBe("dialog");
    expect(help.title).toBe("Keyboard shortcuts (⌘/)");
    help.click();
    expect(opened()).toBe(1);
  });

  it("hides the server notice while the server is fine", () => {
    expect(render("control", null).root.querySelector("[data-testid=server-notice]")).toBeNull();
  });

  it("shows the server notice with Restart server and Later", () => {
    const text = "poimen-server 0.1.0 is running, 0.2.0 is installed";
    const { root, serverClicks } = render("control", null, true, null, null, text);
    expect(root.querySelector("[data-testid=server-notice]")?.textContent).toContain(text);
    root.querySelector<HTMLButtonElement>("[data-testid=restart-server]")!.click();
    root.querySelector<HTMLButtonElement>("[data-testid=later-server]")!.click();
    expect(serverClicks).toEqual(["restart", "later"]);
  });

  it("says before the install that restarting the server ends the terminals", () => {
    const info = { version: "9.9.9", currentVersion: "0.1.0", notes: null, date: null };
    const { root } = render("control", null, true, null, null, null, {
      kind: "available",
      info,
    });
    const note = root.querySelector("[data-testid=restart-note]")?.textContent ?? "";
    expect(note).toContain("restarting ends all terminals");
  });

  it("ends with the versions, the update link and the ? button, in this order", () => {
    const { root } = render("control", null);
    const items = Array.from(root.querySelectorAll(".bar > .item"));
    const kinds = items.map((el) =>
      el.classList.contains("help-button")
        ? "help"
        : el.classList.contains("update-button")
          ? "update"
          : el.classList.contains("version") && !el.classList.contains("notice")
            ? "version"
            : "other",
    );
    expect(kinds.slice(-4)).toEqual(["version", "version", "update", "help"]);
    expect(items.at(-1)?.classList.contains("help-button")).toBe(true);
    expect(items.at(-2)?.textContent?.trim()).toBe("Check for updates");
  });

  it("puts the pane id, mode and size before the versions", () => {
    const { root } = render("control", null);
    const items = Array.from(root.querySelectorAll(".bar > .item"));
    const mode = items.findIndex((el) => el.classList.contains("mode-button"));
    const firstVersion = items.findIndex(
      (el) => el.classList.contains("version") && !el.classList.contains("notice"),
    );
    expect(mode).toBeGreaterThan(-1);
    expect(mode).toBeLessThan(firstVersion);
  });

  it("separates items with a bar that is no element of its own", () => {
    const { root } = render("control", null);
    expect(root.querySelectorAll(".bar > .item").length).toBeGreaterThan(4);
    // CSS draws the bar (.item + .item::before), so a screen reader never meets it.
    expect(root.textContent).not.toContain("|");
  });

  it("shows the usage next to the connection, colored, with the tooltip", () => {
    const report: UsageReport = {
      schema_version: 1,
      generated_at: "2026-11-03T10:05:00Z",
      providers: [
        {
          provider: "claude",
          status: "ok",
          windows: [
            {
              window: "five_hour",
              unit: "percent",
              used: 12,
              limit: 100,
              resets_at: "2026-11-03T13:30:00Z",
            },
            {
              window: "seven_day",
              unit: "percent",
              used: 85,
              limit: 100,
              resets_at: "2026-11-07T12:00:00Z",
            },
          ],
        },
      ],
    };
    const { root } = render("control", null, true, report);
    const item = root.querySelector<HTMLElement>(".bar > .item.usage")!;
    expect(item.textContent?.replace(/\s+/g, " ").trim()).toBe("5h 12%·7d 85%");
    expect(item.querySelector(".figure.green")?.textContent).toBe("5h 12%");
    expect(item.querySelector(".figure.red")?.textContent).toBe("7d 85%");
    expect(item.title).toContain("5h resets");
    const items = Array.from(root.querySelectorAll(".bar > .item"));
    expect(items[0].classList.contains("conn")).toBe(true);
    expect(items[1]).toBe(item);
  });

  it("hides the usage item without a report", () => {
    const { root } = render("control", null);
    expect(root.querySelector(".usage")).toBeNull();
  });

  it("hides the panel button while the panel is open", () => {
    const { root } = render("control", null, true);
    expect(root.querySelector("button.panel-button")).toBeNull();
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
});
