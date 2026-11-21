import { InjectionToken, inject } from "@angular/core";
import { HxMessageService } from "@gravionlabs/helix-ui";

/** How long a toast stays (ms); a failure stays longer so its reason can be read. */
export const TOAST_MS = 5000;
export const TOAST_ERROR_MS = 10000;

/**
 * The `HxMessageService` the `<hx-toast>` of the shell shows (`app.config.ts` provides it). A
 * spec without that provider gets its own instance, so it can read `messages()`.
 */
export const MESSAGES = new InjectionToken<HxMessageService>("poimen toast messages", {
  providedIn: "root",
  factory: () => inject(HxMessageService, { optional: true }) ?? new HxMessageService(),
});

/** The messages of the shell; a bare injector without a root (a spec) gets a service of its own. */
export function injectMessages(): HxMessageService {
  return inject(MESSAGES, { optional: true }) ?? new HxMessageService();
}
