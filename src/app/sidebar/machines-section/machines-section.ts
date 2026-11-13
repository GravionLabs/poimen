import {
  ChangeDetectionStrategy,
  Component,
  computed,
  inject,
  output,
  signal,
} from "@angular/core";
import { rowName, type Terminal, type WorkspaceNode } from "../../core/herdr";
import { HerdrStore } from "../../core/herdr-store";
import { checkPassed, Machines, type MachineCheck, type MachineStatus } from "../../core/machines";

/** One line for the last check of a machine: its result, the error, or what is missing. */
export function checkNote(check: MachineCheck | { error: string } | undefined): string | null {
  if (!check) return null;
  if ("error" in check) return `Check failed: ${check.error}`;
  if (checkPassed(check)) return "Check passed";
  const missing = check.checks.filter((c) => c.status === "missing").map((c) => c.id);
  return `Check: missing ${missing.join(", ")}`;
}

/** The text under a machine's name: its connection state, or why it is not usable. */
export function machineNote(machine: MachineStatus): string {
  switch (machine.state) {
    case "connecting":
      return "Connecting…";
    case "connected":
      return `Connected, poimen-server ${machine.serverVersion}`;
    case "offline":
      return machine.error ? `Offline: ${machine.error}` : "Offline";
    case "incompatible":
      return (
        `Incompatible: poimen-server ${machine.serverVersion} (protocol ${machine.serverProtocol}) ` +
        `on the machine, poimen ${machine.appVersion} (protocol ${machine.appProtocol}). ` +
        "Install the same version on the machine, then reconnect."
      );
  }
}

/** A machine's spaces are shown but not usable while it is not connected. */
export const machineUsable = (machine: MachineStatus): boolean => machine.state === "connected";

/** One row of a machine's space: a terminal with the name the tab shows. */
export interface MachineRow {
  terminal: Terminal;
  name: string;
}

/** The rows of a space of a machine, in tab order. */
export function machineRows(node: WorkspaceNode): MachineRow[] {
  return node.tabs.flatMap(({ tab, panes }) =>
    panes.map((terminal) => ({ terminal, name: rowName(terminal, tab.label) })),
  );
}

