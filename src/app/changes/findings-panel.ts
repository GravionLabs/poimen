import {
  ChangeDetectionStrategy,
  Component,
  EventEmitter,
  Input,
  Output,
  inject,
} from "@angular/core";
import { findingLocation, Findings, type IndexedFinding } from "./findings";

/**
 * "Other findings": the review findings that have no marker in a diff (file without a diff, no
 * line, line beyond the file). Read-only; each can be sent to the agent as a pending comment.
 */
@Component({
  selector: "app-findings-panel",
  template: `
    <header>
      <strong>Other findings</strong>
      <span class="count">{{ items.length }}</span>
    </header>
    <ul aria-label="Other findings">
      @for (item of items; track item.id) {
        <li [class]="'sev-' + item.finding.severity">
          <span class="sev">{{ item.finding.severity }}</span>
          <span class="body">
            <span class="where">{{ location(item) }}</span>
            <strong class="title">{{ item.finding.title }}</strong>
            <span class="why">{{ item.finding.why }}</span>
          </span>
          <button
            type="button"
            class="send"
            [disabled]="findings.added().has(item.id)"
            (click)="sendFinding.emit(item)"
          >
            {{ findings.added().has(item.id) ? "Added to comments" : "Send to agent" }}
          </button>
        </li>
      }
    </ul>
  `,
  styles: `
    :host {
      display: block;
      max-height: 30%;
      overflow: auto;
      padding: 6px 12px;
      border-top: 1px solid var(--h-border-color, var(--h-surface-300));
    }
    .count,
    .where {
      color: var(--h-text-muted-color);
    }
    .where {
      font-family: var(--font-mono);
    }
    ul {
      list-style: none;
      margin: 4px 0;
      padding: 0;
    }
    li {
      display: flex;
      align-items: baseline;
      gap: 8px;
      padding: 2px 0;
    }
    .body {
      flex: 1;
      min-width: 0;
      display: flex;
      flex-direction: column;
      white-space: pre-wrap;
    }
    .sev {
      font-family: var(--font-mono);
    }
    .sev-blocking .sev {
      color: var(--term-ansi-1);
    }
    .sev-note .sev {
      color: var(--term-ansi-3);
    }
    button {
      font: inherit;
      color: inherit;
      background: transparent;
      border: 1px solid var(--h-border-color, var(--h-surface-300));
      padding: 1px 8px;
      cursor: pointer;
    }
    button:disabled {
      opacity: 0.6;
      cursor: default;
    }
  `,
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class FindingsPanel {
  protected readonly findings = inject(Findings);
  // Decorator inputs: the specs run without Angular's compiler transform.
  @Input({ required: true }) items: readonly IndexedFinding[] = [];
  @Output() readonly sendFinding = new EventEmitter<IndexedFinding>();

  protected location(item: IndexedFinding): string {
    return findingLocation(item.finding);
  }
}
