// @vitest-environment jsdom
import { readFileSync } from "node:fs";
import { Component, type WritableSignal } from "@angular/core";
import { TestBed, getTestBed } from "@angular/core/testing";
import { HxStatusBar, HxStatusItem } from "@gravionlabs/helix-desktop";
import { HxButton } from "@gravionlabs/helix-ui";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { beforeAll, describe, expect, it } from "vitest";
import { Icon } from "../ui/icon/icon";
import { SERVER_RESTART_NOTE, type UpdateState } from "../core/updater";
import type { BarSummary } from "../deliver/deliver-status";
import {
  CHANNEL_TITLE,
  StatusBar,
  connectionLabel,
  modeLabel,
  modeTitle,
  sizeLabel,
} from "./status-bar";

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
  ) => {
    const emitted: boolean[] = [];
    const serverClicks: string[] = [];
    let deliverClicks = 0;
    let shown = 0;
    let opened = 0;
    let closed = 0;
    @Component({
      selector: "app-host",
      imports: [HxStatusBar, HxStatusItem, HxButton, Icon],
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
      channel = () => channel;
      channelTitle = CHANNEL_TITLE;
      update = () => update;
      detailsOpen = () => update.kind === "available";
      closeDetails = () => closed++;
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
      limits = () => null;
      showDeliver = { emit: () => deliverClicks++ };
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
      closed: () => closed,
    };
  };

  it("closes the update details with the close button", () => {
    const { root, closed } = render("control", null, true, null, null, {
      kind: "available",
      info: { version: "1.2.3", notes: "n" },
    } as UpdateState);
    const close = root.querySelector<HTMLButtonElement>(".update-details .close-details")!;
    expect(close.getAttribute("aria-label")).toBe("Close");
    close.click();
    expect(closed()).toBe(1);
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
});

describe("update details popover", () => {
  const make = () => {
    TestBed.resetTestingModule();
    const bar = TestBed.runInInjectionContext(() => new StatusBar());
    const open = bar as unknown as { detailsOpen: WritableSignal<boolean> };
    open.detailsOpen.set(true);
    return { bar, open };
  };

  it("has an opaque background from a token other popups use", () => {
    const css = readFileSync("src/app/status-bar/status-bar.css", "utf8");
    const rule = /\.update-details \{[^}]*\}/.exec(css)![0];
    expect(rule).toContain("background: var(--h-content-background);");
    expect(rule).not.toContain("--h-surface-color");
  });

  it("closes on Escape", () => {
    const { bar, open } = make();
    bar.closeDetails();
    expect(open.detailsOpen()).toBe(false);
  });

  it("closes on a click outside", () => {
    const { bar, open } = make();
    bar.onDocumentClick({ target: document.body } as unknown as Event);
    expect(open.detailsOpen()).toBe(false);
  });

  it("stays open on a click inside the details or on the update button", () => {
    const { bar, open } = make();
    for (const cls of ["update-details", "update-button"]) {
      const el = document.createElement("div");
      el.className = cls;
      const inner = document.createElement("span");
      el.append(inner);
      bar.onDocumentClick({ target: inner } as unknown as Event);
      expect(open.detailsOpen()).toBe(true);
    }
  });
});
