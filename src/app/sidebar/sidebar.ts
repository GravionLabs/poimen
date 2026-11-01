import { ChangeDetectionStrategy, Component, inject, input, output } from "@angular/core";
import { HxMessage } from "@gravionlabs/helix-ui";
import { paneLabel, type Pane } from "../core/herdr";
import { HerdrStore } from "../core/herdr-store";

/** Workspaces and their panes with live agent status; a click opens the pane. */
@Component({
  selector: "app-sidebar",
  imports: [HxMessage],
  templateUrl: "./sidebar.html",
  styleUrl: "./sidebar.css",
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class Sidebar {
  /** Ids of the panes that have an open view. */
  readonly openIds = input<ReadonlySet<string>>(new Set());
  readonly selected = output<Pane>();

  protected readonly store = inject(HerdrStore);
  protected readonly paneLabel = paneLabel;

  protected rowTitle(pane: Pane): string {
    return pane.cwd ? `${pane.pane_id} · ${pane.cwd}` : pane.pane_id;
  }
}
