// @vitest-environment jsdom
import { signal } from "@angular/core";
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { SpacesStore, type BackendStatus } from "./spaces-store";
import { HooksOffer, missingHooks, type HookStatus } from "./hooks-offer";
import { TauriBridge } from "./tauri";
import { UI_STATE_KEY, UI_STORAGE, parseLayout } from "./ui-state";

const bridge = { invoke: vi.fn() };
/** A plain rejecting function: vitest reports rejections returned by mocks as unhandled. */
const rejecting = (reason: unknown) => {
  bridge.invoke = (() => Promise.reject(reason)) as unknown as typeof bridge.invoke;
};
const missing: HookStatus[] = [
  { agent: "claude", state: "missing" },
  { agent: "copilot", state: "installed" },
];

function setup(opts: { kind?: "poimen" | "herdr"; stored?: string } = {}) {
  const data = new Map<string, string>();
  if (opts.stored) data.set(UI_STATE_KEY, opts.stored);
  const backend = signal<BackendStatus | null>(null);
  TestBed.resetTestingModule();
  TestBed.configureTestingModule({
    providers: [
      { provide: TauriBridge, useValue: bridge },
      {
        provide: SpacesStore,
        useValue: { backend, backendKind: signal(opts.kind ?? "poimen") },
      },
      {
        provide: UI_STORAGE,
        useValue: {
          getItem: (k: string) => data.get(k) ?? null,
          setItem: (k: string, v: string) => void data.set(k, v),
        },
      },
    ],
  });
  const offer = TestBed.inject(HooksOffer);
  const start = async () => {
    backend.set({ state: "started", reason: null });
    TestBed.tick();
    await vi.waitFor(() => undefined);
    await Promise.resolve();
  };
  return { offer, data, start };
}

describe("missingHooks", () => {
  it("lists missing and partial agents, not installed or unknown ones", () => {
    expect(
      missingHooks([
        { agent: "claude", state: "partial" },
        { agent: "copilot", state: "unknown" },
        { agent: "x", state: "installed" },
        { agent: "y", state: "missing" },
      ]),
    ).toEqual(["claude", "y"]);
    expect(missingHooks([])).toEqual([]);
  });
});

describe("HooksOffer", () => {
  beforeAll(() => {
    getTestBed().initTestEnvironment(BrowserTestingModule, platformBrowserTesting(), {
      teardown: { destroyAfterEach: true },
    });
  });

  beforeEach(() => {
    bridge.invoke = vi.fn();
  });

  it("offers the missing agents once the poimen backend runs", async () => {
    bridge.invoke.mockResolvedValue(missing);
    const { offer, start } = setup();
    expect(offer.visible()).toBe(false);
    await start();
    expect(offer.missing()).toEqual(["claude"]);
    expect(offer.visible()).toBe(true);
    expect(bridge.invoke).toHaveBeenCalledWith("hooks_status");
  });

  it("stays quiet on the herdr backend, when all is installed or the check fails", async () => {
    bridge.invoke.mockResolvedValue(missing);
    const herdr = setup({ kind: "herdr" });
    await herdr.start();
    expect(bridge.invoke).not.toHaveBeenCalled();

    bridge.invoke.mockResolvedValue([{ agent: "claude", state: "installed" }]);
    const done = setup();
    await done.start();
    expect(done.offer.visible()).toBe(false);

    rejecting(new Error("no server"));
    const failed = setup();
    await failed.start();
    expect(failed.offer.visible()).toBe(false);
  });

  it("does not ask again after Not now", async () => {
    bridge.invoke.mockResolvedValue(missing);
    const first = setup();
    await first.start();
    first.offer.dismiss();
    expect(first.offer.visible()).toBe(false);
    expect(parseLayout(first.data.get(UI_STATE_KEY)!).hooksOfferDismissed).toBe(true);

    const again = setup({ stored: first.data.get(UI_STATE_KEY) });
    await again.start();
    expect(again.offer.visible()).toBe(false);
  });

  it("offers again after Not now when installed hooks go missing", async () => {
    bridge.invoke.mockResolvedValue([{ agent: "claude", state: "installed" }]);
    const first = setup();
    await first.start();
    await first.offer.refresh();
    first.offer.dismiss();

    bridge.invoke.mockResolvedValue([{ agent: "claude", state: "missing" }]);
    const again = setup({ stored: first.data.get(UI_STATE_KEY) });
    await again.start();
    await again.offer.refresh();
    expect(again.offer.missing()).toEqual(["claude"]);

    again.offer.dismiss();
    await again.offer.refresh();
    expect(again.offer.visible()).toBe(false);
  });

  it("installs one agent from the settings page and refreshes the states", async () => {
    bridge.invoke.mockResolvedValue(missing);
    const { offer, start } = setup();
    await start();
    offer.dismiss();
    bridge.invoke.mockImplementation(async (cmd: string) =>
      cmd === "hooks_status" ? [{ agent: "claude", state: "installed" }] : undefined,
    );
    await offer.installAgent("claude");
    expect(bridge.invoke).toHaveBeenCalledWith("hooks_install", { agent: "claude" });
    expect(offer.statuses()).toEqual([{ agent: "claude", state: "installed" }]);
  });

  it("installs the offered agents and closes", async () => {
    bridge.invoke.mockResolvedValue(missing);
    const { offer, start } = setup();
    await start();
    bridge.invoke.mockResolvedValue(undefined);
    await offer.install();
    expect(bridge.invoke).toHaveBeenCalledWith("hooks_install", { agent: "claude" });
    expect(bridge.invoke).not.toHaveBeenCalledWith("hooks_install", { agent: "copilot" });
    expect(offer.visible()).toBe(false);
  });

  it("keeps the offer open with the error when the install fails", async () => {
    bridge.invoke.mockResolvedValue(missing);
    const { offer, start } = setup();
    await start();
    rejecting("settings.json is not valid JSON");
    await offer.install();
    expect(offer.visible()).toBe(true);
    expect(offer.error()).toBe("settings.json is not valid JSON");
    expect(offer.busy()).toBe(false);
  });
});
