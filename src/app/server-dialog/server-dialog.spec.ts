// @vitest-environment jsdom
import { readFileSync } from "node:fs";
import { signal } from "@angular/core";
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { beforeAll, describe, expect, it, vi } from "vitest";
import { ServerRestart, type ServerAction } from "../core/server-restart";
import { ServerDialog } from "./server-dialog";

function setup(action: ServerAction | null, terminals: string[] = ["build", "t2"], kept = 0) {
  const restart = {
    action: signal(action),
    terminals: signal(terminals),
    kept: signal(kept),
    busy: signal(false),
    error: signal<string | null>(null),
    confirm: vi.fn(),
    cancel: vi.fn(),
  };
  TestBed.resetTestingModule();
  TestBed.configureTestingModule({ providers: [{ provide: ServerRestart, useValue: restart }] });
  TestBed.overrideComponent(ServerDialog, {
    set: {
      templateUrl: undefined,
      styleUrl: undefined,
      template: readFileSync("src/app/server-dialog/server-dialog.html", "utf8"),
    },
  });
  const fixture = TestBed.createComponent(ServerDialog);
  fixture.detectChanges();
  return { restart, root: fixture.nativeElement as HTMLElement };
}

describe("ServerDialog", () => {
  beforeAll(() => {
    getTestBed().initTestEnvironment(BrowserTestingModule, platformBrowserTesting(), {
      teardown: { destroyAfterEach: true },
    });
  });

  it("renders nothing without a confirmation to ask", () => {
    expect(setup(null).root.querySelector("[role=dialog]")).toBeNull();
  });

  it("names the terminals that end and wires both buttons for a restart", () => {
    const { restart, root } = setup("restart");
    expect(root.textContent).toContain("2 terminals end");
    expect(Array.from(root.querySelectorAll(".terminals li")).map((li) => li.textContent)).toEqual([
      "build",
      "t2",
    ]);
    expect(root.querySelector(".confirm")?.textContent).toContain("Restart server");
    root.querySelector<HTMLButtonElement>(".confirm")!.click();
    expect(restart.confirm).toHaveBeenCalled();
    root.querySelector<HTMLButtonElement>(".cancel")!.click();
    expect(restart.cancel).toHaveBeenCalled();
  });

  it("asks the same for quitting and stopping the server", () => {
    const { root } = setup("quit", ["only"]);
    expect(root.textContent).toContain("Quit poimen and stop");
    expect(root.textContent).toContain("1 terminal ends");
    expect(root.querySelector(".confirm")?.textContent).toContain("Quit and stop server");
  });

  it("says when no terminal is running", () => {
    expect(setup("quit", []).root.textContent).toContain("No terminal is running");
  });

  it("warns of no lost terminal on a restart that keeps them all", () => {
    const { root } = setup("restart", [], 2);
    expect(root.textContent).toContain("2 terminals keep running");
    expect(root.textContent).not.toMatch(/terminals? ends?/);
    expect(root.textContent).not.toContain("No terminal is running");
    expect(root.querySelector(".terminals")).toBeNull();
  });

  it("names only the terminals a restart ends next to the kept ones", () => {
    const { root } = setup("restart", ["old"], 1);
    expect(root.textContent).toContain("1 terminal ends");
    expect(root.textContent).toContain("1 terminal keeps running");
  });
});
