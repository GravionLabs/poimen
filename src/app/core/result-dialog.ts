import { Injectable, signal } from "@angular/core";

/** What the result dialog shows: a title, text (a command's output), and an optional action. */
export interface ResultContent {
  title: string;
  /** A short sentence above the text. */
  summary: string;
  /** Preformatted text (captured output); may be empty. */
  text: string;
  /** Offers a second button next to "Close". */
  action?: { label: string; run: () => void };
}

/** The dialog for the result of a command run by poimen (teardown output, a confirmation). */
@Injectable({ providedIn: "root" })
export class ResultDialog {
  private readonly _content = signal<ResultContent | null>(null);
  readonly content = this._content.asReadonly();

  show(content: ResultContent): void {
    this._content.set(content);
  }

  close(): void {
    this._content.set(null);
  }

  /** Closes, then runs the action. */
  runAction(): void {
    const action = this._content()?.action;
    this._content.set(null);
    action?.run();
  }
}
