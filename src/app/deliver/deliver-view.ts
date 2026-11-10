import { ChangeDetectionStrategy, Component, inject, output } from "@angular/core";
import { Deliver } from "./deliver";
import {
  SUPPORTED_EVENTS,
  SUPPORTED_SCHEMA,
  cardKey,
  percent,
  progressText,
  runState,
  runTitle,
  stepText,
  stateText,
  type Attention,
  type DeliverStatus,
  type Link,
} from "./deliver-status";

/** "PR #343" for a pull request URL, the URL itself when it has no number at the end. */
export function prLabel(url: string): string {
  const nr = /\/pull\/(\d+)\/?$/.exec(url)?.[1];
  return nr ? `PR #${nr}` : url;
}

/** What a link is called: its title, else a pull request number, else its `rel`. */
export function linkLabel(link: Link): string {
  if (link.title) return link.title;
  return /\/pull\/\d+\/?$/.test(link.url) ? prLabel(link.url) : link.rel;
}

export { stepText };

/** The Deliver panel as an editor tab: one card per run (any writer of agent-events). */
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
  protected readonly progressText = progressText;
  protected readonly linkLabel = linkLabel;
  protected readonly stepText = stepText;
  protected readonly title = runTitle;
  /** The terminal of a run was asked for. */
  readonly paneChosen = output<string>();

  protected runState(status: DeliverStatus): string {
    return runState(status, this.deliver.now());
  }

  protected stateText(status: DeliverStatus): string {
    return stateText(status, this.deliver.now());
  }

  protected needsYou(status: DeliverStatus): Attention[] {
    return status.attentions.filter((a) => a.needsYou);
  }

  protected notices(status: DeliverStatus): Attention[] {
    return status.attentions.filter((a) => !a.needsYou);
  }

  /** "milestone" for `github.milestone`, else the generic "subject". */
  protected subjectLabel(status: DeliverStatus): string {
    const kind = status.subject?.kind ?? "";
    return kind.includes(".") ? kind.slice(kind.indexOf(".") + 1).replace("-", " ") : "subject";
  }

  /** "claude sonnet · " when the run said which agent it is, else empty. */
  protected agentText(status: DeliverStatus): string {
    const agent = [status.harness, status.model].filter(Boolean).join(" ");
    return agent ? `${agent} · ` : "";
  }
}
