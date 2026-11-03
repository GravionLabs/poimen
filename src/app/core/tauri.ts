import { Injectable } from "@angular/core";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import {
  isPermissionGranted,
  onAction,
  requestPermission,
  sendNotification,
} from "@tauri-apps/plugin-notification";
import { openUrl } from "@tauri-apps/plugin-opener";

/** Stops a listener started with {@link TauriBridge.listen}. */
export type Unlisten = () => void;

/** The only place that talks to the Tauri backend: its commands and its events. */
@Injectable({ providedIn: "root" })
export class TauriBridge {
  invoke<T>(command: string, args?: Record<string, unknown>): Promise<T> {
    return invoke<T>(command, args);
  }

  /** Opens `url` in the default browser through the opener plugin (callers check the scheme). */
  openUrl(url: string): Promise<void> {
    return openUrl(url);
  }

  /** Shows a system notification, asking for permission once; `false` when it is not allowed. */
  async notify(title: string, body: string, extra?: Record<string, unknown>): Promise<boolean> {
    const granted = (await isPermissionGranted()) || (await requestPermission()) === "granted";
    if (granted) sendNotification({ title, body, extra });
    return granted;
  }

  /**
   * Calls `handler` with the `extra` of a notification the user clicked. The plugin shows and
   * reports clicks only while a listener exists, so register it before the first notification.
   */
  async onNotificationClick(handler: (extra: Record<string, unknown>) => void): Promise<Unlisten> {
    const listener = await onAction((event) => {
      if (event.actionId === "tap") handler(event.notification?.extra ?? {});
    });
    return () => void listener.unregister();
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
