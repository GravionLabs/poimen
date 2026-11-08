import {
  ChangeDetectionStrategy,
  Component,
  DestroyRef,
  ElementRef,
  computed,
  effect,
  inject,
  signal,
  viewChild,
} from "@angular/core";
import { FlowView } from "./flow-view";
import { EditorTabs, tabPaneId } from "../core/editor-tabs";
import { transcriptEmptyText, type Terminal } from "../core/herdr";
import { HerdrStore } from "../core/herdr-store";
import { TaskLog } from "../core/task-log";
import { PANEL_MAX_HEIGHT, PANEL_MIN_HEIGHT, UiState } from "../core/ui-state";
import {
  Subagents,
  atBottom,
  reportIndex,
  subagentLabel,
  toolSummary,
  type TranscriptItem,
} from "../core/transcripts";

/** The tabs of the panel: Output shows subagent transcripts, Flow the run as a graph. */
export const PANEL_TAB_LABELS: Readonly<Record<string, string>> = {
  output: "Output",
  flow: "Flow",
  log: "Log",
};

/** Bottom panel below the editor area: a tab strip, the subagent dropdown and the Output tab. */
@Component({
  selector: "app-bottom-panel",
  imports: [FlowView],
  templateUrl: "./bottom-panel.html",
  styleUrl: "./bottom-panel.css",
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class BottomPanel {
  protected readonly ui = inject(UiState);
  protected readonly subagents = inject(Subagents);
  protected readonly editor = inject(EditorTabs);
  protected readonly taskLog = inject(TaskLog);
  private readonly store = inject(HerdrStore);
  protected readonly minHeight = PANEL_MIN_HEIGHT;
  protected readonly maxHeight = PANEL_MAX_HEIGHT;
  protected readonly tabs = Object.entries(PANEL_TAB_LABELS);
  protected readonly label = subagentLabel;
  protected readonly summary = toolSummary;
  protected readonly reportIndex = () => reportIndex(this.subagents.items());

  /** The pane of the active tab, the one the Output panel and the Flow tab follow. */
  private readonly activePane = computed((): Terminal | undefined => {
    const active = this.editor.activeId();
    const paneId = active === null ? null : tabPaneId(active);
    return paneId ? this.store.panes().get(paneId) : undefined;
  });
  protected readonly outputEmpty = computed(() =>
    transcriptEmptyText(this.activePane(), "Subagent output"),
  );
  protected readonly flowEmpty = computed(() => transcriptEmptyText(this.activePane(), "The flow"));

  /** Clock for the durations of running subagents. */
  protected readonly now = signal(Date.now());
  /** New lines scroll into view until the user scrolls up. */
  protected readonly following = signal(true);
  private readonly box = viewChild<ElementRef<HTMLElement>>("box");
  private drag: { startY: number; startHeight: number } | null = null;

  constructor() {
    const timer = setInterval(() => this.now.set(Date.now()), 1000);
    inject(DestroyRef).onDestroy(() => clearInterval(timer));
    effect(() => {
      this.subagents.items();
      const element = this.box()?.nativeElement;
      if (element && this.following())
        queueMicrotask(() => (element.scrollTop = element.scrollHeight));
    });
  }

  protected onScroll(): void {
    const element = this.box()?.nativeElement;
    if (element) this.following.set(atBottom(element));
  }

  protected resume(): void {
    this.following.set(true);
    const element = this.box()?.nativeElement;
    if (element) element.scrollTop = element.scrollHeight;
  }

  /** A subagent node of the flow was clicked: show its transcript in the Output tab. */
  protected openInOutput(agentId: string): void {
    this.subagents.pick(agentId);
    this.following.set(true);
    this.ui.selectPanelTab("output");
  }

  protected pick(event: Event): void {
    this.subagents.pick((event.target as HTMLSelectElement).value);
    this.following.set(true);
  }

  protected dragStart(event: PointerEvent): void {
    const handle = event.target as HTMLElement;
    handle.setPointerCapture(event.pointerId);
    handle.classList.add("dragging");
    this.drag = { startY: event.clientY, startHeight: this.ui.panelHeight() };
  }

  protected dragMove(event: PointerEvent): void {
    if (this.drag) this.ui.setPanelHeight(this.drag.startHeight + this.drag.startY - event.clientY);
  }

  protected dragEnd(event: PointerEvent): void {
    this.drag = null;
    (event.target as HTMLElement).classList.remove("dragging");
  }

  protected resizeKey(event: KeyboardEvent): void {
    const step = event.shiftKey ? 40 : 10;
    if (event.key === "ArrowUp") this.ui.setPanelHeight(this.ui.panelHeight() + step);
    else if (event.key === "ArrowDown") this.ui.setPanelHeight(this.ui.panelHeight() - step);
    else return;
    event.preventDefault();
  }

  protected isTool(item: TranscriptItem): item is Extract<TranscriptItem, { kind: "tool" }> {
    return item.kind === "tool";
  }
}
