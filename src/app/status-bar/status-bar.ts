import {
  ChangeDetectionStrategy,
  Component,
  DestroyRef,
  ElementRef,
  Injector,
  afterNextRender,
  computed,
  inject,
  input,
  output,
  signal,
} from "@angular/core";
import { HxStatusBar, HxStatusItem, HxStatusPopover } from "@gravionlabs/helix-desktop";
import { HxButton } from "@gravionlabs/helix-ui";
import { SERVER_RESTART_NOTE, errorTitle, type UpdateState } from "../core/updater";
import type { BarSummary } from "../deliver/deliver-status";
import type { ConnectionState } from "../core/spaces-store";
import {
  appVersionLabel,
  backendVersionLabel,
  protocolWarning,
  type AppChannel,
  type BackendInfo,
  type Mode,
} from "../core/backend";
import type { LimitsItem } from "../core/claude-limits";
import { densityFlags, nextLevel, shortRuns, type DensityLevel } from "./status-density";

/** Room per item beyond its measured width (the gap between items) and the bar's own padding. */
const ITEM_GAP = 2;
const BAR_PADDING = 8;

/** Tooltip of the RC badge. */
export const CHANNEL_TITLE =
  "Release candidate: installed next to the stable poimen, with its own server, settings and hooks; it does not update itself";

/** The connection as one line of text. */
export function connectionLabel(c: ConnectionState, backend = "poimen"): string {
  switch (c.state) {
    case "connected":
      return `${backend} connected`;
    case "reconnecting":
      return c.reason ? `${backend} reconnecting: ${c.reason}` : `${backend} reconnecting`;
    case "unreachable":
      return c.reason ? `${backend} unreachable: ${c.reason}` : `${backend} unreachable`;
  }
}

/** The terminal size as `cols×rows`. */
export function sizeLabel(size: { cols: number; rows: number } | undefined): string {
  return size ? `${size.cols}×${size.rows}` : "";
}

/** The real mode of the active tab as text; says why it is read-only when another client controls. */
export function modeLabel(mode: Mode, blocked: string | null): string {
  if (mode === "control") return "Input";
  return blocked ? "Read-only (controlled elsewhere)" : "Read-only";
}

/** Tooltip of the mode button. */
export function modeTitle(blocked: string | null): string {
  return blocked
    ? `Another client controls this terminal (${blocked}); use Take over to switch`
    : "Switch between read-only and input";
}

