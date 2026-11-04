// @vitest-environment jsdom
import { readFileSync } from "node:fs";
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { beforeAll, describe, expect, it } from "vitest";
import { NotificationCenter } from "../core/notification-center";
import { Notifications } from "./notifications";

describe("Notifications", () => {
  beforeAll(() => {
    getTestBed().initTestEnvironment(BrowserTestingModule, platformBrowserTesting());
  });

  const setup = () => {
    TestBed.resetTestingModule();
    TestBed.overrideComponent(Notifications, {
      set: {
        templateUrl: undefined,
        styleUrl: undefined,
        template: readFileSync("src/app/notifications/notifications.html", "utf8"),
      },
    });
    const fixture = TestBed.createComponent(Notifications);
    const center = TestBed.inject(NotificationCenter);
    const chosen: string[] = [];
    fixture.componentInstance.paneChosen.subscribe((id) => chosen.push(id));
    return { fixture, center, chosen, el: fixture.nativeElement as HTMLElement };
  };

  it("shows a hint while empty", async () => {
    const { fixture, el } = setup();
    await fixture.whenStable();
    expect(el.textContent).toContain("No notifications yet");
  });

  it("lists entries newest first", async () => {
    const { fixture, center, el } = setup();
    center.record([{ paneId: "p1", status: "done" }], () => "one", 1);
    center.record([{ paneId: "p2", status: "blocked" }], () => "two", 2);
    await fixture.whenStable();
    const messages = [...el.querySelectorAll(".message")].map((m) => m.textContent);
    expect(messages).toEqual(["two is waiting for you", "one is done"]);
  });

  it("a click marks the entry read and chooses its pane", async () => {
    const { fixture, center, chosen, el } = setup();
    center.record([{ paneId: "p1", status: "done" }], () => "one", 1);
    await fixture.whenStable();
    el.querySelector<HTMLButtonElement>(".entry")!.click();
    expect(chosen).toEqual(["p1"]);
    expect(center.unread()).toBe(0);
  });

  it("mark all read clears the unread count", async () => {
    const { fixture, center, el } = setup();
    center.record([{ paneId: "p1", status: "done" }], () => "one", 1);
    await fixture.whenStable();
    el.querySelector<HTMLButtonElement>(".mark")!.click();
    expect(center.unread()).toBe(0);
  });
});
