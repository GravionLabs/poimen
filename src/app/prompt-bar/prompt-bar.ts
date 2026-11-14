import { ChangeDetectionStrategy, Component, inject, input, signal } from "@angular/core";
import { HxButton } from "@gravionlabs/helix-ui";
import { AgentPrompts, promptMessage, promptSucceeded } from "../core/agent-prompt";
import type { Terminal } from "../core/snapshot";

/** A one-line input that sends a prompt to the agent of a pane; works in a read-only tab. */
@Component({
  selector: "app-prompt-bar",
  imports: [HxButton],
  templateUrl: "./prompt-bar.html",
  styleUrl: "./prompt-bar.css",
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class PromptBar {
  readonly pane = input.required<Terminal>();
  protected readonly text = signal("");
  protected readonly sending = signal(false);
  protected readonly result = signal<{ message: string; ok: boolean } | null>(null);
  private readonly prompts = inject(AgentPrompts);

  protected type(event: Event): void {
    this.text.set((event.target as HTMLInputElement).value);
  }

  /** Sends the text; it is kept when the agent did not take it, so it can be retried. */
  async send(): Promise<void> {
    const text = this.text().trim();
    if (!text || this.sending()) return;
    this.sending.set(true);
    this.result.set(null);
    const outcome = await this.prompts.send(this.pane().terminal_id, text);
    this.sending.set(false);
    this.result.set({ message: promptMessage(outcome), ok: promptSucceeded(outcome) });
    if (outcome.kind === "started" || outcome.kind === "stalled") this.text.set("");
  }
}
