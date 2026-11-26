import { ChangeDetectionStrategy, Component, inject, signal } from "@angular/core";
import { HxButton, HxMessage } from "@gravionlabs/helix-ui";
import { ServerInstall, planScript, sizeText } from "../core/server-install";
import { DialogShell } from "../ui/dialog-shell/dialog-shell";

const ICON = { pending: "·", running: "…", ok: "✓", failed: "✕" } as const;

/**
 * The confirmation of "Install server…" / "Update server…" (ADR 0013, #865): the machine, the
 * versions, the target path and the exact commands. Cancel has the focus; nothing runs on the
 * machine before "Install on <machine>" is pressed. Progress is per step; a failed step shows
 * ssh's output.
 */
@Component({
  selector: "app-server-install-dialog",
  imports: [DialogShell, HxButton, HxMessage],
  changeDetection: ChangeDetectionStrategy.OnPush,
  templateUrl: "./server-install-dialog.html",
  styles: `
    .facts {
      display: grid;
      grid-template-columns: max-content 1fr;
      gap: 4px 12px;
      margin: 0 0 8px;
    }
    .facts dt {
      font-weight: 600;
    }
    .facts dd {
      margin: 0;
      overflow-wrap: anywhere;
    }
    .commands,
    .output {
      margin: 0;
      padding: 4px 8px;
      overflow-x: auto;
      user-select: all;
      border: 1px solid var(--h-content-border-color);
      border-radius: 4px;
    }
    .steps {
      list-style: none;
      padding: 0;
      margin: 8px 0;
    }
    [data-status="ok"] {
      color: var(--status-done);
    }
    [data-status="failed"] {
      color: var(--status-error);
    }
    .sr-only {
      position: absolute;
      width: 1px;
      height: 1px;
      overflow: hidden;
      clip-path: inset(50%);
    }
  `,
})
export class ServerInstallDialog {
  protected readonly install = inject(ServerInstall);
  protected readonly copied = signal(false);
  protected readonly script = planScript;
  protected readonly size = sizeText;
  protected readonly icon = (status: keyof typeof ICON): string => ICON[status];

  /** Whether the terminals note applies: before and while the install, not afterwards. */
  protected installable(phase: string): boolean {
    return phase === "confirm" || phase === "running";
  }

  protected copy(text: string): void {
    void Promise.resolve(navigator.clipboard?.writeText(text)).then(() => {
      this.copied.set(true);
      setTimeout(() => this.copied.set(false), 2000);
    });
  }
}
