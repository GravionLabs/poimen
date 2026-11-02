import { ChangeDetectionStrategy, Component, input, output } from "@angular/core";
import type { ConnectionState } from "../core/herdr-store";
import { appVersionLabel, herdrVersionLabel, protocolWarning, type HerdrInfo } from "../core/herdr";

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
  readonly mode = input<string | null>(null);
  readonly size = input<{ cols: number; rows: number }>();
  readonly appVersion = input<string | null>(null);
  readonly info = input<HerdrInfo | null>(null);
  readonly jump = output<void>();

  protected readonly label = connectionLabel;
  protected readonly sizeText = sizeLabel;
  protected readonly appLabel = appVersionLabel;
  protected readonly herdrLabel = herdrVersionLabel;
  protected readonly warning = protocolWarning;
}