/** Bottom bar: server connection, waiting agents (click jumps to the next) and the active tab. */
@Component({
  selector: "app-status-bar",
  imports: [HxButton, HxStatusBar, HxStatusItem, HxStatusPopover],
  templateUrl: "./status-bar.html",
  styleUrl: "./status-bar.css",
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class StatusBar {
  readonly connection = input.required<ConnectionState>();
  readonly waiting = input(0);
  /** The server holds a sleep inhibitor (`agents.keepAwake`, #987). */
  readonly awake = input(false);
  readonly paneId = input<string | null>(null);
  /** The real mode of the active tab; `null` without a tab. */
  readonly mode = input<Mode | null>(null);
  /** Why the tab is read-only although input was wanted. */
  readonly blocked = input<string | null>(null);
  readonly size = input<{ cols: number; rows: number }>();
  readonly appVersion = input<string | null>(null);
  /** The release channel of this build; `rc` shows the RC badge and hides the self-update (an RC never updates itself). */
  readonly channel = input<AppChannel>("stable");
  protected readonly channelTitle = CHANNEL_TITLE;
  readonly info = input<BackendInfo | null>(null);
  /** A notice such as a pane layout problem of a new space; empty hides it. */
  readonly notice = input<string | null>(null);
  /** The running poimen-server is outdated or incompatible: the notice text; `null` hides it. */
  readonly serverNotice = input<string | null>(null);
  /** "Restart server" was clicked. */
  readonly restartServer = output<void>();
  /** "Later" was clicked. */
  readonly laterServer = output<void>();
  protected readonly restartNote = SERVER_RESTART_NOTE;
  /** Whether the bottom panel is shown; while it is closed a button shows it again. */
  readonly panelOpen = input(true);
  /** Shortcut label of `toggleOutput` for the button's tooltip; empty when unbound. */
  readonly panelShortcut = input("");
  /** Shortcut label of `showShortcuts` for the ? button's tooltip; empty when unbound. */
  readonly shortcutsShortcut = input("");
  /** State of the self-update shown next to the version. */
  readonly update = input<UpdateState>({ kind: "idle" });
  /** Check for updates was clicked. */
  readonly checkUpdate = output<void>();
  /** Install and restart was clicked. */
  readonly installUpdate = output<void>();
  readonly jump = output<void>();
  /** The ? button was clicked. */
  readonly showShortcuts = output<void>();
  /** The panel button was clicked. */
  readonly showPanel = output<void>();
  /** The user flips the mode button: `true` asks for input. */
  readonly control = output<boolean>();

  /** Compact summary of the running deliver loops; `null` hides the item. */
  readonly deliver = input<BarSummary | null>(null);
  /** The Claude limits item (#784); `null` hides it. */
  readonly limits = input<LimitsItem | null>(null);
  /** The deliver item was clicked. */
  readonly showDeliver = output<void>();

  private readonly injector = inject(Injector);
  private readonly host = inject<ElementRef<HTMLElement>>(ElementRef);
  /** How far the bar has given way (#1034): 0 shows everything, see `status-density.ts`. */
  protected readonly level = signal<DensityLevel>(0);
  protected readonly density = computed(() => densityFlags(this.level()));
  /** The width the content needed at each level, as measured while it was shown. */
  private readonly needed: (number | undefined)[] = [];

  constructor() {
    const destroyRef = inject(DestroyRef);
    afterNextRender(() => {
      const bar = this.host.nativeElement.querySelector("hx-status-bar");
      if (!bar || typeof ResizeObserver === "undefined") return;
      const observer = new ResizeObserver(() => this.measure(bar));
      observer.observe(bar);
      // The bar keeps its width when its content changes (a notice, a longer text): watch the content too.
      const content = new MutationObserver(() => this.measure(bar));
      content.observe(bar, { childList: true, subtree: true, characterData: true });
      destroyRef.onDestroy(() => {
        observer.disconnect();
        content.disconnect();
      });
      this.measure(bar);
    });
  }

  /** Measures the items against the bar. Runs on every size change of the bar. */
  private measure(bar: Element): void {
    const items = Array.from(bar.querySelectorAll<HTMLElement>("hx-status-item"));
    const needed = items.reduce((sum, i) => sum + i.offsetWidth + ITEM_GAP, 0) + BAR_PADDING;
    this.fit(needed, bar.clientWidth);
  }

  /**
   * Takes the width the content needs at the current level and the width of the bar, and moves
   * to the next level; the render that follows is measured again (the bar's own size observer
   * does not fire for a change of content), so it settles in at most one step per call.
   */
  fit(needed: number, available: number): void {
    const level = this.level();
    this.needed[level] = needed;
    const next = nextLevel(level, this.needed, available);
    if (next === level) return;
    this.level.set(next);
    afterNextRender(
      () => {
        const bar = this.host.nativeElement.querySelector("hx-status-bar");
        if (bar && typeof ResizeObserver !== "undefined") this.measure(bar);
      },
      { injector: this.injector },
    );
  }

  protected readonly shortRuns = shortRuns;
  protected readonly label = connectionLabel;
  protected readonly modeText = modeLabel;
  protected readonly modeHint = modeTitle;
  protected readonly sizeText = sizeLabel;
  protected readonly appLabel = appVersionLabel;
  protected readonly versionLabel = backendVersionLabel;
  protected readonly warning = protocolWarning;
  protected readonly errTitle = errorTitle;
  /** Whether the popover with the notes or the error of the update is open (Escape and an outside click close it). */
  protected readonly detailsOpen = signal(false);
}
