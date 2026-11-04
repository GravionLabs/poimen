import {
  ChangeDetectionStrategy,
  Component,
  ElementRef,
  effect,
  inject,
  input,
  output,
  signal,
  viewChild,
} from "@angular/core";
import {
  CommandPalette as Palette,
  type PaletteItem,
  type PoimenCommand,
} from "../core/command-palette";
import { palettePosition, PALETTE_MAX_HEIGHT } from "./palette-position";
import { titleParts } from "./title-parts";
import type { PluginPaneDef } from "../core/plugin-panes";

/** The command palette dialog: type to filter, arrows to move, Enter to run, Escape to close. */
@Component({
  selector: "app-command-palette",
  changeDetection: ChangeDetectionStrategy.OnPush,
  templateUrl: "./command-palette.html",
  styleUrl: "./command-palette.css",
})
export class CommandPalette {
  protected readonly palette = inject(Palette);
  private readonly host = inject<ElementRef<HTMLElement>>(ElementRef);
  /** The pane that has focus in the editor; the context of a plugin action. */
  readonly activePaneId = input<string | null>(null);
  /** A poimen command was chosen; the host runs it. */
  readonly commandChosen = output<PoimenCommand["id"]>();
  /** A plugin pane was chosen; the host opens it as a tab. */
  readonly paneChosen = output<PluginPaneDef>();
  /** A plugin action failed; the host shows the message as a notice. */
  readonly failed = output<string>();
  private readonly field = viewChild<ElementRef<HTMLInputElement>>("field");

  /** Top-left corner of the popup, centred on the editor area. */
  protected readonly position = signal({ left: 0, top: 0 });

  private place(): void {
    const area = document.querySelector(".editor-area")?.getBoundingClientRect() ?? null;
    this.position.set(
      palettePosition(
        area,
        { width: window.innerWidth, height: window.innerHeight },
        PALETTE_MAX_HEIGHT,
      ),
    );
  }

  constructor() {
    effect((onCleanup) => {
      if (!this.palette.isOpen()) return;
      this.place();
      const place = () => this.place();
      window.addEventListener("resize", place);
      const area = document.querySelector(".editor-area");
      const observer = area ? new ResizeObserver(place) : null;
      if (area) observer?.observe(area);
      onCleanup(() => {
        window.removeEventListener("resize", place);
        observer?.disconnect();
      });
    });
    effect(() => {
      if (this.palette.isOpen()) this.field()?.nativeElement.focus();
    });
    effect(() => {
      this.palette.selected();
      if (!this.palette.isOpen()) return;
      queueMicrotask(() =>
        this.host.nativeElement.querySelector(".selected")?.scrollIntoView({ block: "nearest" }),
      );
    });
  }

  /** The title of a row as runs of text; matched characters are flagged for highlighting. */
  protected parts(title: string, positions: readonly number[]): { text: string; hit: boolean }[] {
    return titleParts(title, positions);
  }

  protected type(event: Event): void {
    this.palette.setQuery((event.target as HTMLInputElement).value);
  }

  protected key(event: KeyboardEvent): void {
    if (event.key === "ArrowDown") this.palette.move(1);
    else if (event.key === "ArrowUp") this.palette.move(-1);
    else if (event.key === "Enter") {
      const item = this.palette.current();
      if (item) void this.run(item);
    } else if (event.key === "Escape") this.palette.close();
    else return;
    event.preventDefault();
  }

  protected async run(item: PaletteItem): Promise<void> {
    this.palette.recordRun(item);
    this.palette.close();
    if (item.kind === "command") {
      this.commandChosen.emit(item.command);
      return;
    }
    if (item.kind === "pane") {
      this.paneChosen.emit(item.def);
      return;
    }
    const error = await this.palette.invoke(item.action, this.activePaneId());
    if (error) this.failed.emit(error);
  }
}
