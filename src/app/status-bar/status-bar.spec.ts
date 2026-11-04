// @vitest-environment jsdom
import { readFileSync } from "node:fs";
import { Component } from "@angular/core";
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { beforeAll, describe, expect, it } from "vitest";
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
  const render = (mode: "control" | "observe", blocked: string | null, panelOpen = true) => {
    const emitted: boolean[] = [];
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
      update = () => ({ kind: "idle" }) as const;
      detailsOpen = () => false;
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
      root: fixture.nativeElement as HTMLElement,
      shown: () => shown,
      opened: () => opened,
    };
  };

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
