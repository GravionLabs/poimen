import {
  ChangeDetectionStrategy,
  Component,
  computed,
  inject,
  input,
  output,
  signal,
  untracked,
  viewChild,
  effect,
} from "@angular/core";
import {
  EFConnectableSide,
  FCanvasComponent,
  FFlowModule,
  FZoomDirective,
  provideFFlow,
} from "@foblex/flow";
import { HxButton } from "@gravionlabs/helix-ui";
import { flowAgentName } from "../core/snapshot";
import { SpacesStore } from "../core/spaces-store";
import { UiState } from "../core/ui-state";
import {
  buildFlowGraph,
  flowDuration,
  flowStatusText,
  flowTooltip,
  startedTerminals,
  withStartedTerminals,
  type FlowNode,
  type StartedTerminal,
} from "../core/flow-graph";
import { TokenTotalsService } from "../core/token-totals";
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
  imports: [FFlowModule, FlowNodeTokens, Icon, HxButton],
  providers: [provideFFlow()],
  templateUrl: "./flow-view.html",
  styleUrl: "./flow-view.css",
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class FlowView {
  private readonly subagents = inject(Subagents);
  private readonly store = inject(SpacesStore);
  private readonly ui = inject(UiState);
  private readonly tokenTotals = inject(TokenTotalsService);
  /** Clock for the durations of running nodes. */
  readonly now = input.required<number>();
  /** A subagent node was clicked: show its transcript. */
  readonly openAgent = output<string>();

  /** A node of a started terminal was clicked: focus that terminal. */
  readonly paneChosen = output<string>();
  /** What was last seen of the terminals this agent started, so a closed one stays a node. */
  private readonly seen = signal<{ paneId: string; list: readonly StartedTerminal[] }>({
    paneId: "",
    list: [],
  });

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

  private readonly started = computed(() => {
    const paneId = this.subagents.paneId();
    if (!paneId) return [];
    return startedTerminals(
      paneId,
      this.store.all().terminals,
      this.tokenTotals.totals(),
      this.seen().paneId === paneId ? this.seen().list : [],
    );
  });

  protected readonly graph = computed(() =>
    withStartedTerminals(
      buildFlowGraph(
        this.subagents.agents(),
        this.subagents.entries(),
        this.subagents.main(),
        flowAgentName(this.store.panes().get(this.subagents.paneId() ?? "")),
      ),
      this.started(),
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
  protected statusText = flowStatusText;

  constructor() {
    // Remember the started terminals; a pane change starts again.
    effect(() => {
      const started = this.started();
      const paneId = this.subagents.paneId() ?? "";
      untracked(() => this.seen.set({ paneId, list: started }));
    });
  }

  protected duration(node: FlowNode): string {
    const ms = flowDuration(node, this.now());
    return ms === null ? "" : formatDuration(ms);
  }

  protected isLink(node: FlowNode): boolean {
    return node.agentId !== null || (node.terminalId !== null && !node.closed);
  }

  protected open(node: FlowNode): void {
    if (node.agentId) this.openAgent.emit(node.agentId);
    else if (node.terminalId && !node.closed) this.paneChosen.emit(node.terminalId);
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
