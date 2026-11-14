import { ChangeDetectionStrategy, Component, input, output, signal } from "@angular/core";
import { SERVER_RESTART_NOTE, errorTitle, type UpdateState } from "../core/updater";
import type { BackendKind } from "../core/settings";
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

/** Tooltip of the RC badge. */
export const CHANNEL_TITLE =
  "Release candidate: installed next to the stable poimen, with its own server, settings and hooks; it does not update itself";

/** The connection as one line of text. */
export function connectionLabel(c: ConnectionState, backend = "herdr"): string {
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
  /** The release channel of this build; `rc` shows the RC badge and hides the self-update (an RC never updates itself). */
  readonly channel = input<AppChannel>("stable");
  protected readonly channelTitle = CHANNEL_TITLE;
  readonly info = input<BackendInfo | null>(null);
  /** The backend this run talks to; names the connection and the server version. */
  readonly backend = input<BackendKind>("herdr");
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
  /** The deliver item was clicked. */
  readonly showDeliver = output<void>();

  protected readonly label = connectionLabel;
  protected readonly modeText = modeLabel;
  protected readonly modeHint = modeTitle;
  protected readonly sizeText = sizeLabel;
  protected readonly appLabel = appVersionLabel;
  protected readonly herdrLabel = backendVersionLabel;
  protected readonly warning = protocolWarning;
  protected readonly errTitle = errorTitle;
  /** Whether the notes or the error of the update are shown above the bar. */
  protected readonly detailsOpen = signal(false);
}
