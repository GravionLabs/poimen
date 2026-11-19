import { Injectable, computed, inject, signal } from "@angular/core";
import type { BackendEvent } from "./backend";
import { plainText } from "./snapshot";
import { NotificationCenter } from "./notification-center";
import { TauriBridge } from "./tauri";

/** What an agent does through `poimen mcp`. */
export type AgentWriteAction = "start_agent" | "prompt_agent";

/** An agent asks to write into a space other than its own (`agent.confirm_requested`). */
export interface ConfirmRequest {
  id: string;
  callerName: string;
  action: AgentWriteAction;
  spaceName: string;
  /** The prompt or the command line, shown as is. */
  text: string;
}

/** What the server sent in `data` of `agent.confirm_requested` and `agent.wrote`. */
interface WriteData {
  id?: string;
  caller_name?: string;
  action?: AgentWriteAction;
  space_name?: string;
  text?: string;
  confirmed?: boolean;
  /** `terminal.resume_failed` (#904). */
  reason?: string;
}

/** `agent.confirm_requested` as a request; `null` when the event lacks what the dialog shows. */
export function toRequest(data: WriteData | null): ConfirmRequest | null {
  if (!data?.id || !data.caller_name || !data.action || !data.space_name) return null;
  return {
    id: data.id,
    callerName: data.caller_name,
    action: data.action,
    spaceName: data.space_name,
    text: data.text ?? "",
  };
}

/** One line for the dialog and the notification center. */
export function writeMessage(
  caller: string,
  action: AgentWriteAction,
  space: string,
  confirmed: boolean,
): string {
  const what = action === "start_agent" ? "started an agent" : "prompted an agent";
  return `${caller} ${what} in ${space}${confirmed ? " (you allowed it)" : ""}`;
}

/**
 * The questions of agents that want to write into another space (ADR 0012, #615) and the list of
 * what agents wrote. The server holds the agent's call until `answer`, or until it times out,
 * which settles the question without us (`agent.confirm_resolved`). In memory only.
 */
@Injectable({ providedIn: "root" })
export class AgentConfirm {
  private readonly bridge = inject(TauriBridge);
  private readonly center = inject(NotificationCenter);

  private readonly queue = signal<readonly ConfirmRequest[]>([]);
  /** The question on screen: the oldest open one. */
  readonly current = computed(() => this.queue()[0] ?? null);
  readonly waiting = computed(() => this.queue().length);
  readonly error = signal<string | null>(null);

  /** Listens to the backend events; returns the function that stops it. */
  start(): () => void {
    const stop = this.bridge.listen<BackendEvent>("backend", (event) => this.handle(event));
    return () => void stop.then((f) => f());
  }

  /** Applies one backend event; the ones that are not about agent writes are ignored. */
  handle(event: BackendEvent, now = Date.now()): void {
    const data = event.data as WriteData | null;
    switch (event.kind) {
      case "agent.confirm_requested": {
        const request = toRequest(data);
        if (request && !this.queue().some((r) => r.id === request.id)) {
          this.queue.update((q) => [...q, request]);
        }
        return;
      }
      case "agent.confirm_resolved":
        this.queue.update((q) => q.filter((r) => r.id !== data?.id));
        return;
      case "agent.wrote": {
        if (!data?.caller_name || !data.action || !data.space_name) return;
        this.center.recordAgentWrite(
          data.caller_name,
          writeMessage(data.caller_name, data.action, data.space_name, data.confirmed === true),
          data.text ?? "",
          now,
        );
        return;
      }
      case "terminal.resume_failed": {
        // A hibernated agent could not be resumed (#904): the shell stays at its prompt.
        const reason = plainText(data?.reason);
        if (!reason) return;
        this.center.recordFailure("Hibernated agent", `Resume failed: ${reason}`, now);
        return;
      }
    }
  }

  /** The user's yes or no for the question on screen. */
  async answer(approve: boolean): Promise<void> {
    const request = this.current();
    if (!request) return;
    this.error.set(null);
    try {
      await this.bridge.invoke("agent_confirm_answer", { id: request.id, approve });
    } catch (e) {
      // Settled already (timed out): nothing is waiting for it anymore.
      this.error.set(typeof e === "string" ? e : String(e));
    }
    this.queue.update((q) => q.filter((r) => r.id !== request.id));
  }
}
