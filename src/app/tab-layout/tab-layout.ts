import { NgTemplateOutlet } from "@angular/common";
import {
  ChangeDetectionStrategy,
  Component,
  ElementRef,
  EventEmitter,
  Input,
  OnChanges,
  OnDestroy,
  SimpleChanges,
  Output,
  inject,
  signal,
} from "@angular/core";
import { olderHost, treeTerminals, type Terminal, type TabTree } from "../core/snapshot";
import { type Mode } from "../core/backend";
import { SplitPane } from "../ui/helix-candidates/split-pane/split-pane";
import { TerminalView } from "../terminal-view/terminal-view";

export type PaneMenuAction = "split-right" | "split-down" | "restart" | "close";

/** The leaf `delta` places after `current` in `leaves`, wrapping; the first leaf when `current` is unknown. */
export function cycleFocus(
  leaves: readonly string[],
  current: string | null,
  delta: 1 | -1,
): string | null {
  if (leaves.length === 0) return null;
  const at = current === null ? -1 : leaves.indexOf(current);
  if (at < 0) return leaves[0];
  return leaves[(at + delta + leaves.length) % leaves.length];
}

/** The key that moves focus: Alt+Right/Down to the next pane, Alt+Left/Up to the previous; else `null`. */
export function focusDelta(
  event: Pick<KeyboardEvent, "key" | "altKey" | "ctrlKey" | "metaKey">,
): 1 | -1 | null {
  if (!event.altKey || event.ctrlKey || event.metaKey) return null;
  if (event.key === "ArrowRight" || event.key === "ArrowDown") return 1;
  if (event.key === "ArrowLeft" || event.key === "ArrowUp") return -1;
  return null;
}

/**
 * The split tree of one tab: one `terminal-view` per leaf, nested `app-split-pane`s for the
 * splits (direction and ratio from the tree). The focused leaf gets the wanted mode, the others
 * observe; every leaf keeps its own stream and size. Decorator inputs: the specs run without the
 * Angular compiler transform.
 */
@Component({
  selector: "app-tab-layout",
  imports: [NgTemplateOutlet, SplitPane, TerminalView],
  templateUrl: "./tab-layout.html",
  styleUrl: "./tab-layout.css",
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class TabLayout implements OnChanges, OnDestroy {
  @Input({ required: true }) tabId = "";
  @Input({ required: true }) root!: TabTree;
  @Input() panes: ReadonlyMap<string, Terminal> = new Map();
  /** The focused leaf, as the server (or the last click) says. */
  @Input() focused: string | null = null;
  /** The mode the user wants for the focused leaf. */
  @Input() wanted: Mode = "observe";
  @Output() readonly focusChange = new EventEmitter<string>();
  @Output() readonly ratioChange = new EventEmitter<{ splitId: string; ratio: number }>();
  @Output() readonly paneAction = new EventEmitter<{
    terminalId: string;
    action: PaneMenuAction;
  }>();
  @Output() readonly modeChange = new EventEmitter<{
    terminalId: string;
    mode: Mode;
    blocked: string | null;
  }>();
  @Output() readonly sized = new EventEmitter<{
    terminalId: string;
    size: { cols: number; rows: number };
  }>();

  /** Ratios of splits being dragged; the tree's own ratio takes over once the server confirms. */
  protected readonly dragged = signal<ReadonlyMap<string, number>>(new Map());
  protected readonly menu = signal<{ terminalId: string; x: number; y: number } | null>(null);
  private readonly host = inject<ElementRef<HTMLElement>>(ElementRef);
  private readonly onKey = (event: KeyboardEvent): void => {
    const delta = focusDelta(event);
    if (delta === null) return;
    const next = cycleFocus(treeTerminals(this.root), this.focused, delta);
    if (next === null) return;
    event.preventDefault();
    event.stopPropagation();
    this.focusChange.emit(next);
  };

  constructor() {
    // Capture phase: xterm would otherwise turn Alt+arrow into an escape sequence for the shell.
    this.host.nativeElement.addEventListener("keydown", this.onKey, true);
  }

  ngOnChanges(changes: SimpleChanges): void {
    // A new tree from the server carries the ratios it settled on.
    if (changes["root"] && !changes["root"].firstChange) this.dragged.set(new Map());
  }

  ngOnDestroy(): void {
    this.host.nativeElement.removeEventListener("keydown", this.onKey, true);
  }

  protected pane(id: string): Terminal | undefined {
    return this.panes.get(id);
  }

  /** The terminal runs on an older PTY host: its menu offers a restart on the new one (#606). */
  protected onOlderHost(id: string): boolean {
    return olderHost(this.panes.get(id)) !== null;
  }

  protected modeOf(id: string): Mode {
    return id === this.focused ? this.wanted : "observe";
  }

  protected sizesOf(node: Extract<TabTree, { kind: "split" }>): number[] {
    const ratio = this.dragged().get(node.split_id) ?? node.ratio;
    return [ratio, 1 - ratio];
  }

  protected resize(node: Extract<TabTree, { kind: "split" }>, sizes: number[]): void {
    const ratio = sizes[0];
    this.dragged.update((m) => new Map(m).set(node.split_id, ratio));
    this.ratioChange.emit({ splitId: node.split_id, ratio });
  }

  protected openMenu(event: MouseEvent, terminalId: string): void {
    event.preventDefault();
    this.focusChange.emit(terminalId);
    this.menu.set({ terminalId, x: event.clientX, y: event.clientY });
  }

  protected choose(action: PaneMenuAction): void {
    const open = this.menu();
    this.menu.set(null);
    if (open) this.paneAction.emit({ terminalId: open.terminalId, action });
  }

  protected closeMenu(): void {
    this.menu.set(null);
  }
}
