import { Injectable, inject, signal } from "@angular/core";
import { HX_UPDATE_BACKEND, type HxUpdateBackend } from "@gravionlabs/helix-desktop/tauri";
import { TauriBridge } from "./tauri";

/** A newer release than the running app (wire shape of `updater.rs`). */
export interface UpdateInfo {
  version: string;
  currentVersion?: string;
  notes: string | null;
  date: string | null;
}

/**
 * poimen's update backend (#932): the commands `update_check` and `update_install` instead of the
 * JS updater plugin. `update_install` installs and restarts in one command, so `relaunch` has
 * nothing left to do.
 */
export function poimenUpdateBackend(bridge: TauriBridge): HxUpdateBackend {
  return {
    check: () => bridge.invoke<UpdateInfo | null>("update_check"),
    install: () => bridge.invoke<void>("update_install"),
    relaunch: () => Promise.resolve(),
  };
}

/** Provides {@link poimenUpdateBackend} as `HX_UPDATE_BACKEND`. */
export const POIMEN_UPDATE_BACKEND_PROVIDER = {
  provide: HX_UPDATE_BACKEND,
  useFactory: () => poimenUpdateBackend(inject(TauriBridge)),
};

/** Which step failed: the check, or the download and install. */
export type UpdateStep = "check" | "install";

export type UpdateState =
  | { kind: "idle" }
  | { kind: "checking" }
  | { kind: "current" }
  | { kind: "available"; info: UpdateInfo }
  | { kind: "installing"; info: UpdateInfo }
  /** `info` is the update that stays available after a failed install; `null` after a failed check. */
  | { kind: "error"; step: UpdateStep; message: string; info: UpdateInfo | null };

/**
 * Shown with the update before it is installed: the update replaces the app, not the running
 * server, but restarting the server (offered afterwards when it is older) ends every terminal.
 */
export const SERVER_RESTART_NOTE =
  "poimen-server keeps your terminals running during the update. A newer server only starts when you restart it, and restarting ends all terminals.";

/** The headline of a failed step. */
export function errorTitle(step: UpdateStep): string {
  return step === "check" ? "Update check failed" : "Update failed";
}

/** An error value as text. */
function errorText(error: unknown): string {
  if (typeof error === "string") return error;
  return error instanceof Error ? error.message : JSON.stringify(error);
}

/**
 * Self-update from the latest GitHub release, through `HX_UPDATE_BACKEND`. `HxUpdateService` is not
 * used: after a failed install it drops the found update (a retry would need a new check) and it
 * reads version and channel synchronously. It only talks to the network when the user asks
 * (`check`); `install` downloads, verifies, installs and restarts (the app does not come back
 * from a successful install). After a failed install the update stays available for a retry.
 */
@Injectable({ providedIn: "root" })
export class Updater {
  private readonly backend = inject(HX_UPDATE_BACKEND);
  private readonly _state = signal<UpdateState>({ kind: "idle" });
  readonly state = this._state.asReadonly();

  async check(): Promise<void> {
    if (this.busy()) return;
    this._state.set({ kind: "checking" });
    try {
      const info = (await this.backend.check()) as UpdateInfo | null;
      this._state.set(info ? { kind: "available", info } : { kind: "current" });
    } catch (error) {
      this._state.set({ kind: "error", step: "check", message: errorText(error), info: null });
    }
  }

  async install(): Promise<void> {
    if (this.busy()) return;
    const current = this._state();
    const info = current.kind === "available" || current.kind === "error" ? current.info : null;
    if (!info) return;
    this._state.set({ kind: "installing", info });
    try {
      await this.backend.install();
    } catch (error) {
      this._state.set({ kind: "error", step: "install", message: errorText(error), info });
    }
  }

  private busy(): boolean {
    const kind = this._state().kind;
    return kind === "checking" || kind === "installing";
  }
}
