import {
  ChangeDetectionStrategy,
  Component,
  Input,
  computed,
  inject,
  output,
  signal,
} from "@angular/core";
import { EditorTabs } from "../core/editor-tabs";
import { HerdrStore } from "../core/herdr-store";
import { PaneActions } from "../core/pane-actions";
import { TauriBridge } from "../core/tauri";
import { isGithubUrl } from "../github/pulls";
import { activityText, overviewRows, spaceDirectory, spacePullUrl } from "./overview";

/**
 * The overview of one space as an editor tab: its panes with kind, label, session status and
 * actions, plus the space actions. Everything is read from the store, so it follows the backend.
 */
@Component({
  selector: "app-space-overview",
  templateUrl: "./space-overview.html",
  styleUrl: "./space-overview.css",
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class SpaceOverview {
  private readonly spaceId = signal("");
  @Input({ required: true }) set space(id: string) {
    this.spaceId.set(id);
  }

  /** Show the Output panel for a pane. */
  readonly outputRequested = output<string>();
  /** Show a directory in the explorer. */
  readonly explorerRequested = output<string>();
  /** Delete (close) the space. */
  readonly deleteRequested = output<string>();

  private readonly store = inject(HerdrStore);
  private readonly editor = inject(EditorTabs);
  private readonly paneActions = inject(PaneActions);
  private readonly bridge = inject(TauriBridge);

  protected readonly node = computed(() =>
    this.store.spaces().find((s) => s.workspace.space_id === this.spaceId()),
  );
  protected readonly rows = computed(() => {
    const n = this.node();
    return n ? overviewRows(n) : [];
  });
  protected readonly pullUrl = computed(() => {
    const n = this.node();
    return n ? spacePullUrl(n) : undefined;
  });
  protected readonly directory = computed(() => {
    const n = this.node();
    return n ? spaceDirectory(n) : undefined;
  });

  protected activity(at: number | undefined): string {
    return activityText(at, Date.now());
  }

  protected focus(paneId: string): void {
    this.editor.open(paneId, true);
  }
  protected flow(paneId: string): void {
    this.editor.openFlow(paneId);
  }
  protected rename(paneId: string): void {
    this.paneActions.askRename(paneId);
  }
  protected close(paneId: string): void {
    this.paneActions.askClose(paneId);
  }
  /** The backend has no restart: starting an agent in the pane is the closest existing action. */
  protected restart(paneId: string): void {
    this.paneActions.askStartAgent(paneId);
  }
  protected newPane(): void {
    this.paneActions.askNew(this.spaceId());
  }
  protected openPull(): void {
    const url = this.pullUrl();
    if (url && isGithubUrl(url)) void this.bridge.openUrl(url);
  }
}
