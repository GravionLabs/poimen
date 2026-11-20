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
  templateUrl: "./comments-panel.html",
  styles: `
    :host {
      display: block;
      max-height: 40%;
      overflow: auto;
      padding: 6px 12px;
      border-top: 1px solid var(--h-content-border-color);
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
      border: 1px solid var(--h-content-border-color);
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
      color: var(--status-error);
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
