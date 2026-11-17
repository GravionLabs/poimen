import {
  ChangeDetectionStrategy,
  Component,
  EventEmitter,
  Output,
  inject,
  signal,
} from "@angular/core";
import { HxButton } from "@gravionlabs/helix-ui";
import { rowName } from "../core/snapshot";
import type { LineComment } from "../core/ui-state";
import { commentLocation } from "./comment-prompt";
import { LineComments } from "./line-comments";

/**
 * The review comments of the Changes tab: the pending ones (editable, removable), the button that
 * sends them to the worktree's agent and the ones already sent.
 */
@Component({
  selector: "app-comments-panel",
  imports: [HxButton],
  template: `
    <header>
      <strong>Comments</strong>
      <span class="count">{{ comments.pending().length }} pending</span>
      <span class="spacer"></span>
      @if (comments.agents().length > 1) {
        <select
          aria-label="Agent to send to"
          [value]="comments.chosen() ?? ''"
          (change)="comments.chosen.set($any($event.target).value || null)"
        >
          <option value="">Pick an agent…</option>
          @for (agent of comments.agents(); track agent.terminal_id) {
            <option
              [value]="agent.terminal_id"
              [selected]="agent.terminal_id === comments.chosen()"
            >
              {{ label(agent) }}
            </option>
          }
        </select>
      }
      <button
        type="button"
        hx-button
        size="small"
        class="send"
        [disabled]="comments.blocked() !== null"
        [title]="comments.blocked() ?? 'Send the pending comments to the agent as one prompt'"
        (click)="comments.send()"
      >
        Send to agent
      </button>
    </header>
    @if (comments.blocked(); as reason) {
      @if (comments.pending().length) {
        <p class="reason">{{ reason }}</p>
      }
    }
    @if (comments.error(); as error) {
      <p class="problem" role="alert">{{ error }}</p>
    }
    <ul class="pending" aria-label="Pending comments">
      @for (c of comments.pending(); track c.id) {
        <li>
          <button
            type="button"
            hx-button
            variant="text"
            severity="secondary"
            size="small"
            class="where"
            (click)="open.emit(c)"
          >
            {{ where(c) }}
          </button>
          @if (editing() === c.id) {
            <textarea
              rows="3"
              [value]="c.text"
              aria-label="Edit comment"
              #field
              (keydown.escape)="editing.set(null)"
            ></textarea>
            <span class="actions">
              <button
                type="button"
                hx-button
                size="small"
                class="save"
                (click)="save(c, field.value)"
              >
                Save
              </button>
              <button
                type="button"
                hx-button
                variant="text"
                severity="secondary"
                size="small"
                class="cancel"
                (click)="editing.set(null)"
              >
                Cancel
              </button>
            </span>
          } @else {
            <span class="text">{{ c.text }}</span>
            <span class="actions">
              <button
                type="button"
                hx-button
                variant="outlined"
                severity="secondary"
                size="small"
                class="edit"
                (click)="editing.set(c.id)"
              >
                Edit
              </button>
              <button
                type="button"
                hx-button
                variant="text"
                severity="danger"
                size="small"
                class="remove"
                (click)="comments.remove(c.id)"
              >
                Remove
              </button>
            </span>
          }
        </li>
      }
    </ul>
    @if (comments.sent().length) {
      <details class="sent">
        <summary>Sent ({{ comments.sent().length }})</summary>
        <ul aria-label="Sent comments">
          @for (c of comments.sent(); track c.id) {
            <li>
              <span class="where">{{ where(c) }}</span>
              <span class="text">{{ c.text }}</span>
            </li>
          }
        </ul>
        <button
          type="button"
          hx-button
          variant="text"
          severity="secondary"
          size="small"
          class="clear"
          (click)="comments.clearSent()"
        >
          Clear sent
        </button>
      </details>
    }
  `,
  styles: `
    :host {
      display: block;
      max-height: 40%;
      overflow: auto;
      padding: 6px 12px;
      border-top: 1px solid var(--h-border-color, var(--h-surface-300));
    }
    header {
      display: flex;
      align-items: center;
      gap: 8px;
    }
    .spacer {
      flex: 1;
    }
    .count,
    .reason,
    .where {
      color: var(--h-text-muted-color);
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
    .text {
      flex: 1;
      min-width: 0;
      white-space: pre-wrap;
    }
    .where {
      font-family: var(--font-mono);
    }
    textarea {
      flex: 1;
      font: inherit;
      color: inherit;
      background: transparent;
    }
    select {
      font: inherit;
      color: inherit;
      background: transparent;
      border: 1px solid var(--h-border-color, var(--h-surface-300));
      height: var(--control-height);
      padding: 1px 8px;
    }
    button.where {
      font-family: var(--font-mono);
      color: var(--h-text-muted-color);
    }
    .actions {
      display: inline-flex;
      gap: 4px;
    }
    .problem {
      margin: 4px 0;
      color: var(--h-danger-color, #d33);
    }
  `,
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class CommentsPanel {
  protected readonly comments = inject(LineComments);
  protected readonly editing = signal<string | null>(null);
  /** Asks the tab to show the file of a comment. */
  @Output() readonly open = new EventEmitter<LineComment>();

  protected where(c: LineComment): string {
    return commentLocation(c);
  }

  protected label(pane: Parameters<typeof rowName>[0]): string {
    return `${pane.agent ?? "agent"} · ${rowName(pane)}`;
  }

  protected save(c: LineComment, text: string): void {
    this.comments.edit(c.id, text);
    this.editing.set(null);
  }
}
