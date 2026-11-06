import { Injectable, signal } from "@angular/core";

/** An agent pane the user can choose. */
export interface AgentChoice {
  paneId: string;
  label: string;
}

/** The picker for "which agent": resolves to the pane id, or `null` when cancelled. */
@Injectable({ providedIn: "root" })
export class AgentPicker {
  private resolve: ((paneId: string | null) => void) | null = null;
  private readonly _choices = signal<AgentChoice[] | null>(null);
  readonly choices = this._choices.asReadonly();

  pick(choices: AgentChoice[]): Promise<string | null> {
    this.resolve?.(null);
    this._choices.set(choices);
    return new Promise((resolve) => {
      this.resolve = resolve;
    });
  }

  answer(paneId: string | null): void {
    const resolve = this.resolve;
    this.resolve = null;
    this._choices.set(null);
    resolve?.(paneId);
  }
}
