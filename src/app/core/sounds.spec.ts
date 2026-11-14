import { Injector, signal } from "@angular/core";
import { describe, expect, it, vi } from "vitest";
import type { StatusChange } from "./attention";
import { SettingsStore, DEFAULT_SETTINGS, type SoundSettings } from "./settings";
import { AUDIO_CONTEXT, SOUND_WINDOW_MS, Sounds } from "./sounds";
import { SpacesStore } from "./spaces-store";
import { TauriBridge } from "./tauri";

const blocked: StatusChange = { paneId: "a", status: "blocked" };
const done: StatusChange = { paneId: "b", status: "done" };

function setup(
  sounds: SoundSettings,
  fileBytes: () => Promise<ArrayBuffer> = async () => new ArrayBuffer(4),
) {
  const oscillators: { frequency: { value: number }; type: string }[] = [];
  const sources: unknown[] = [];
  const context = {
    currentTime: 0,
    state: "running",
    destination: {},
    resume: vi.fn(async () => {}),
    createOscillator: () => {
      const osc = {
        type: "",
        frequency: { value: 0 },
        connect: vi.fn(),
        start: vi.fn(),
        stop: vi.fn(),
      };
      oscillators.push(osc);
      return osc;
    },
    createGain: () => ({
      gain: { setValueAtTime: vi.fn(), exponentialRampToValueAtTime: vi.fn() },
      connect: vi.fn(),
    }),
    decodeAudioData: vi.fn(async () => ({ decoded: true })),
    createBufferSource: () => {
      const source = { buffer: null, connect: vi.fn(), start: vi.fn() };
      sources.push(source);
      return source;
    },
  };
  const invoke = vi.fn(fileBytes);
  let sessionStart: () => void = () => {};
  const injector = Injector.create({
    providers: [
      { provide: AUDIO_CONTEXT, useValue: () => context },
      { provide: TauriBridge, useValue: { invoke } },
      { provide: SettingsStore, useValue: { settings: signal({ ...DEFAULT_SETTINGS, sounds }) } },
      {
        provide: SpacesStore,
        useValue: {
          onSessionStart: (l: () => void) => {
            sessionStart = l;
            return () => {};
          },
        },
      },
      { provide: Sounds, useClass: Sounds },
    ],
  });
  const sounds$ = injector.get(Sounds);
  // The first snapshot of the session is silent; consume it.
  sounds$.notify([]);
  const freqs = () => oscillators.map((o) => o.frequency.value);
  return {
    sounds: sounds$,
    context,
    oscillators,
    sources,
    invoke,
    freqs,
    startSession: () => sessionStart(),
  };
}

describe("Sounds", () => {
  it("plays nothing while the setting is off", () => {
    const t = setup({ blocked: false, done: false });
    t.sounds.notify([blocked], 10_000);
    expect(t.oscillators).toEqual([]);
  });

  it("plays the built-in tone of the transition, distinct for blocked and done", () => {
    const t = setup({ blocked: true, done: true });
    t.sounds.notify([blocked], 10_000);
    const first = t.freqs();
    expect(first).toEqual([740, 587]);
    t.sounds.notify([done], 20_000);
    expect(t.freqs().slice(first.length)).toEqual([523, 784]);
  });

  it("is silent for the first snapshot of a session, also after a reconnect", () => {
    const t = setup({ blocked: true, done: true });
    t.startSession();
    t.sounds.notify([blocked], 10_000);
    expect(t.oscillators).toEqual([]);
    t.sounds.notify([blocked], 20_000);
    expect(t.oscillators.length).toBe(2);
  });

  it("plays one sound for transitions within 2 s and blocked wins over done", () => {
    const t = setup({ blocked: true, done: true });
    t.sounds.notify([done, blocked], 10_000);
    expect(t.freqs()).toEqual([740, 587]);
    t.sounds.notify([done], 10_000 + SOUND_WINDOW_MS - 1);
    t.sounds.notify([blocked], 10_000 + SOUND_WINDOW_MS - 1);
    expect(t.oscillators.length).toBe(2);
    t.sounds.notify([done], 10_000 + SOUND_WINDOW_MS);
    expect(t.oscillators.length).toBe(4);
  });

  it("lets a blocked transition follow a done sound inside the window", () => {
    const t = setup({ blocked: true, done: true });
    t.sounds.notify([done], 10_000);
    t.sounds.notify([blocked], 10_500);
    expect(t.freqs()).toEqual([523, 784, 740, 587]);
  });

  it("plays a user file read through sound_file", async () => {
    const t = setup({ blocked: "~/b.wav", done: false });
    t.sounds.notify([blocked], 10_000);
    await vi.waitFor(() => expect(t.sources.length).toBe(1));
    expect(t.invoke).toHaveBeenCalledWith("sound_file", { kind: "blocked" });
    expect(t.oscillators).toEqual([]);
    expect(t.sounds.problem()).toBeNull();
  });

  it("falls back to the built-in tone and reports a file that cannot be read", async () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    const t = setup({ blocked: "~/b.wav", done: false }, async () => {
      throw new Error("larger than 1 MiB");
    });
    t.sounds.notify([blocked], 10_000);
    await vi.waitFor(() => expect(t.freqs()).toEqual([740, 587]));
    expect(t.sounds.problem()).toContain("larger than 1 MiB");
    warn.mockRestore();
  });

  it("Test plays the tone even when the sound is off", async () => {
    const t = setup({ blocked: false, done: false });
    await t.sounds.test("done");
    expect(t.freqs()).toEqual([523, 784]);
  });

  it("resumes a suspended context", async () => {
    const t = setup({ blocked: true, done: true });
    t.context.state = "suspended";
    await t.sounds.test("done");
    expect(t.context.resume).toHaveBeenCalled();
  });
});
