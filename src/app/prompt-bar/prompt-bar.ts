import {
  ChangeDetectionStrategy,
  Component,
  DestroyRef,
  effect,
  inject,
  input,
  signal,
} from "@angular/core";
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
  /** Whether the backend was told that the bar holds text (#1163). */
  private told = false;

  constructor() {
    // Text in the bar is not input to the terminal, so the server is told, or it would hibernate
    // the agent under it. Sending, discarding and closing the view clear it.
    effect(() => this.tell(this.text().trim() !== ""));
    inject(DestroyRef).onDestroy(() => this.tell(false));
  }

  private tell(draft: boolean): void {
    if (draft === this.told) return;
    this.told = draft;
    void this.prompts.draft(this.pane().terminal_id, draft);
  }

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
