import { Injectable } from "@angular/core";
import { DemoBackend, DemoChannel } from "./backend";

/** Stops a listener. */
export type Unlisten = () => void;

/** A drag-and-drop event of the webview (never emitted in the browser build). */
export type DragDrop =
  | { type: "enter"; paths: string[]; position: { x: number; y: number } }
  | { type: "over"; position: { x: number; y: number } }
  | { type: "drop"; paths: string[]; position: { x: number; y: number } }
  | { type: "leave" };

/**
 * The browser build's `TauriBridge` (an Angular `fileReplacements` entry swaps it for
 * `src/app/core/tauri.ts`): same methods, answered by the one {@link DemoBackend} of the page.
 */
@Injectable({ providedIn: "root" })
export class DemoBridge {
  /** The backend of the page, set by `tauri.demo.ts` before the app starts. */
  static backend = new DemoBackend();

  invoke<T>(command: string, args?: Record<string, unknown>): Promise<T> {
    return DemoBridge.backend.invoke<T>(command, args);
  }

  invokeRaw<T>(command: string, body: Uint8Array, headers: Record<string, string>): Promise<T> {
    return DemoBridge.backend.invoke<T>(command, { body, headers });
  }

  channel<T>(onMessage: (message: T) => void): DemoChannel<T> {
    const channel = new DemoChannel<T>();
    channel.onmessage = onMessage;
    return channel;
  }

  openUrl(_url: string): Promise<void> {
    return Promise.resolve();
  }

  setBadgeCount(_count: number | undefined): Promise<void> {
    return Promise.resolve();
  }

  pickDirectory(): Promise<string | null> {
    return Promise.resolve(null);
  }

  pickJsonFile(): Promise<string | null> {
    return Promise.resolve(null);
  }

  pickSaveFile(_defaultPath: string): Promise<string | null> {
    return Promise.resolve(null);
  }

  listen<T>(event: string, handler: (payload: T) => void): Promise<Unlisten> {
    return Promise.resolve(DemoBridge.backend.listen(event, handler as (p: unknown) => void));
  }

  onDragDrop(_handler: (event: DragDrop) => void): Promise<Unlisten> {
    return Promise.resolve(() => {});
  }
}
