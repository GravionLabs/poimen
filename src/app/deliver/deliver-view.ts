import { ChangeDetectionStrategy, Component, inject, output } from "@angular/core";
import { Deliver } from "./deliver";
import {
  SUPPORTED_EVENTS,
  SUPPORTED_SCHEMA,
  cardKey,
  percent,
  runState,
  stateText,
  type DeliverStatus,
} from "./deliver-status";

/** "PR #343" for a pull request URL, the URL itself when it has no number at the end. */
export function prLabel(url: string): string {
  const nr = /\/pull\/(\d+)\/?$/.exec(url)?.[1];
  return nr ? `PR #${nr}` : url;
}

/** The Deliver panel as an editor tab: one card per deliver run (loop or interactive session). */
@Component({
  selector: "app-deliver-view",
  templateUrl: "./deliver-view.html",
  styleUrl: "./deliver-view.css",
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class DeliverView {
  protected readonly deliver = inject(Deliver);
  protected readonly supported = SUPPORTED_SCHEMA;
  protected readonly eventsVersion = SUPPORTED_EVENTS;
  protected readonly cardKey = cardKey;
  protected readonly percent = percent;
  protected readonly prLabel = prLabel;
  /** The terminal of a loop was asked for. */
  readonly paneChosen = output<string>();

  protected runState(status: DeliverStatus): string {
    return runState(status, this.deliver.now());
  }

  protected stateText(status: DeliverStatus): string {
    return stateText(status, this.deliver.now());
  }
}
