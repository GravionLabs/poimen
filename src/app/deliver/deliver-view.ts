import {
  ChangeDetectionStrategy,
  Component,
  ElementRef,
  DestroyRef,
  computed,
  effect,
  inject,
  linkedSignal,
  untracked,
  output,
} from "@angular/core";
import { NgTemplateOutlet } from "@angular/common";
import { HxButton } from "@gravionlabs/helix-ui";
import { UiState } from "../core/ui-state";
import { eventTime, toneStyle } from "../bottom-panel/run-events";
import { errorText, type GhError } from "../github/pulls";
import { compactTotal, totalsTip } from "../core/token-totals";
import { Deliver } from "./deliver";
import { OVERVIEW_REFRESH_MS, Overview } from "./overview";
import {
  issueSubjectNumber,
  type IssueState,
  type MilestoneRow,
  type OverviewIssue,
  type RepoOverview,
} from "./milestone-overview";
import {
  SUPPORTED_EVENTS,
  SUPPORTED_SCHEMA,
  ageText,
  cardKey,
  isAlert,
  leftOpen,
  needsYouItems,
  outcomeText,
  foldFinished,
  percent,
  progressText,
  runSections,
  runState,
  runningRun,
  runTitle,
  stepText,
  stateText,
  timelineLine,
  type Attention,
  type DeliverCard,
  type DeliverStatus,
  type GroupRun,
  type LeftTask,
  type Link,
  type Progress,
  type RunGroup,
  type RunSections,
  type Step,
  type TimelineLine,
} from "./deliver-status";

/** "PR #343" for a pull request URL, the URL itself when it has no number at the end. */
export function prLabel(url: string): string {
  const nr = /\/pull\/(\d+)\/?$/.exec(url)?.[1];
  return nr ? `PR #${nr}` : url;
}

/** What a link is called: its title, else a pull request number, else its `rel`. */
export function linkLabel(link: Link): string {
  if (link.title) return link.title;
  return /\/pull\/\d+\/?$/.test(link.url) ? prLabel(link.url) : link.rel;
}

export { stepText };

/** A row the keyboard moves over: a group head or, in an expanded group, a timeline line. */
export type NavRow = { kind: "group"; key: string } | { kind: "run"; key: string; group: string };

/** The rows of the cursor in display order: Running now, then History. */
export function navRows(sections: RunSections, expanded: ReadonlySet<string>): NavRow[] {
  return [...sections.running, ...sections.history].flatMap((g): NavRow[] => [
    { kind: "group", key: g.key },
    ...(expanded.has(g.key)
      ? g.runs.map((r): NavRow => ({ kind: "run", key: r.key, group: g.key }))
      : []),
  ]);
}

/** The words after the outcome of a timeline line: skipped, left open, waiting for merge. */
export function lineCounts(line: TimelineLine): string[] {
  return [
    line.skipped ? `${line.skipped} skipped` : "",
    line.leftOpen ? `${line.leftOpen} left open` : "",
    line.waiting ? `${line.waiting} waiting for merge` : "",
  ].filter((t) => t !== "");
}

/**
 * The Runs view as an editor tab (#584): the runs of the open repositories grouped by subject;
 * first what runs now and what needs the user, then each subject's history as a timeline.
 */
@Component({
  selector: "app-deliver-view",
  imports: [NgTemplateOutlet, HxButton],
  templateUrl: "./deliver-view.html",
  styleUrl: "./deliver-view.css",
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: { tabindex: "0", "aria-label": "Runs", "(keydown)": "onKey($event)" },
})
export class DeliverView {
  protected readonly deliver = inject(Deliver);
  protected readonly ui = inject(UiState);
  protected readonly overview = inject(Overview);
  private readonly host = inject<ElementRef<HTMLElement>>(ElementRef);
  protected readonly supported = SUPPORTED_SCHEMA;
  protected readonly eventsVersion = SUPPORTED_EVENTS;
  protected readonly cardKey = cardKey;
  protected readonly percent = percent;
  protected readonly progressText = progressText;
  protected readonly linkLabel = linkLabel;
  protected readonly prLabel = prLabel;
  protected readonly stepText = stepText;
  protected readonly title = runTitle;
  protected readonly outcomeText = outcomeText;
  protected readonly foldFinished = foldFinished;
  protected readonly eventTime = eventTime;
  protected readonly toneStyle = toneStyle;
  protected readonly lineCounts = lineCounts;
  /** The terminal of a run was asked for. */
  readonly paneChosen = output<string>();

