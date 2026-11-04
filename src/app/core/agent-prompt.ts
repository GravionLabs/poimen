import { Injectable, inject } from "@angular/core";
import { TauriBridge } from "./tauri";

/** What happened to a prompt (wire type of the `agent_prompt` command; shapes live in `herdr.rs`). */
export type PromptOutcome =
  | { kind: "started"; status: string }
  | { kind: "stalled" }
  | { kind: "timeout" }
  | { kind: "blocked" }
  | { kind: "failed"; message: string };

/** The line shown to the user for an outcome. */
export function promptMessage(outcome: PromptOutcome): string {
  switch (outcome.kind) {
    case "started":
      return outcome.status === "blocked"
        ? "Sent. The agent now waits for an answer."
        : "Sent. The agent is working.";
    case "stalled":
      return "Sent, but the agent showed no activity (stalled).";
    case "timeout":
      return "Timed out waiting for the agent to react.";
    case "blocked":
      return "The agent waits at an approval or question; nothing was sent.";
    case "failed":
      return outcome.message;
  }
}

/** Whether the prompt did what the user wanted. */
export function promptSucceeded(outcome: PromptOutcome): boolean {
  return outcome.kind === "started";
}

/**
 * Sends prompts to agents through herdr's `agent.prompt`. It needs no controller and no focus, so
 * it works from a read-only tab. herdr owns the agent's state; nothing is stored here.
 */
@Injectable({ providedIn: "root" })
export class AgentPrompts {
  private readonly bridge = inject(TauriBridge);

  /** Submits `text` to the agent in pane `target`; a transport failure becomes a `failed` outcome. */
  async send(target: string, text: string): Promise<PromptOutcome> {
    try {
      return await this.bridge.invoke<PromptOutcome>("agent_prompt", { target, text });
    } catch (err) {
      return { kind: "failed", message: String(err) };
    }
  }
}
