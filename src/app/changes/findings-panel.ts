import {
  ChangeDetectionStrategy,
  Component,
  EventEmitter,
  Input,
  Output,
  inject,
  signal,
} from "@angular/core";
import { HxButton } from "@gravionlabs/helix-ui";
import { ContextMenu } from "../ui/context-menu/context-menu";
import { plainGroups } from "../ui/context-menu/menu-model";
import { findingLocation, Findings, type IndexedFinding } from "./findings";

/**
 * "Other findings": the review findings that have no marker in a diff (file without a diff, no
 * line, line beyond the file). Read-only; each can be sent to the agent as a pending comment.
 */
@Component({
  selector: "app-findings-panel",
  imports: [HxButton, ContextMenu],
  templateUrl: "./findings-panel.html",
  styles: `
    :host {
      display: block;
      max-height: 30%;
      overflow: auto;
      padding: 6px 12px;
      border-top: 1px solid var(--h-content-border-color);
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
  `,
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class FindingsPanel {
  protected readonly findings = inject(Findings);
  // Decorator inputs: the specs run without Angular's compiler transform.
  @Input({ required: true }) items: readonly IndexedFinding[] = [];
  @Output() readonly sendFinding = new EventEmitter<IndexedFinding>();
  /** "Open in editor" of a finding's context menu (#1002). */
  @Output() readonly openInEditor = new EventEmitter<IndexedFinding>();
  protected readonly menuAt = signal<{ item: IndexedFinding; x: number; y: number } | null>(null);

  protected openMenu(event: MouseEvent, item: IndexedFinding): void {
    if (!item.finding.file) return;
    event.preventDefault();
    this.menuAt.set({ item, x: event.clientX, y: event.clientY });
  }

  protected pick(item: IndexedFinding): void {
    this.menuAt.set(null);
    this.openInEditor.emit(item);
  }

  protected readonly menuGroups = plainGroups([{ id: "open-in-editor", label: "Open in editor" }]);

  protected location(item: IndexedFinding): string {
    return findingLocation(item.finding);
  }
}
