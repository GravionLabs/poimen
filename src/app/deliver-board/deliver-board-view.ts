import {
  ChangeDetectionStrategy,
  Component,
  DestroyRef,
  HostListener,
  inject,
  output,
  signal,
} from "@angular/core";
import {
  DeliverBoard,
  REFRESH_MS,
  checksText,
  isGithubUrl,
  type BoardRow,
  type RepoBoard,
} from "../core/deliver-board";

/** The deliver board as an editor tab: PBIs, claims and PR state per repository. */
@Component({
  selector: "app-deliver-board-view",
  templateUrl: "./deliver-board-view.html",
  styleUrl: "./deliver-board-view.css",
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class DeliverBoardView {
  protected readonly board = inject(DeliverBoard);
  /** The user wants the plugin's terminal board instead (unsupported state). */
  readonly terminalRequested = output<void>();
  protected readonly notice = signal<string | null>(null);
  protected readonly checksText = checksText;
  protected readonly isGithubUrl = isGithubUrl;

  constructor() {
    void this.board.load();
    const timer = setInterval(() => void this.board.load(false), REFRESH_MS);
    inject(DestroyRef).onDestroy(() => clearInterval(timer));
  }

  @HostListener("window:focus")
  protected onFocus(): void {
    void this.board.load(false);
  }

  protected refresh(): void {
    void this.board.load();
  }

  protected async start(repo: RepoBoard, row: BoardRow): Promise<void> {
    this.notice.set(null);
    const error = await this.board.startDeliver(repo.root, row);
    this.notice.set(error ?? `Started /deliver for #${row.number}.`);
  }

  protected claim(row: BoardRow): string {
    return row.assignee ? `@${row.assignee}` : "unclaimed";
  }
}
