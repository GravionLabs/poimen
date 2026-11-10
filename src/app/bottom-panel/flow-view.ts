import {
  ChangeDetectionStrategy,
  Component,
  computed,
  inject,
  input,
  output,
  viewChild,
} from "@angular/core";
import {
  EFConnectableSide,
  FCanvasComponent,
  FFlowModule,
  FZoomDirective,
  provideFFlow,
} from "@foblex/flow";
import { flowAgentName } from "../core/herdr";
import { HerdrStore } from "../core/herdr-store";
import { UiState } from "../core/ui-state";
import { buildFlowGraph, flowDuration, flowTooltip, type FlowNode } from "../core/flow-graph";
import { FLOW_NODE_HEIGHT, FLOW_NODE_WIDTH, layoutFlow } from "../core/flow-layout";
import { Subagents, formatDuration } from "../core/transcripts";
import { Icon } from "../ui/icon/icon";
import { FlowNodeTokens } from "./flow-node-tokens";

const FIT_PADDING = 24;

/**
 * The Flow tab: the run of the agent as a graph (main agent, skill calls, subagents), laid out
 * automatically. The graph is rebuilt from the transcripts; the canvas keeps zoom and pan.
 */
@Component({
  selector: "app-flow-view",
  imports: [FFlowModule, FlowNodeTokens, Icon],
  providers: [provideFFlow()],
  templateUrl: "./flow-view.html",
  styleUrl: "./flow-view.css",
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class FlowView {
  private readonly subagents = inject(Subagents);
  private readonly store = inject(HerdrStore);
  private readonly ui = inject(UiState);
  /** Clock for the durations of running nodes. */
  readonly now = input.required<number>();
  /** A subagent node was clicked: show its transcript. */
  readonly openAgent = output<string>();

  protected readonly width = FLOW_NODE_WIDTH;
  protected readonly height = FLOW_NODE_HEIGHT;
  protected readonly direction = this.ui.flowDirection;
  protected readonly inSide = computed(() =>
    this.direction() === "LR" ? EFConnectableSide.LEFT : EFConnectableSide.TOP,
  );
  protected readonly outSide = computed(() =>
    this.direction() === "LR" ? EFConnectableSide.RIGHT : EFConnectableSide.BOTTOM,
  );
  private readonly canvas = viewChild(FCanvasComponent);
  private readonly zoom = viewChild(FZoomDirective);

  protected readonly graph = computed(() =>
    buildFlowGraph(
      this.subagents.agents(),
      this.subagents.entries(),
      this.subagents.main(),
      flowAgentName(this.store.panes().get(this.subagents.paneId() ?? "")),
    ),
  );
  protected readonly positions = computed(() => layoutFlow(this.graph(), this.direction()));
  protected readonly nodes = computed(() => {
    const positions = this.positions();
    return this.graph().nodes.map((node) => ({
      node,
      position: positions.get(node.id) ?? { x: 0, y: 0 },
    }));
  });

  protected tooltip = flowTooltip;

  protected duration(node: FlowNode): string {
    const ms = flowDuration(node, this.now());
    return ms === null ? "" : formatDuration(ms);
  }

  protected open(node: FlowNode): void {
    if (node.agentId) this.openAgent.emit(node.agentId);
  }

  protected onKey(event: KeyboardEvent, node: FlowNode): void {
    if (event.key !== "Enter" && event.key !== " ") return;
    event.preventDefault();
    this.open(node);
  }

  protected fit(animated = true): void {
    // Never zoom in past 100%: a lone main node would fill the panel.
    this.canvas()?.fitToScreen({ x: FIT_PADDING, y: FIT_PADDING }, animated, true, 1);
  }

  protected toggleDirection(): void {
    this.ui.setFlowDirection(this.direction() === "LR" ? "TB" : "LR");
    // Positions change with the direction; refit once the nodes have moved.
    setTimeout(() => this.fit(false));
  }

  protected zoomIn(): void {
    this.zoom()?.zoomIn();
  }

  protected zoomOut(): void {
    this.zoom()?.zoomOut();
  }
}
