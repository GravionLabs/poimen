import {
  ChangeDetectionStrategy,
  Component,
  EventEmitter,
  Input,
  Output,
  computed,
  inject,
  output,
  signal,
} from "@angular/core";
import { HxButton, type HxTreeNode } from "@gravionlabs/helix-ui";
import { qualifyId, rowName, type Terminal, type WorkspaceNode } from "../../core/snapshot";
import { ContextMenu } from "../../ui/context-menu/context-menu";
import { plainGroups, type MenuGroup } from "../../ui/context-menu/menu-model";
import { SpacesStore } from "../../core/spaces-store";
import { UiState } from "../../core/ui-state";
import { EditorTabs } from "../../core/editor-tabs";
import { SpaceCard, type CardMenuRequest } from "../space-card";
import type { SpaceData } from "../sidebar";
import type { SidebarMenuAction } from "../sidebar-menu";
import {
  checkPassed,
  machineHint,
  Machines,
  type MachineCheck,
  type MachineStatus,
} from "../../core/machines";

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
  imports: [ContextMenu, HxButton, SpaceCard],
  templateUrl: "./machines-section.html",
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

    .command {
      margin: 0;
      font-family: var(--font-mono, monospace);
      overflow-x: auto;
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
    .status-blocked {
      background: var(--status-blocked);
    }

    .status-working {
      background: var(--status-working);
    }

    .state-offline,
    .state-incompatible {
      background: var(--status-error);
    }
  `,
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class MachinesSection {
  private readonly store = inject(SpacesStore);
  protected readonly machines = inject(Machines);
  protected readonly ui = inject(UiState);
  private readonly editor = inject(EditorTabs);
  /** A terminal of a machine was picked. */
  readonly selected = output<Terminal>();

  private readonly cardsSignal = signal(false);
  private readonly nodesSignal = signal<ReadonlyMap<string, HxTreeNode<SpaceData>[]>>(new Map());
  /** The `cards` layout: each space of a machine is a card (#938). */
  @Input() set cardsLayout(value: boolean) {
    this.cardsSignal.set(value);
  }
  /** The card nodes of `spaceNodes` per machine name, built by the sidebar from `remoteTree`. */
  @Input() set nodes(value: ReadonlyMap<string, HxTreeNode<SpaceData>[]>) {
    this.nodesSignal.set(value);
  }
  @Input() openIds: ReadonlySet<string> = new Set();
  @Input() activeId: string | null = null;
  /** A card asked for the context menu of the sidebar (same entries as the tree). */
  @Output() readonly cardMenu = new EventEmitter<CardMenuRequest>();
  /** An action button of a card (new terminal, close space). */
  @Output() readonly cardAct = new EventEmitter<SidebarMenuAction>();
  protected readonly cards = this.cardsSignal.asReadonly();

  protected cardNodes(machine: string): HxTreeNode<SpaceData>[] {
    return this.nodesSignal().get(machine) ?? [];
  }

  protected copy(text: string): void {
    void Promise.resolve(navigator.clipboard?.writeText(text)).catch(() => undefined);
  }

  /** A row of a card: the terminal by its qualified id, only on a connected machine. */
  protected pickId(id: string): void {
    for (const g of this.groups()) {
      const row = this.store
        .remoteSpaces(g.machine.name)
        .flatMap((s) => s.rows)
        .find((r) => r.pane.terminal_id === id);
      if (row) return this.pick(row.pane, g.usable);
    }
  }

  protected readonly groups = computed(() =>
    this.machines.machines().map((machine) => ({
      machine,
      note: machineNote(machine),
      hint: machine.state === "offline" ? machineHint(machine) : null,
      usable: machineUsable(machine),
      blocked: this.machines.blockedReason(qualifyId(machine.name, "")),
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

  /** The `+` of the empty heading: the add form on the Machines page. */
  protected add(): void {
    this.machines.openForm();
    this.editor.openSettings("machines");
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
