import { DestroyRef, Injectable, InjectionToken, inject, signal } from "@angular/core";
import type { StatusChange } from "./attention";
import { SettingsStore, type SoundKind } from "./settings";
import { SpacesStore } from "./spaces-store";
import { TauriBridge } from "./tauri";

/** Transitions within this many milliseconds of a played sound do not play another (#724). */
export const SOUND_WINDOW_MS = 2000;

/** The Web Audio context; the one place that creates it is this token's default. */
export const AUDIO_CONTEXT = new InjectionToken<() => AudioContext>("AUDIO_CONTEXT", {
  providedIn: "root",
  factory: () => () => new AudioContext(),
});

interface Tone {
  type: OscillatorType;
  /** Frequencies in Hz, played one after the other. */
  notes: readonly number[];
}

/** The built-in tones: a falling pair asks for you, a rising pair says it is finished. */
export const TONES: Readonly<Record<SoundKind, Tone>> = {
  blocked: { type: "triangle", notes: [740, 587] },
  done: { type: "sine", notes: [523, 784] },
};
const NOTE_SECONDS = 0.14;
const GAIN = 0.2;

/**
 * The transitions that may play a sound: not for a pane being viewed and not for a machine that is
 * not connected (`blockedReason` is null for a pane that is reachable).
 */
export function soundChanges(
  observed: readonly StatusChange[],
  viewed: ReadonlySet<string>,
  blockedReason: (paneId: string) => string | null,
): StatusChange[] {
  return observed.filter((c) => !viewed.has(c.paneId) && blockedReason(c.paneId) === null);
}

/**
 * Sounds for an agent that is blocked or done while not viewed (#724). The only file that touches
 * `AudioContext`. The transitions come from `Attention.observe`; the first snapshot of a backend
 * session is silent, several transitions within 2 s play one sound and blocked wins over done.
 */
@Injectable({ providedIn: "root" })
export class Sounds {
  private readonly bridge = inject(TauriBridge);
  private readonly prefs = inject(SettingsStore);
  private readonly makeContext = inject(AUDIO_CONTEXT);
  private context: AudioContext | undefined;
  private fresh = true;
  private last: { at: number; kind: SoundKind } | undefined;
  private readonly _problem = signal<string | null>(null);
  /** Why the last user file could not be played (the built-in tone played instead). */
  readonly problem = this._problem.asReadonly();

  constructor() {
    const stop = inject(SpacesStore).onSessionStart(() => (this.fresh = true));
    inject(DestroyRef).onDestroy(stop);
  }

  /** Plays the sound for the transitions of one snapshot; `now` is in milliseconds. */
  notify(changes: readonly StatusChange[], now: number = Date.now()): void {
    // Every snapshot passes here, so the first one of a session is consumed even when empty.
    const first = this.fresh;
    this.fresh = false;
    if (first || changes.length === 0) return;
    const sounds = this.prefs.settings().sounds;
    const wanted = changes.filter((c) => sounds[c.status as SoundKind] !== false);
    if (wanted.length === 0) return;
    const kind: SoundKind = wanted.some((c) => c.status === "blocked") ? "blocked" : "done";
    const last = this.last;
    if (last && now - last.at < SOUND_WINDOW_MS && !(kind === "blocked" && last.kind === "done"))
      return;
    this.last = { at: now, kind };
    void this.play(kind);
  }

  /** The "Test" button: plays the configured sound of `kind`, or the built-in tone when it is off. */
  async test(kind: SoundKind): Promise<void> {
    await this.play(kind, true);
  }

  private async play(kind: SoundKind, force = false): Promise<void> {
    const setting = this.prefs.settings().sounds[kind];
    if (setting === false && !force) return;
    try {
      const context = (this.context ??= this.makeContext());
      if (context.state === "suspended") await context.resume();
      if (typeof setting === "string") {
        try {
          const bytes = await this.bridge.invoke<ArrayBuffer>("sound_file", { kind });
          const buffer = await context.decodeAudioData(bytes);
          const source = context.createBufferSource();
          source.buffer = buffer;
          source.connect(context.destination);
          source.start();
          this._problem.set(null);
          return;
        } catch (e) {
          this._problem.set(`Cannot play ${setting}: ${e}`);
          console.warn(`poimen settings: cannot play the ${kind} sound ${setting}: ${e}`);
        }
      } else {
        this._problem.set(null);
      }
      this.tone(context, TONES[kind]);
    } catch (e) {
      console.warn(`poimen: cannot play the ${kind} sound: ${e}`);
    }
  }

  private tone(context: AudioContext, tone: Tone): void {
    tone.notes.forEach((hz, i) => {
      const start = context.currentTime + i * NOTE_SECONDS;
      const osc = context.createOscillator();
      const gain = context.createGain();
      osc.type = tone.type;
      osc.frequency.value = hz;
      gain.gain.setValueAtTime(GAIN, start);
      gain.gain.exponentialRampToValueAtTime(0.001, start + NOTE_SECONDS);
      osc.connect(gain);
      gain.connect(context.destination);
      osc.start(start);
      osc.stop(start + NOTE_SECONDS);
    });
  }
}
