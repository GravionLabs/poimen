import {
  ChangeDetectionStrategy,
  Component,
  DestroyRef,
  computed,
  effect,
  inject,
  input,
  output,
  signal,
  untracked,
} from "@angular/core";
import { FlowView } from "../bottom-panel/flow-view";
import { flowSession, transcriptEmptyText } from "../core/snapshot";
import { SpacesStore } from "../core/spaces-store";
import { Machines } from "../core/machines";
import { Subagents } from "../core/transcripts";

/**
 * A flow editor tab: the agent run of one pane as a graph, bound to that pane whichever tab is
 * active. It has its own {@link Subagents} (its own backend watch), which stops with the tab.
 */
@Component({
  selector: "app-flow-tab",
  imports: [FlowView],
  providers: [Subagents],
  template: `
    @if (paneBlock() || !subagents.supported()) {
      <p class="empty">{{ emptyText() }}</p>
    } @else {
      @defer {
        <app-flow-view [now]="now()" (openAgent)="openAgent.emit($event)" />
      }
    }
  `,
  styles: `
    :host {
      flex: 1;
      display: flex;
      flex-direction: column;
      min-height: 0;
      font-size: 12px;
      background: var(--h-content-background);
    }

    .empty {
      margin: auto;
      max-width: 360px;
      text-align: center;
      color: var(--h-text-muted-color);
    }
  `,
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class FlowTab {
  protected readonly subagents = inject(Subagents);
  private readonly store = inject(SpacesStore);
  /** The pane whose agent run the tab shows. */
  readonly paneId = input.required<string>();
  /** A subagent node was clicked: show its transcript. */
  readonly openAgent = output<string>();
  private readonly machines = inject(Machines);
  /** Why the machine of a remote pane cannot be asked for its transcripts (#698). */
  protected readonly paneBlock = computed(() => this.machines.blockedReason(this.paneId()));
  protected readonly emptyText = computed(() =>
    transcriptEmptyText(this.store.panes().get(this.paneId()), "The flow", this.paneBlock()),
  );

  /** Clock for the durations of running nodes. */
  protected readonly now = signal(Date.now());

  private readonly session = computed(() => {
    const pane = this.store.panes().get(this.paneId());
    return flowSession(pane);
  });

  constructor() {
    const timer = setInterval(() => this.now.set(Date.now()), 1000);
    inject(DestroyRef).onDestroy(() => clearInterval(timer));
    effect(() => {
      const paneId = this.paneId();
      const session = this.session();
      untracked(() => void this.subagents.follow(session ? paneId : null, session));
    });
  }
}
