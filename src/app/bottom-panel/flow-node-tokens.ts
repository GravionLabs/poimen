import { ChangeDetectionStrategy, Component, computed, Input, signal } from "@angular/core";
import { flowContextLine, flowTokenLine, type FlowNode } from "../core/flow-graph";

/** The token row of a flow node: hidden when the transcript has no usage for the node. */
@Component({
  selector: "app-flow-node-tokens",
  templateUrl: "./flow-node-tokens.html",
  styles: `
    .meta {
      display: flex;
      gap: var(--h-space-2);
      font-size: var(--h-font-size-xs);
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
