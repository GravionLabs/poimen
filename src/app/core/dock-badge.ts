import { Injectable, inject } from "@angular/core";
import { TauriBridge } from "./tauri";

/** The dock badge: how many agents wait for the user. 0 clears it. */
@Injectable({ providedIn: "root" })
export class DockBadge {
  private readonly bridge = inject(TauriBridge);
  private last: number | undefined;

  /** Shows `waiting` (blocked agents) on the dock icon; platforms without badges only warn. */
  set(waiting: number): void {
    const count = Math.max(0, Math.floor(waiting));
    if (count === this.last) return;
    this.last = count;
    void this.bridge
      .setBadgeCount(count > 0 ? count : undefined)
      .catch((error: unknown) => console.warn("poimen: could not set the dock badge", error));
  }
}
