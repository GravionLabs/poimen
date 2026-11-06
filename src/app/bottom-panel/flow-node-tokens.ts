import { ChangeDetectionStrategy, Component, computed, Input, signal } from "@angular/core";
import { flowContextLine, flowTokenLine, type FlowNode } from "../core/flow-graph";

/** The token row of a flow node: hidden when the transcript has no usage for the node. */
@Component({
  selector: "app-flow-node-tokens",
  template: `
    @if (tokens(); as line) {
      <div class="meta tokens">
        <span>{{ line }}</span>
        @if (context(); as ctx) {
          <span>{{ ctx }}</span>
        }
      </div>
    }
  `,
  styles: `
    .meta {
      display: flex;
      gap: 8px;
      font-size: 11px;
      color: var(--h-text-muted-color);
      white-space: nowrap;
      overflow: hidden;
    }
  `,
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class FlowNodeTokens {
  /** A decorator input (set into a signal): the specs run without the signal-input transform. */
  private readonly current = signal<FlowNode | null>(null);
  @Input({ required: true }) set node(node: FlowNode) {
    this.current.set(node);
  }
  protected readonly tokens = computed(() => this.line(flowTokenLine));
  protected readonly context = computed(() => this.line(flowContextLine));

  private line(fn: (node: FlowNode) => string): string {
    const node = this.current();
    return node ? fn(node) : "";
  }
}
