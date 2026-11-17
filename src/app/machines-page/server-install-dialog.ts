import { ChangeDetectionStrategy, Component, inject, signal } from "@angular/core";
import { HxButton } from "@gravionlabs/helix-ui";
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
  imports: [DialogShell, HxButton],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    @if (install.state(); as s) {
      <app-dialog-shell
        [heading]="
          (s.kind === 'update' ? 'Update' : 'Install') + ' poimen-server on ' + s.machine + '?'
        "
        label="Install poimen-server"
        width="640px"
        [cancelDefault]="true"
        (cancelled)="install.cancel()"
      >
        @if (s.phase === "planning") {
          <p class="hint progress" role="status">
            Reading {{ s.machine }} and downloading the server… Nothing is changed on the machine.
          </p>
        }
        @if (s.plan; as p) {
          <dl class="facts">
            <dt>Machine</dt>
            <dd>{{ s.machine }} ({{ p.ssh }})</dd>
            <dt>Version</dt>
            <dd>
              {{ p.version }} ({{ p.channel }})
              @if (s.found || p.existing) {
                , replacing {{ s.found ? "version " + s.found : p.existing }}
              }
            </dd>
            <dt>Target path</dt>
            <dd>
              <code>{{ p.dir }}/{{ p.server }}</code> and <code>{{ p.dir }}/{{ p.host }}</code>
            </dd>
            <dt>Archive</dt>
            <dd>
              <code>{{ p.archive }}</code> ({{ size(p.size) }}), SHA-256 checked against the
              release: <code>{{ p.sha256 }}</code>
            </dd>
            <dt>From</dt>
            <dd>
              <code>{{ p.origin }}</code>
            </dd>
          </dl>
          @if (s.phase === "confirm" || s.phase === "planning") {
            <p class="hint">These commands run on {{ s.machine }} over ssh, in this order:</p>
            <pre class="commands"><code>{{ script(p) }}</code></pre>
            <button
              type="button"
              hx-button
              size="small"
              variant="text"
              severity="secondary"
              class="copy"
              (click)="copy(script(p))"
            >
              {{ copied() ? "Copied" : "Copy commands" }}
            </button>
          }
          @if (installable(s.phase)) {
            <p class="hint terminals">
              @if (install.terminals().running > 0) {
                {{ install.terminals().running }}
                {{ install.terminals().running === 1 ? "terminal keeps" : "terminals keep" }}
                running; no process on the machine is stopped.
                @if (install.terminals().lost.length) {
                  Not adopted by the next server (they end when it replaces the old one):
                  {{ install.terminals().lost.join(", ") }}.
                }
              } @else {
                No terminal of {{ s.machine }} is known; no process on the machine is stopped.
              }
              The new server starts with the next connection. Agent hooks are not touched.
            </p>
          }
          <ol class="steps" aria-label="Steps">
            @for (step of s.steps; track step.id) {
              <li [attr.data-status]="step.status">
                <span aria-hidden="true">{{ icon(step.status) }}</span>
                {{ step.label }}
                <span class="sr-only">{{ step.status }}</span>
                @if (step.status === "failed" && step.output) {
                  <pre class="output"><code>{{ step.output }}</code></pre>
                }
              </li>
            }
          </ol>
        }
        @if (s.phase === "done") {
          <p class="hint" role="status">
            Installed. {{ s.machine }} connects again and is checked.
          </p>
        }
        @if (s.error; as e) {
          <p class="error" role="alert">{{ e }}</p>
        }
        <div class="dialog-buttons">
          <button
            type="button"
            hx-button
            variant="outlined"
            severity="secondary"
            class="cancel"
            [disabled]="s.phase === 'running'"
            (click)="install.cancel()"
          >
            {{ s.phase === "done" || s.phase === "failed" ? "Close" : "Cancel" }}
          </button>
          @if (s.phase === "planning" || s.phase === "confirm") {
            <button
              type="button"
              hx-button
              class="confirm"
              [disabled]="s.phase !== 'confirm'"
              (click)="install.confirm()"
            >
              {{ s.kind === "update" ? "Update" : "Install" }} on {{ s.machine }}
            </button>
          }
        </div>
      </app-dialog-shell>
    }
  `,
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
