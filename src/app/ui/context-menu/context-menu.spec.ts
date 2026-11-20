// @vitest-environment jsdom
import { Component, signal } from "@angular/core";
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { beforeAll, describe, expect, it, vi } from "vitest";
import { ContextMenu } from "./context-menu";
import type { MenuGroup, MenuPick } from "./menu-model";

@Component({
  imports: [ContextMenu],
  template: `
    <button id="opener" type="button">opener</button>
    @if (open()) {
      <app-context-menu
        label="Sample"
        [at]="{ x: 10, y: 20 }"
        [groups]="groups"
        empty="Nothing"
        (picked)="picked($event)"
        (closed)="closed()"
      />
    }
  `,
})
class Host {
  readonly open = signal(false);
  groups: MenuGroup[] = [
    {
      id: "one",
      items: [
        { id: "a", label: "Alpha" },
        { id: "b", label: "Beta", disabled: "Not now" },
        { id: "c", label: "Gamma" },
      ],
    },
    { id: "two", label: "Plugin", items: [{ id: "d", label: "Delta", checked: true }] },
  ];
  readonly picked = vi.fn((pick: MenuPick) => {
    void pick;
    this.open.set(false);
  });
  readonly closed = vi.fn(() => this.open.set(false));
}

beforeAll(() => {
  getTestBed().initTestEnvironment(BrowserTestingModule, platformBrowserTesting(), {
    teardown: { destroyAfterEach: true },
  });
});

async function mount() {
  TestBed.resetTestingModule();
  const fixture = TestBed.createComponent(Host);
  document.body.append(fixture.nativeElement);
  fixture.detectChanges();
  const opener = fixture.nativeElement.querySelector("#opener") as HTMLElement;
  opener.focus();
  const settle = async () => {
    fixture.detectChanges();
    await fixture.whenStable();
    fixture.detectChanges();
  };
  fixture.componentInstance.open.set(true);
  await settle();
  const root = fixture.nativeElement as HTMLElement;
  const menu = () => root.querySelector<HTMLElement>(".menu");
  const items = () => [...root.querySelectorAll<HTMLElement>('[role^="menuitem"]')];
  const key = (name: string) =>
    (document.activeElement ?? menu())!.dispatchEvent(
      new KeyboardEvent("keydown", { key: name, bubbles: true, cancelable: true }),
    );
  return { fixture, root, opener, menu, items, key, settle, host: fixture.componentInstance };
}

describe("ContextMenu", () => {
  it("takes the focus on open and returns it to the opener on close", async () => {
    const { menu, key, settle, opener } = await mount();
    expect(document.activeElement).toBe(menu());
    key("Escape");
    await settle();
    expect(menu()).toBeNull();
    expect(document.activeElement).toBe(opener);
  });

  it("returns the focus without a ring when a mouse click opened the menu", async () => {
    document.dispatchEvent(new MouseEvent("mousedown"));
    const { key, settle, opener } = await mount();
    key("Escape");
    await settle();
    expect(document.activeElement).toBe(opener);
    expect(opener.hasAttribute("data-pointer-focus")).toBe(true);
  });

  it("returns the focus with its ring when the keyboard opened the menu", async () => {
    document.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter" }));
    const { key, settle, opener } = await mount();
    key("Escape");
    await settle();
    expect(document.activeElement).toBe(opener);
    expect(opener.hasAttribute("data-pointer-focus")).toBe(false);
  });

  it("moves with the arrows (wrapping), Home and End, and picks with Enter", async () => {
    const { items, key, host, settle } = await mount();
    key("ArrowDown");
    expect(document.activeElement).toBe(items()[0]);
    key("ArrowUp");
    expect(document.activeElement).toBe(items()[3]);
    key("Home");
    expect(document.activeElement).toBe(items()[0]);
    key("End");
    expect(document.activeElement).toBe(items()[3]);
    items()[2].focus();
    items()[2].click(); // a native Enter on a focused button clicks it
    await settle();
    expect(host.picked).toHaveBeenCalledWith({ id: "c", shiftKey: false });
  });

  it("shows a disabled entry dimmed with its reason and picks nothing from it", async () => {
    const { items, host, key } = await mount();
    const beta = items()[1];
    expect(beta.getAttribute("aria-disabled")).toBe("true");
    expect(beta.getAttribute("title")).toBe("Not now");
    beta.click();
    expect(host.picked).not.toHaveBeenCalled();
    key("ArrowDown");
    key("ArrowDown");
    expect(document.activeElement).toBe(beta); // focusable, so keyboard users reach the reason
  });

  it("keeps the separator, the group heading and the radio state", async () => {
    const { root, items } = await mount();
    expect(root.querySelectorAll('[role="separator"]')).toHaveLength(1);
    expect(root.querySelector(".menu-heading")?.textContent).toBe("Plugin");
    expect(items()[3].getAttribute("role")).toBe("menuitemradio");
    expect(items()[3].getAttribute("aria-checked")).toBe("true");
  });

  it("closes on a click outside, without picking", async () => {
    const { root, host, menu, settle } = await mount();
    root.querySelector<HTMLElement>(".menu-backdrop")!.click();
    await settle();
    expect(host.closed).toHaveBeenCalledTimes(1);
    expect(host.picked).not.toHaveBeenCalled();
    expect(menu()).toBeNull();
  });

  it("shows the empty text when no entry is left", async () => {
    const { host, root, settle } = await mount();
    host.open.set(false);
    host.groups = [];
    host.open.set(true);
    await settle();
    expect(root.querySelector(".menu-empty")?.textContent).toBe("Nothing");
  });
});