  constructor() {
    // The overview follows every run event of the open repositories and is read again at least
    // every five minutes while the view is open.
    effect(() => {
      this.deliver.cards();
      untracked(() => void this.overview.load());
    });
    const timer = setInterval(() => void this.overview.load(), OVERVIEW_REFRESH_MS);
    inject(DestroyRef).onDestroy(() => clearInterval(timer));
  }

  /** Cards that are not a readable run: unknown versions and broken files. */
  protected readonly problems = computed(() =>
    this.deliver.cards().filter((c) => c.state.kind !== "ok"),
  );
  protected readonly sections = computed(() =>
    runSections(
      this.deliver.groups().filter((g) => this.overview.shown(g)),
      this.deliver.now(),
    ),
  );
  protected readonly needsYou = computed(() => {
    const { running, history } = this.sections();
    return needsYouItems([...running, ...history]);
  });
  private readonly rows = computed(() => navRows(this.sections(), this.ui.expandedRunGroups()));
  /** The key of the row under the cursor; a revealed group takes it. */
  protected readonly cursor = linkedSignal<string | null>(() => this.deliver.revealed());

  protected stateText(status: DeliverStatus): string {
    return stateText(status, this.deliver.now());
  }

  protected age(status: DeliverStatus): string {
    return ageText(status.updated, this.deliver.now());
  }

  /** The progress of a group head: GitHub's `closed/total` for a milestone, else the run's own. */
  protected progressOf(group: RunGroup): Progress | null {
    return this.overview.progressOf(group) ?? this.newest(group).progress;
  }

  protected newest(group: RunGroup): DeliverStatus {
    return group.runs[0]!.status;
  }

  protected running(group: RunGroup): GroupRun | undefined {
    return runningRun(group, this.deliver.now());
  }

  /** `1.2M tokens` of a run (#896), with the four counts as the tooltip; null without a total. */
  protected tokens(run: GroupRun): { text: string; tip: string } | null {
    const t = this.deliver.tokensOf()(run.card);
    return t ? { text: `${compactTotal(t)} tokens`, tip: totalsTip(t) } : null;
  }

  protected line(group: RunGroup, index: number): TimelineLine {
    return timelineLine(group, index, this.deliver.now());
  }

  protected leftOpen(group: RunGroup, index: number): LeftTask[] {
    return leftOpen(group, index, this.deliver.now());
  }

  protected isRunning(run: GroupRun): boolean {
    return runState(run.status, this.deliver.now()) === "running";
  }

  /** The attentions of a run that ask for a look: needs you, or a warning/danger tone. */
  protected alerts(status: DeliverStatus): Attention[] {
    return status.attentions.filter((a) => isAlert(a, status));
  }

  /** "Start loop" once per milestone group of this Mac, only while none of its runs runs. */
  protected canStart(group: RunGroup): boolean {
    return (
      group.machine === undefined &&
      group.subject?.kind === "github.milestone" &&
      this.running(group) === undefined
    );
  }

  protected startLoop(group: RunGroup): void {
    void this.deliver.startLoop(this.newest(group), group.root, group.machine);
  }

  /** The terminal of a group's running run, else of its newest run. */
  protected paneOf(group: RunGroup): string | undefined {
    return this.deliver.paneOf((this.running(group) ?? group.runs[0]!).card);
  }

  protected step(step: Step): { n: number; total: number } | null {
    return step.n !== null && step.total !== null && step.total > 0
      ? { n: step.n, total: step.total }
      : null;
  }

  /** "milestone" for `github.milestone`, else the generic "subject". */
  protected subjectLabel(group: RunGroup): string {
    const kind = group.subject?.kind ?? "";
    return kind.includes(".") ? kind.slice(kind.indexOf(".") + 1).replace("-", " ") : "subject";
  }

  /** Where the repository is: its path, with the machine in front for a remote one (`box:/src/api`). */
  protected where(card: Pick<DeliverCard, "root" | "machine">): string {
    return card.machine ? `${card.machine}:${card.root}` : card.root;
  }

  /** "claude sonnet · " when the run said which agent it is, else empty. */
  protected agentText(status: DeliverStatus): string {
    const agent = [status.harness, status.model].filter(Boolean).join(" ");
    return agent ? `${agent} · ` : "";
  }

  protected failureText(failure: GhError | string): string {
    return typeof failure === "string" ? failure : errorText(failure);
  }

