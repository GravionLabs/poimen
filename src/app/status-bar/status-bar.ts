import { ChangeDetectionStrategy, Component, input, output } from "@angular/core";
import { HxSwitch } from "@gravionlabs/helix-ui";
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

/** Tooltip of the mode switch. */
export function modeTitle(blocked: string | null): string {
  return blocked
    ? `Another client controls this terminal (${blocked}); use Take over to switch`
    : "Switch between read-only and input";
}

/** Bottom bar: herdr connection, waiting agents (click jumps to the next) and the active tab. */
@Component({
  selector: "app-status-bar",
  imports: [HxSwitch],
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
  readonly jump = output<void>();
  /** The user flips the mode switch: `true` asks for input. */
  readonly control = output<boolean>();

  protected readonly label = connectionLabel;
  protected readonly modeText = modeLabel;
  protected readonly modeHint = modeTitle;
  protected readonly sizeText = sizeLabel;
  protected readonly appLabel = appVersionLabel;
  protected readonly herdrLabel = herdrVersionLabel;
  protected readonly warning = protocolWarning;
}
