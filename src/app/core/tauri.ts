import { Injectable } from "@angular/core";
import { Channel, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { open } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";

/** Stops a listener started with {@link TauriBridge.listen}. */
export type Unlisten = () => void;

/** The only place that talks to the Tauri backend: its commands and its events. */
@Injectable({ providedIn: "root" })
export class TauriBridge {
  invoke<T>(command: string, args?: Record<string, unknown>): Promise<T> {
    return invoke<T>(command, args);
  }

  /**
   * A channel for a command to push messages through (`onMessage` runs for each, in order). JSON
   * messages arrive as values; raw bytes as an `ArrayBuffer`.
   */
  channel<T>(onMessage: (message: T) => void): Channel<T> {
    const channel = new Channel<T>();
    channel.onmessage = onMessage;
    return channel;
  }

  /** Opens `url` in the default browser through the opener plugin (callers check the scheme). */
  openUrl(url: string): Promise<void> {
    return openUrl(url);
  }

  /** Sets the dock badge of the app; `undefined` clears it. Rejects where badges are unsupported. */
  setBadgeCount(count: number | undefined): Promise<void> {
    return getCurrentWindow().setBadgeCount(count);
  }

  /** The native folder picker; the chosen directory, or `null` when cancelled. */
  async pickDirectory(): Promise<string | null> {
    const dir = await open({ directory: true, multiple: false });
    return typeof dir === "string" ? dir : null;
  }

  /** Calls `handler` with the payload of every `event`; resolves once the listener is registered. */
  listen<T>(event: string, handler: (payload: T) => void): Promise<Unlisten> {
    return listen<T>(event, (e) => handler(e.payload));
  }
}
