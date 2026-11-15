import {
  ChangeDetectionStrategy,
  Component,
  DestroyRef,
  ElementRef,
  computed,
  effect,
  inject,
  signal,
  untracked,
  viewChild,
} from "@angular/core";
import { FlowView } from "./flow-view";
import { EditorTabs, tabPaneId } from "../core/editor-tabs";
import { paneDir, transcriptEmptyText, type Terminal } from "../core/snapshot";
import { Deliver } from "../deliver/deliver";
import { SpacesStore } from "../core/spaces-store";
import { Machines } from "../core/machines";
import {
  eventTime,
  foldSkipped,
  groupBySubject,
  groupTitle,
  mergedItems,
  pickHolds,
  pickRunOf,
  runLabel,
  runsOfSpace,
  selectRun,
  toneStyle,
  type ListItem,
  type Pick,
} from "./run-events";
import { TaskLog } from "../core/task-log";
import {
  EVENT_COLUMN_MAX,
  EVENT_COLUMN_MIN,
  PANEL_MAX_HEIGHT,
  PANEL_MIN_HEIGHT,
  UiState,
  type EventColumn,
} from "../core/ui-state";
import {
  Subagents,
  atBottom,
  reportIndex,
  subagentLabel,
  toolSummary,
  type TranscriptItem,
} from "../core/transcripts";

/** Value prefix of the "all runs" entry of the run picker. */
const ALL_PREFIX = "all:";

/** The tabs of the panel: Output shows subagent transcripts, Flow the run as a graph. */
export const PANEL_TAB_LABELS: Readonly<Record<string, string>> = {
  output: "Output",
  flow: "Flow",
  events: "Events",
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
  private readonly store = inject(SpacesStore);
  protected readonly deliver = inject(Deliver);
  protected readonly minHeight = PANEL_MIN_HEIGHT;
  protected readonly maxHeight = PANEL_MAX_HEIGHT;
  protected readonly colMin = EVENT_COLUMN_MIN;
  protected readonly colMax = EVENT_COLUMN_MAX;
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
  private readonly machines = inject(Machines);
  /** Why the machine of a remote active pane cannot be asked for its transcripts (#698). */
  protected readonly paneBlock = computed(() => {
    const id = this.activePane()?.terminal_id;
    return id === undefined ? null : this.machines.blockedReason(id);
  });
  protected readonly outputEmpty = computed(() =>
    transcriptEmptyText(this.activePane(), "Subagent output", this.paneBlock()),
  );
  protected readonly flowEmpty = computed(() =>
    transcriptEmptyText(this.activePane(), "The flow", this.paneBlock()),
  );

  /** The runs of the active space's repository, the one chosen for the Events tab and its rows. */
  protected readonly spaceRuns = computed(() =>
    runsOfSpace(
      this.deliver.cards(),
      paneDir(this.activePane() ?? ({} as Terminal)),
      this.activePane()?.machine,
    ),
  );
  private readonly pickedRun = signal<Pick | null>(null);
  protected readonly run = computed(() =>
    selectRun(this.spaceRuns(), this.activePane()?.terminal_id, this.pickedRun(), this.now()),
  );
  /** The runs by subject, for the picker (#780). */
  protected readonly groups = computed(() => groupBySubject(this.spaceRuns()));
  /** The group whose runs are listed together: the stored choice of the repository, if it holds. */
  protected readonly allGroup = computed(() => {
    const root = this.spaceRuns()[0]?.root;
    const key = root === undefined ? undefined : this.ui.eventsAll()[root];
    return this.groups().find((g) => g.key === key && g.runs.length > 1);
  });
  protected readonly rows = computed<ListItem[]>(() => {
    const group = this.allGroup();
    return group ? mergedItems(group, this.now()) : foldSkipped(this.run()?.status.events ?? []);
  });
  protected readonly groupTitle = groupTitle;
  /** Folds the user opened; kept while the panel lives. */
  protected readonly openFolds = signal<ReadonlySet<string>>(new Set());
  protected readonly allPrefix = ALL_PREFIX;
  protected readonly runLabel = runLabel;
  protected readonly eventTime = eventTime;
  protected readonly toneStyle = toneStyle;

  /** Clock for the durations of running subagents. */
  protected readonly now = signal(Date.now());
  /** New lines scroll into view until the user scrolls up. */
  protected readonly following = signal(true);
  private readonly box = viewChild<ElementRef<HTMLElement>>("box");
  private drag: { startY: number; startHeight: number } | null = null;

  constructor() {
    const timer = setInterval(() => this.now.set(Date.now()), 1000);
    inject(DestroyRef).onDestroy(() => clearInterval(timer));
    // A pick is dropped when a newer run starts, its run is gone or the repository changes.
    effect(() => {
      const runs = this.spaceRuns();
      if (untracked(this.pickedRun) && !pickHolds(runs, untracked(this.pickedRun)))
        this.pickedRun.set(null);
    });
    effect(() => {
      this.subagents.items();
      this.rows();
      const element = this.box()?.nativeElement;
      if (element && this.following())
        queueMicrotask(() => (element.scrollTop = element.scrollHeight));
    });
  }

  protected toggleFold(key: string): void {
    this.openFolds.update((s) => {
      const next = new Set(s);
      if (!next.delete(key)) next.add(key);
      return next;
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

  protected pickRun(event: Event): void {
    const value = (event.target as HTMLSelectElement).value;
    const root = this.spaceRuns()[0]?.root;
    if (root !== undefined) {
      if (value.startsWith(ALL_PREFIX)) {
        this.ui.setEventsAll(root, value.slice(ALL_PREFIX.length));
        this.pickedRun.set(null);
        this.following.set(true);
        return;
      }
      if (this.ui.eventsAll()[root] !== undefined) this.ui.setEventsAll(root, null);
    }
    this.pickedRun.set(pickRunOf(this.spaceRuns(), value));
    this.following.set(true);
  }

  protected openPr(event: Event, url: string): void {
    event.preventDefault();
    void this.deliver.openUrl(url);
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

  /** Grid columns shared by the Events header and every row. */
  protected readonly eventGrid = computed(() => {
    const c = this.ui.eventColumns();
    return `1.2em ${c.time}px ${c.task}px 1fr ${c.result}px 2.5em`;
  });

  private colDrag: { column: EventColumn; sign: 1 | -1; startX: number; start: number } | null =
    null;

  /** Starts dragging a column border; `sign` is -1 where growing the column moves the border left. */
  protected colStart(event: PointerEvent, column: EventColumn, sign: 1 | -1): void {
    (event.target as HTMLElement).setPointerCapture?.(event.pointerId);
    this.colDrag = { column, sign, startX: event.clientX, start: this.ui.eventColumns()[column] };
  }

  protected colMove(event: PointerEvent): void {
    const d = this.colDrag;
    if (d) this.ui.setEventColumn(d.column, d.start + d.sign * (event.clientX - d.startX));
  }

  protected colEnd(): void {
    this.colDrag = null;
  }

  protected colKey(event: KeyboardEvent, column: EventColumn, sign: 1 | -1): void {
    const step = event.shiftKey ? 40 : 10;
    const delta = event.key === "ArrowRight" ? step : event.key === "ArrowLeft" ? -step : 0;
    if (!delta) return;
    this.ui.setEventColumn(column, this.ui.eventColumns()[column] + sign * delta);
    event.preventDefault();
  }

  protected isTool(item: TranscriptItem): item is Extract<TranscriptItem, { kind: "tool" }> {
    return item.kind === "tool";
  }
}
