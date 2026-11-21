import type { Injector } from "@angular/core";
import { TestBed } from "@angular/core/testing";
import { HxMessageService } from "@gravionlabs/helix-ui";
import { MESSAGES } from "../app/core/toast";

/** A provider for a spec that builds its own `Injector`: one shared message service. */
export const testMessages = () => ({ provide: MESSAGES, useValue: new HxMessageService() });

/** The text of the newest toast, `null` when none was shown (of `injector`, else of the TestBed). */
export function lastToast(injector?: Injector): string | null {
  const messages = injector ? injector.get(MESSAGES) : TestBed.inject(MESSAGES);
  return messages.messages().at(-1)?.summary ?? null;
}
