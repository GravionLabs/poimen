// @vitest-environment jsdom
import { Component, signal } from "@angular/core";
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { beforeAll, describe, expect, it, vi } from "vitest";
import { DialogShell, type DialogSize } from "./dialog-shell";
import { dialogOf, expectFocusInside, focusOpener, press } from "./dialog-contract";

@Component({
  imports: [DialogShell],
  template: `
    @if (open()) {
      <app-dialog-shell
        heading="Sample"
        [size]="size()"
        [width]="width()"
        [cancelDefault]="cancelDefault()"
        (cancelled)="cancelled()"
        (confirmed)="confirmed()"
      >
        @if (field()) {
          <input class="field" />
        }
        <div class="dialog-buttons">
          <button type="button" class="cancel">Cancel</button>
          <button type="button" class="confirm">Confirm</button>
        </div>
      </app-dialog-shell>
    }
  `,
})
class Host {
  readonly open = signal(true);
  readonly size = signal<DialogSize>("md");
  readonly width = signal<string | null>(null);
  readonly field = signal(true);
  readonly cancelDefault = signal(false);
  readonly cancelled = vi.fn();
  readonly confirmed = vi.fn();
}

beforeAll(() => {
  getTestBed().initTestEnvironment(BrowserTestingModule, platformBrowserTesting(), {
    teardown: { destroyAfterEach: true },
  });
});

async function mount(setup: (host: Host) => void = () => undefined) {
  TestBed.resetTestingModule();
  const opener = focusOpener();
  const fixture = TestBed.createComponent(Host);
  setup(fixture.componentInstance);
  const settle = async () => {
    fixture.detectChanges();
    await fixture.whenStable();
    fixture.detectChanges();
  };
  await settle();
  return {
    fixture,
    host: fixture.componentInstance,
    root: fixture.nativeElement as HTMLElement,
    opener,
    settle,
  };
}

describe("DialogShell", () => {
  it("is a modal dialog named by its heading, with one close icon button", async () => {
    const { root } = await mount();
    const dialog = dialogOf(root);
    expect(dialog.getAttribute("role")).toBe("dialog");
    expect(dialog.getAttribute("aria-label")).toBe("Sample");
    expect(dialog.hasAttribute("open")).toBe(true);
    expect(root.querySelector("h2")?.textContent).toBe("Sample");
    const close = root.querySelectorAll<HTMLButtonElement>("button.dialog-close");
    expect(close).toHaveLength(1);
    expect(close[0].getAttribute("aria-label")).toBe("Close");
  });

  it.each(["sm", "md", "lg"] as const)("takes the width of the %s token", async (size) => {
    const { root } = await mount((h) => h.size.set(size));
    expect(dialogOf(root).style.width).toBe(`min(var(--h-dialog-${size}-width), 92vw)`);
  });

  it("is md without a size", async () => {
    TestBed.resetTestingModule();
    const fixture = TestBed.createComponent(DialogShell);
    fixture.detectChanges();
    await fixture.whenStable();
    expect(dialogOf(fixture.nativeElement).style.width).toBe("min(var(--h-dialog-md-width), 92vw)");
  });

  it("lets width override the size class, and returns to it", async () => {
    const { root, host, settle } = await mount((h) => {
      h.size.set("lg");
      h.width.set("360px");
    });
    expect(dialogOf(root).style.width).toBe("min(360px, 92vw)");
    host.width.set(null);
    await settle();
    expect(dialogOf(root).style.width).toBe("min(var(--h-dialog-lg-width), 92vw)");
  });

  it("moves the focus to the first field", async () => {
    const { root } = await mount();
    expectFocusInside(root);
    expect(document.activeElement?.localName).toBe("input");
  });

  it("without a field the confirming button gets the focus, the cancelling one when Cancel is the default", async () => {
    const plain = await mount((h) => h.field.set(false));
    expect(document.activeElement?.className).toContain("confirm");
    const safe = await mount((h) => {
      h.field.set(false);
      h.cancelDefault.set(true);
    });
    expect(document.activeElement?.className).toContain("cancel");
    expect(plain.root).not.toBe(safe.root);
  });

  it("Escape cancels without a click first", async () => {
    const { host } = await mount();
    const event = press("Escape");
    expect(host.cancelled).toHaveBeenCalledTimes(1);
    expect(event.defaultPrevented).toBe(true);
  });

  it("Enter in a field confirms; on a button it is left to the button", async () => {
    const { host, root } = await mount();
    press("Enter");
    expect(host.confirmed).toHaveBeenCalledTimes(1);
    root.querySelector<HTMLButtonElement>(".cancel")!.focus();
    press("Enter");
    expect(host.confirmed).toHaveBeenCalledTimes(1);
  });

  it("Enter does not confirm when Cancel is the default", async () => {
    const { host } = await mount((h) => h.cancelDefault.set(true));
    press("Enter");
    expect(host.confirmed).not.toHaveBeenCalled();
  });

  it("leaves a key the content handled itself", async () => {
    const { host, root } = await mount();
    root.querySelector("input")!.addEventListener("keydown", (e) => e.preventDefault());
    press("Escape");
    press("Enter");
    expect(host.cancelled).not.toHaveBeenCalled();
    expect(host.confirmed).not.toHaveBeenCalled();
  });

  it("keeps Tab inside the dialog", async () => {
    const { root } = await mount();
    const dialog = dialogOf(root);
    const items = [...dialog.querySelectorAll<HTMLElement>("button, input")];
    items[items.length - 1].focus();
    const forward = press("Tab");
    expect(forward.defaultPrevented).toBe(true);
    expect(document.activeElement).toBe(items[0]);
    const back = press("Tab", { shiftKey: true });
    expect(back.defaultPrevented).toBe(true);
    expect(document.activeElement).toBe(items[items.length - 1]);
  });

  it("cancels on a click on the backdrop, not inside", async () => {
    const { host, root } = await mount();
    root.querySelector("h2")!.click();
    expect(host.cancelled).not.toHaveBeenCalled();
    dialogOf(root).click();
    expect(host.cancelled).toHaveBeenCalledTimes(1);
  });

  it("the close icon button cancels", async () => {
    const { host, root } = await mount();
    root.querySelector<HTMLButtonElement>(".dialog-close")!.click();
    expect(host.cancelled).toHaveBeenCalledTimes(1);
  });

  it("gives the focus back to the element that opened it", async () => {
    const { host, opener, settle } = await mount();
    expect(document.activeElement).not.toBe(opener);
    host.open.set(false);
    await settle();
    expect(document.activeElement).toBe(opener);
  });

  it("returns the focus without a ring after a mouse open, with it after a key", async () => {
    document.dispatchEvent(new MouseEvent("mousedown"));
    const a = await mount();
    a.host.open.set(false);
    await a.settle();
    expect(a.opener.hasAttribute("data-pointer-focus")).toBe(true);
    document.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter" }));
    const b = await mount();
    b.host.open.set(false);
    await b.settle();
    expect(b.opener.hasAttribute("data-pointer-focus")).toBe(false);
  });
});
