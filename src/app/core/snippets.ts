import { Injectable, inject } from "@angular/core";
import { AgentPrompts, promptMessage, promptSucceeded } from "./agent-prompt";
import { Machines } from "./machines";
import type { Terminal } from "./snapshot";
import { offeredFor, type Snippet } from "./snippets-model";
import { SpaceActions } from "./space-actions";
import { TauriBridge } from "./tauri";

/**
 * Sends a snippet to a pane: a `terminal` snippet is typed (`terminal_type`, with Enter unless
 * `enter` is off), an `agent` snippet goes through the prompt path (`agent_prompt`).
 */
@Injectable({ providedIn: "root" })
export class SnippetRunner {
  private readonly bridge = inject(TauriBridge);
  private readonly prompts = inject(AgentPrompts);
  private readonly spaces = inject(SpaceActions);
  private readonly machines = inject(Machines, { optional: true });

  async run(snippet: Snippet, pane: Terminal | undefined): Promise<void> {
    const label = `Snippet: ${snippet.label}`;
    if (!pane) {
      this.spaces.notify(`${label}: there is no active pane`);
      return;
    }
    if (!offeredFor(snippet, pane)) {
      this.spaces.notify(`${label}: the pane is not an agent`);
      return;
    }
    const id = pane.terminal_id;
    if (snippet.target === "agent") {
      const outcome = await this.prompts.send(id, snippet.text);
      if (!promptSucceeded(outcome)) this.spaces.notify(`${label}: ${promptMessage(outcome)}`);
      return;
    }
    // A pane of an offline machine is never written through another backend (#693).
    const blocked = this.machines?.blockedReason(id) ?? null;
    if (blocked) {
      this.spaces.fail(`${label}: ${blocked}`);
      return;
    }
    try {
      await this.bridge.invoke("terminal_type", {
        target: id,
        text: snippet.text,
        enter: snippet.enter,
      });
    } catch (err) {
      this.spaces.fail(`${label} failed: ${String(err)}`);
    }
  }
}