/** The machines of the settings below the spaces of this Mac: state, spaces, terminals (#280). */
@Component({
  selector: "app-machines-section",
  template: `
    @for (g of groups(); track g.machine.name) {
      <section class="machine" [class.offline]="!g.usable" [attr.aria-label]="g.machine.name">
        <div class="machine-head" (contextmenu)="openMenu($event, g.machine.name)">
          <span class="dot state-{{ g.machine.state }}" aria-hidden="true"></span>
          <h2 class="machine-name">{{ g.machine.name }}</h2>
          @if (g.machine.state === "offline" || g.machine.state === "incompatible") {
            <button
              type="button"
              class="reconnect"
              [attr.aria-label]="'Reconnect ' + g.machine.name"
              (click)="reconnect(g.machine.name)"
            >
              Reconnect
            </button>
          }
        </div>
        <p class="note" role="status">{{ g.note }}</p>
        @if (g.check) {
          <p class="note check" role="status">{{ g.check }}</p>
        }
        @for (s of g.spaces; track s.node.workspace.space_id) {
          <div class="space" [attr.aria-disabled]="!g.usable || null">
            <span class="space-name">{{ s.node.workspace.label }}</span>
            @for (r of s.rows; track r.terminal.terminal_id) {
              <button
                type="button"
                class="row"
                [disabled]="!g.usable"
                (click)="pick(r.terminal, g.usable)"
              >
                <span class="dot status-{{ r.terminal.agent_status }}" aria-hidden="true"></span>
                {{ r.name }}
              </button>
            }
          </div>
        }
      </section>
    }
    @if (menuAt(); as at) {
      <button
        type="button"
        class="menu-backdrop"
        tabindex="-1"
        aria-label="Close menu"
        (click)="closeMenu()"
        (contextmenu)="$event.preventDefault(); closeMenu()"
      ></button>
      <div class="menu" role="menu" [style.left.px]="at.x" [style.top.px]="at.y">
        <button type="button" role="menuitem" class="menu-item" (click)="reconnect(at.name)">
          Reconnect
        </button>
        <button
          type="button"
          role="menuitem"
          class="menu-item"
          [disabled]="!usableMenu()"
          (click)="runCheck(at.name)"
        >
          Check
        </button>
      </div>
    }
  `,
  styles: `
    :host {
      display: block;
    }

    .machine {
      border-top: 1px solid var(--h-content-border-color);
      padding: 6px 10px;
    }

    .machine.offline .space,
    .machine.offline .note {
      opacity: 0.55;
    }

    .machine-head {
      display: flex;
      align-items: center;
      gap: 6px;
    }

    .machine-name {
      flex: 1;
      margin: 0;
      font-size: inherit;
      font-weight: 600;
    }

    .note {
      margin: 2px 0 4px;
      font-size: 0.85em;
      opacity: 0.8;
    }

    .space-name {
      display: block;
      padding: 2px 0;
      font-weight: 500;
    }

    .row {
      display: flex;
      align-items: center;
      gap: 6px;
      width: 100%;
      padding: 2px 0 2px 12px;
      border: 0;
      background: transparent;
      color: inherit;
      font: inherit;
      text-align: left;
      cursor: pointer;
    }

    .row:disabled {
      cursor: default;
    }

    .dot {
      width: 8px;
      height: 8px;
      border-radius: 50%;
      background: var(--h-content-border-color);
    }

    .state-connected,
    .status-done {
      background: var(--h-success-color, #3fb950);
    }

    .state-connecting,
    .status-working {
      background: var(--h-warn-color, #d29922);
    }

    .state-offline,
    .state-incompatible,
    .status-blocked {
      background: var(--h-danger-color, #f85149);
    }

    .reconnect {
      padding: 0 6px;
      border: 1px solid var(--h-content-border-color);
      border-radius: 4px;
      background: transparent;
      color: inherit;
      font: inherit;
      cursor: pointer;
    }

    .menu-backdrop {
      position: fixed;
      inset: 0;
      border: 0;
      background: transparent;
      cursor: default;
    }

    .menu {
      position: fixed;
      z-index: 10;
      min-width: 140px;
      padding: 4px;
      border: 1px solid var(--h-content-border-color);
      border-radius: 6px;
      background: var(--h-content-background);
    }

    .menu-item {
      display: block;
      width: 100%;
      padding: 4px 8px;
      border: 0;
      background: transparent;
      color: inherit;
      font: inherit;
      text-align: left;
      cursor: pointer;
    }
  `,
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class MachinesSection {
  private readonly store = inject(HerdrStore);
  private readonly machines = inject(Machines);
  /** A terminal of a machine was picked. */
  readonly selected = output<Terminal>();

  protected readonly groups = computed(() =>
    this.machines.machines().map((machine) => ({
      machine,
      note: machineNote(machine),
      usable: machineUsable(machine),
      check: checkNote(this.machines.checks()[machine.name]),
      spaces: this.store.remoteTree(machine.name).map((node) => ({
        node,
        rows: machineRows(node),
      })),
    })),
  );
  protected readonly menuAt = signal<{ name: string; x: number; y: number } | null>(null);

  protected readonly usableMenu = computed(() => {
    const at = this.menuAt();
    const m = at && this.machines.machines().find((x) => x.name === at.name);
    return !!m && machineUsable(m);
  });

  protected pick(terminal: Terminal, usable: boolean): void {
    if (usable) this.selected.emit(terminal);
  }

  protected openMenu(event: MouseEvent, name: string): void {
    event.preventDefault();
    this.menuAt.set({ name, x: event.clientX, y: event.clientY });
  }

  protected closeMenu(): void {
    this.menuAt.set(null);
  }

  protected runCheck(name: string): void {
    this.closeMenu();
    void this.machines.check(name);
  }

  protected reconnect(name: string): void {
    this.closeMenu();
    void this.machines.reconnect(name);
  }
}
