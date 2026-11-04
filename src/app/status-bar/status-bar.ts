import { ChangeDetectionStrategy, Component, input, output, signal } from "@angular/core";
import { errorTitle, type UpdateState } from "../core/updater";
import type { ConnectionState } from "../core/herdr-store";
import {
  appVersionLabel,
  herdrVersionLabel,
  protocolWarning,
  type HerdrInfo,
  type Mode,
} from "../core/herdr";

/** The connection as one line of text. */
export function connectionLabel(c: ConnectionState): string {
  switch (c.state) {
    case "connected":
      return "herdr connected";
    case "reconnecting":
      return c.reason ? `herdr reconnecting: ${c.reason}` : "herdr reconnecting";
    case "unreachable":
      return c.reason ? `herdr unreachable: ${c.reason}` : "herdr unreachable";
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

/** Bottom bar: herdr connection, waiting agents (click jumps to the next) and the active tab. */
@Component({
  selector: "app-status-bar",
  templateUrl: "./status-bar.html",
  styleUrl: "./status-bar.css",
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class StatusBar {
  readonly connection = input.required<ConnectionState>();
  readonly waiting = input(0);
  readonly paneId = input<string | null>(null);
  /** The real mode of the active tab; `null` without a tab. */
  readonly mode = input<Mode | null>(null);
  /** Why the tab is read-only although input was wanted. */
  readonly blocked = input<string | null>(null);
  readonly size = input<{ cols: number; rows: number }>();
  readonly appVersion = input<string | null>(null);
  readonly info = input<HerdrInfo | null>(null);
  /** A notice such as a pane layout problem of a new space; empty hides it. */
  readonly notice = input<string | null>(null);
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

  protected readonly label = connectionLabel;
  protected readonly modeText = modeLabel;
  protected readonly modeHint = modeTitle;
  protected readonly sizeText = sizeLabel;
  protected readonly appLabel = appVersionLabel;
  protected readonly herdrLabel = herdrVersionLabel;
  protected readonly warning = protocolWarning;
  protected readonly errTitle = errorTitle;
  /** Whether the notes or the error of the update are shown above the bar. */
  protected readonly detailsOpen = signal(false);
}
