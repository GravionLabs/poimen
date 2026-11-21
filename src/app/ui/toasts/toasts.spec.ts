// @vitest-environment jsdom
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { HxMessageService } from "@gravionlabs/helix-ui";
import { beforeAll, describe, expect, it } from "vitest";
import { Toasts } from "./toasts";

describe("Toasts", () => {
  beforeAll(() => {
    try {
      getTestBed().initTestEnvironment(BrowserTestingModule, platformBrowserTesting());
    } catch {
      // already initialized
    }
  });

  function setup() {
    TestBed.resetTestingModule();
    TestBed.configureTestingModule({ providers: [HxMessageService] });
    const fixture = TestBed.createComponent(Toasts);
    fixture.detectChanges();
    const layer = fixture.nativeElement.querySelector(".layer") as HTMLElement;
    return { fixture, layer, messages: TestBed.inject(HxMessageService) };
  }

  it("shows a message in the top layer, so a modal dialog does not cover it", async () => {
    const { fixture, layer, messages } = setup();
    messages.add({ severity: "error", summary: "Cannot save the theme: disk full" });
    fixture.detectChanges();
    await fixture.whenStable();
    expect(fixture.nativeElement.textContent).toContain("Cannot save the theme: disk full");
    if (typeof layer.showPopover === "function") expect(layer.matches(":popover-open")).toBe(true);
  });

  it("hides the layer when the last message is gone", async () => {
    const { fixture, layer, messages } = setup();
    const id = messages.add({ summary: "Copied" });
    fixture.detectChanges();
    await fixture.whenStable();
    messages.remove(id);
    fixture.detectChanges();
    await fixture.whenStable();
    expect(fixture.nativeElement.textContent).not.toContain("Copied");
    if (typeof layer.showPopover === "function") expect(layer.matches(":popover-open")).toBe(false);
  });
});
