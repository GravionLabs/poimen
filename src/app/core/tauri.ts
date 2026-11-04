import { Injectable } from "@angular/core";
import { Channel, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { open } from "@tauri-apps/plugin-dialog";
import {
  isPermissionGranted,
  onAction,
  registerActionTypes,
  requestPermission,
  sendNotification,
} from "@tauri-apps/plugin-notification";
import { openUrl } from "@tauri-apps/plugin-opener";

/** Stops a listener started with {@link TauriBridge.listen}. */
export type Unlisten = () => void;

/**
 * The action type of every notification. On macOS the notification backend (notify-rust, the
 * NSUserNotificationCenter) only waits for, and so only reports, a click on a notification that
 * has a button; the single "Open" action gives it one. Clicking the button or the notification
 * both reach the click listener.
 */
const NOTIFICATION_ACTION_TYPE = "poimen-open";
const OPEN_ACTION = "open";

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

  /** Shows a system notification, asking for permission once; `false` when it is not allowed. */
  async notify(title: string, body: string, extra?: Record<string, unknown>): Promise<boolean> {
    const granted = (await isPermissionGranted()) || (await requestPermission()) === "granted";
    if (!granted) return false;
    try {
      await registerActionTypes([
        { id: NOTIFICATION_ACTION_TYPE, actions: [{ id: OPEN_ACTION, title: "Open" }] },
      ]);
      sendNotification({ title, body, extra, actionTypeId: NOTIFICATION_ACTION_TYPE });
    } catch (error) {
      console.warn("poimen: could not show the notification", error);
      return false;
    }
    return true;
  }

  /**
   * Calls `handler` with the `extra` of a notification the user clicked. The plugin shows and
   * reports clicks only while a listener exists, so register it before the first notification.
   */
  async onNotificationClick(handler: (extra: Record<string, unknown>) => void): Promise<Unlisten> {
    const listener = await onAction((event) => {
      if (event.actionId === "tap" || event.actionId === OPEN_ACTION)
        handler(event.notification?.extra ?? {});
    });
    return () => void listener.unregister();
  }

  /** Brings the main window to the front: shows it if hidden, restores it if minimized, focuses it. */
  async focusWindow(): Promise<void> {
    const win = getCurrentWindow();
    await win.show();
    await win.unminimize();
    await win.setFocus();
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