  protected repoLabel(repo: RepoOverview): string {
    return repo.repo ?? this.where(repo);
  }

  protected milestoneOpen(row: MilestoneRow): boolean {
    return this.ui.expandedMilestones().has(row.key);
  }

  protected toggleMilestone(row: MilestoneRow): void {
    this.ui.setMilestoneExpanded(row.key, !this.milestoneOpen(row));
  }

  /** "running, updated 2 min ago" / "failed 3 h ago: ..." of the run a milestone row shows. */
  protected runLabel(run: GroupRun): string {
    return stateText(run.status, this.deliver.now());
  }

  protected paneOfRun(run: GroupRun): string | undefined {
    return this.deliver.paneOf(run.card);
  }

  /** The terminal of the run when it works on this issue, else nothing. */
  protected issuePane(row: MilestoneRow, entry: OverviewIssue): string | undefined {
    const run = row.run;
    if (!run || !run.status.tasks.some((t) => issueSubjectNumber(t) === entry.issue.number))
      return undefined;
    return this.deliver.paneOf(run.card);
  }

  protected blockers(entry: OverviewIssue): string {
    return entry.blockedBy.map((n) => `#${n}`).join(", ");
  }

  protected issueStateText(state: IssueState): string {
    return {
      "in-progress": "in progress",
      "needs-you": "needs you",
      blocked: "blocked",
      next: "next",
      done: "done",
    }[state];
  }

  protected groupOpen(group: RunGroup): boolean {
    return this.ui.expandedRunGroups().has(group.key);
  }

  protected runOpen(run: GroupRun): boolean {
    return this.ui.expandedRuns().has(run.key);
  }

  protected toggleGroup(group: RunGroup): void {
    this.cursor.set(group.key);
    this.ui.setRunGroupExpanded(group.key, !this.groupOpen(group));
  }

  protected toggleRun(run: GroupRun): void {
    this.cursor.set(run.key);
    this.ui.setRunExpanded(run.key, !this.runOpen(run));
  }

  /**
   * Up/Down move over groups and lines, Enter expands (again: collapses), Escape collapses. Enter
   * and Escape act on the cursor only from the view or a row toggle, so Enter on a link or an
   * action button keeps doing what that button does. A focused row toggle takes the cursor first.
   */
  protected onKey(event: KeyboardEvent): void {
    const target = event.target as HTMLElement | null;
    if (target?.closest("input, textarea, select")) return;
    const onRow = target === this.host.nativeElement || target?.dataset["row"] !== undefined;
    if ((event.key === "Enter" || event.key === "Escape") && !onRow) return;
    const rows = this.rows();
    if (rows.length === 0) return;
    // A row toggle reached with Tab (or focused otherwise) is where the keys act.
    const focused = target?.dataset["row"];
    if (focused !== undefined && rows.some((r) => r.key === focused)) this.cursor.set(focused);
    const at = rows.findIndex((r) => r.key === this.cursor());
    const row = rows[at];
    switch (event.key) {
      case "ArrowDown":
      case "ArrowUp": {
        const next =
          at < 0
            ? event.key === "ArrowDown"
              ? 0
              : rows.length - 1
            : Math.max(0, Math.min(rows.length - 1, at + (event.key === "ArrowDown" ? 1 : -1)));
        this.cursor.set(rows[next]!.key);
        this.focus(rows[next]!.key);
        break;
      }
      case "Enter":
        if (!row) return;
        if (row.kind === "group")
          this.ui.setRunGroupExpanded(row.key, !this.ui.expandedRunGroups().has(row.key));
        else this.ui.setRunExpanded(row.key, !this.ui.expandedRuns().has(row.key));
        break;
      case "Escape":
        if (!row) return;
        if (row.kind === "run" && this.ui.expandedRuns().has(row.key)) {
          this.ui.setRunExpanded(row.key, false);
        } else if (row.kind === "run") {
          this.ui.setRunGroupExpanded(row.group, false);
          this.cursor.set(row.group);
          this.focus(row.group);
        } else if (this.ui.expandedRunGroups().has(row.key)) {
          this.ui.setRunGroupExpanded(row.key, false);
        } else return;
        break;
      default:
        return;
    }
    event.preventDefault();
  }

  /** Moves the focus to the toggle of a row once it is rendered. */
  private focus(key: string): void {
    const el = [...this.host.nativeElement.querySelectorAll<HTMLElement>("[data-row]")].find(
      (e) => e.dataset["row"] === key,
    );
    el?.focus();
  }
}
