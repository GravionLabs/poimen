import { Injectable } from "@angular/core";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

/** Stops a listener started with {@link TauriBridge.listen}. */
export type Unlisten = () => void;

/** The only place that talks to the Tauri backend: its commands and its events. */
@Injectable({ providedIn: "root" })
export class TauriBridge {
  invoke<T>(command: string, args?: Record<string, unknown>): Promise<T> {
    return invoke<T>(command, args);
  }

  /** Calls `handler` with the payload of every `event`; resolves once the listener is registered. */
  listen<T>(event: string, handler: (payload: T) => void): Promise<Unlisten> {
    return listen<T>(event, (e) => handler(e.payload));
  }
}
