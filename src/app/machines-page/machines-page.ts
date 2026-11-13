import { ChangeDetectionStrategy, Component, inject, signal } from "@angular/core";
import { Machines, checkPassed, type MachineCheck, type MachineStatus } from "../core/machines";

/** Label of a check id of `machine.check`; an unknown id is shown as it is. */
export function checkLabel(id: string): string {
  return id.replace(/[-_.]+/g, " ");
}

/**
 * Settings page "Machines": the configured remote machines with their state. The context menu of a
 * machine offers "Check machine" (#281), which lists one row per check with its `fix` command.
 * poimen only shows the command (copyable); it never runs it.
 */
@Component({
  selector: "app-machines-page",
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <section aria-label="Machines">
      <p class="hint">
        Remote machines over SSH. Right-click a machine to check its prerequisites; the commands
        listed for a missing item are for you to run on the machine.
      </p>
      @if (machines.machines().length === 0) {
        <p class="hint">No machine is configured.</p>
      } @else {
        <ul class="rows" aria-label="Machines">
          @for (m of machines.machines(); track m.name) {
            <li class="row-item machine" (contextmenu)="openMenu($event, m)">
              <span class="name">{{ m.name }}</span>
              <span class="state">{{ stateText(m) }}</span>
              @if (resultOf(m.name); as r) {
                @if (isError(r)) {
                  <p class="error" role="alert">Check failed: {{ r.error }}</p>
                } @else {
                  <p class="summary">{{ passed(r) ? "All required checks passed" : "Missing" }}</p>
                  <ul class="checks" aria-label="Checks of {{ m.name }}">
                    @for (c of r.checks; track c.id) {
                      <li class="check-row" [attr.data-status]="c.status">
                        <span class="status">{{ c.status }}</span>
                        <span class="id">{{ label(c.id) }}</span>
                        @if (c.detail) {
                          <span class="detail">{{ c.detail }}</span>
                        }
                        @if (c.fix) {
                          <code class="fix">{{ c.fix }}</code>
                          <button type="button" class="copy" (click)="copy(c.fix)">Copy</button>
                        }
                      </li>
                    }
                  </ul>
                }
              }
            </li>
          }
        </ul>
      }
      @if (menu(); as m) {
        <div
          class="backdrop"
          (pointerdown)="closeMenu()"
          (contextmenu)="$event.preventDefault(); closeMenu()"
        ></div>
        <div class="menu" role="menu" [style.left.px]="m.x" [style.top.px]="m.y">
          <button type="button" role="menuitem" (click)="checkMachine()">Check machine</button>
        </div>
      }
    </section>
  `,
  styles: `
    .machine {
      display: flex;
      flex-wrap: wrap;
      gap: 4px 12px;
    }
    .state,
    .detail {
      opacity: 0.7;
    }
    .summary,
    .error,
    .checks {
      flex-basis: 100%;
      margin: 0;
    }
    .checks {
      list-style: none;
      padding: 0;
    }
    .check-row {
      display: flex;
      flex-wrap: wrap;
      align-items: baseline;
      gap: 2px 8px;
      padding: 2px 0;
    }
    .status {
      min-width: 56px;
      font-weight: 600;
    }
    [data-status="ok"] .status {
      color: var(--h-success, currentColor);
    }
    [data-status="warning"] .status {
      color: var(--h-warning, currentColor);
    }
    [data-status="missing"] .status {
      color: var(--h-danger, currentColor);
    }
    .fix {
      user-select: all;
    }
    .backdrop {
      position: fixed;
      inset: 0;
      z-index: 50;
    }
    .menu {
      position: fixed;
      z-index: 51;
      display: flex;
      flex-direction: column;
      min-width: 140px;
      padding: 4px;
      background: var(--h-surface-overlay, var(--term-bg));
      border: 1px solid var(--h-border-color, currentColor);
      border-radius: 6px;
    }
    .menu button {
      text-align: left;
      padding: 4px 8px;
      background: none;
      border: 0;
      color: inherit;
      font: inherit;
      cursor: pointer;
    }
    .menu button:hover {
      background: color-mix(in srgb, currentColor 12%, transparent);
    }
  `,
})
export class MachinesPage {
  protected readonly machines = inject(Machines);
  protected readonly menu = signal<{ name: string; x: number; y: number } | null>(null);
  protected readonly label = checkLabel;
  protected readonly passed = checkPassed;

  protected stateText(m: MachineStatus): string {
    return m.state === "offline" ? `offline: ${m.error}` : m.state;
  }

  protected resultOf(name: string): MachineCheck | { error: string } | undefined {
    return this.machines.checks()[name];
  }

  protected isError(r: MachineCheck | { error: string }): r is { error: string } {
    return "error" in r;
  }

  protected openMenu(event: MouseEvent, m: MachineStatus): void {
    event.preventDefault();
    this.menu.set({ name: m.name, x: event.clientX, y: event.clientY });
  }

  protected closeMenu(): void {
    this.menu.set(null);
  }

  protected checkMachine(): void {
    const open = this.menu();
    this.menu.set(null);
    if (open) void this.machines.check(open.name);
  }

  protected copy(text: string): void {
    void navigator.clipboard?.writeText(text);
  }
}
