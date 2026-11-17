import {
  ChangeDetectionStrategy,
  Component,
  computed,
  inject,
  output,
  signal,
} from "@angular/core";
import { HxButton } from "@gravionlabs/helix-ui";
import { rowName, type Terminal, type WorkspaceNode } from "../../core/snapshot";
import { ContextMenu } from "../../ui/context-menu/context-menu";
import { plainGroups, type MenuGroup } from "../../ui/context-menu/menu-model";
import { SpacesStore } from "../../core/spaces-store";
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
  imports: [ContextMenu, HxButton],
  template: `
    @for (g of groups(); track g.machine.name) {
      <section class="machine" [class.offline]="!g.usable" [attr.aria-label]="g.machine.name">
        <div class="machine-head" (contextmenu)="openMenu($event, g.machine.name)">
          <span class="dot state-{{ g.machine.state }}" aria-hidden="true"></span>
          <h2 class="machine-name">{{ g.machine.name }}</h2>
          @if (g.machine.state === "offline" || g.machine.state === "incompatible") {
            <button
              type="button"
              hx-button
              variant="outlined"
              severity="secondary"
              size="small"
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
      <app-context-menu
        [at]="at"
        [groups]="menuGroups()"
        (picked)="choose(at.name, $event.id)"
        (closed)="closeMenu()"
      />
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
      background: var(--status-done);
    }

    .state-connecting,
    .status-working {
      background: var(--status-blocked);
    }

    .state-offline,
    .state-incompatible,
    .status-blocked {
      background: var(--status-error);
    }
  `,
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class MachinesSection {
  private readonly store = inject(SpacesStore);
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

  protected readonly menuGroups = computed<MenuGroup[]>(() => {
    const at = this.menuAt();
    const m = at && this.machines.machines().find((x) => x.name === at.name);
    const usable = !!m && machineUsable(m);
    return plainGroups([
      { id: "reconnect", label: "Reconnect" },
      {
        id: "check",
        label: "Check",
        disabled: usable ? false : "The machine is not connected",
      },
    ]);
  });

  protected choose(name: string, id: string): void {
    if (id === "reconnect") this.reconnect(name);
    else if (id === "check") this.runCheck(name);
  }

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
