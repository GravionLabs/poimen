import {
  ChangeDetectionStrategy,
  Component,
  Input,
  OnInit,
  computed,
  inject,
  signal,
} from "@angular/core";
import { HxButton, HxTag, HxField, HxInput, HxMessage } from "@gravionlabs/helix-ui";
import { ContextMenu } from "../ui/context-menu/context-menu";
import { plainGroups } from "../ui/context-menu/menu-model";
import {
  DEFAULT_REPOS_DIR,
  Machines,
  checkIcon,
  checkPassed,
  machineHint,
  machineTag,
  machineWarnings,
  serverInstallOffer,
  type MachineCheck,
  type MachineFieldError,
  type MachineStatus,
  type SshHost,
} from "../core/machines";
import { ServerInstall } from "../core/server-install";
import { SettingsStore } from "../core/settings";
import { SpacesStore } from "../core/spaces-store";
import { UiState } from "../core/ui-state";
import { DialogShell } from "../ui/dialog-shell/dialog-shell";
import { ServerInstallDialog } from "./server-install-dialog";

/** The form to add a machine or edit `original`; the errors are the validation of `machines.rs`. */
@Component({
  selector: "app-machine-form",
  imports: [DialogShell, HxButton, HxField, HxInput, HxMessage],
  changeDetection: ChangeDetectionStrategy.OnPush,
  templateUrl: "./machine-form.html",
})
export class MachineForm implements OnInit {
  protected readonly machines = inject(Machines);
  @Input() original: string | null = null;
  protected readonly defaultReposDir = DEFAULT_REPOS_DIR;
  protected readonly name = signal("");
  protected readonly ssh = signal("");
  protected readonly reposDir = signal("");
  protected readonly errors = signal<MachineFieldError[]>([]);
  protected readonly busy = signal(false);
  /** Hosts of `~/.ssh/config`, offered when adding a machine; the input stays free. */
  protected readonly suggestions = signal<SshHost[]>([]);

  /** Fills the SSH target and, when still empty, the name. */
  protected pick(h: SshHost): void {
    this.ssh.set(h.host);
    if (!this.name().trim()) this.name.set(h.host);
  }

  ngOnInit(): void {
    if (!this.original) void this.machines.sshHosts().then((h) => this.suggestions.set(h));
    const m = this.machines.machines().find((x) => x.name === this.original);
    if (!m) return;
    this.name.set(m.name);
    this.ssh.set(m.ssh);
    this.reposDir.set(m.reposDir === DEFAULT_REPOS_DIR ? "" : m.reposDir);
  }

  protected fieldError(field: MachineFieldError["field"]): string | null {
    return (
      this.errors()
        .filter((e) => e.field === field)
        .map((e) => e.message)
        .join("; ") || null
    );
  }

  protected async submit(event: Event): Promise<void> {
    event.preventDefault();
    this.busy.set(true);
    const errors = await this.machines.save(this.original, {
      name: this.name(),
      ssh: this.ssh(),
      reposDir: this.reposDir(),
    });
    this.busy.set(false);
    this.errors.set(errors);
    if (!errors.length) this.machines.closeForm();
  }
}

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
  imports: [ContextMenu, DialogShell, HxButton, HxTag, HxMessage, MachineForm, ServerInstallDialog],
  changeDetection: ChangeDetectionStrategy.OnPush,
  templateUrl: "./machines-page.html",
  styles: `
    :host {
      display: block;
    }
    .hint {
      max-width: 80ch;
      margin: 8px 0;
      font-size: 12px;
      color: var(--h-text-muted-color);
    }
    .rows {
      margin: 0;
      padding: 0;
      list-style: none;
    }
    .row-item {
      padding: 8px 0;
      align-items: center;
    }
    .row-item + .row-item {
      border-top: 1px solid var(--h-content-border-color);
    }
    .name {
      font-weight: 600;
    }
    .toolbar {
      margin: 8px 0;
    }
    .actions {
      margin-left: auto;
      display: flex;
      gap: 6px;
    }
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
      color: var(--status-done);
    }
    [data-status="warning"] .status {
      color: var(--status-blocked);
    }
    [data-status="missing"] .status {
      color: var(--status-error);
    }
    .fix {
      user-select: all;
      flex-basis: 100%;
      margin: 0;
      padding: 4px 8px;
      overflow-x: auto;
      border: 1px solid var(--h-content-border-color);
      border-radius: 4px;
    }
    .guidance,
    .progress {
      flex-basis: 100%;
      margin: 0;
    }
  `,
})
export class MachinesPage {
  protected readonly machines = inject(Machines);
  protected readonly ui = inject(UiState);
  private readonly store = inject(SpacesStore);
  private readonly settings = inject(SettingsStore);
  protected readonly warnings = computed(() => machineWarnings(this.settings.settings().warnings));
  protected readonly removing = signal<string | null>(null);
  protected readonly removeError = signal<string | null>(null);
  protected readonly menu = signal<{ name: string; x: number; y: number } | null>(null);
  protected readonly menuGroups = plainGroups([{ id: "check", label: "Check machine" }]);
  protected readonly label = checkLabel;
  protected readonly passed = checkPassed;

  /** Labels of the spaces open on a machine, for the removal confirmation. */
  protected openSpaces(name: string): string[] {
    return this.store.remoteTree(name).map((n) => n.workspace.label);
  }

  protected async confirmRemove(name: string): Promise<void> {
    const error = await this.machines.remove(name);
    this.removeError.set(error);
    if (!error) this.removing.set(null);
  }

  protected readonly serverInstall = inject(ServerInstall);
  protected readonly offer = serverInstallOffer;
  protected readonly tag = machineTag;
  protected readonly hint = machineHint;
  protected readonly icon = checkIcon;
  /** The key of the fix whose Copy was pressed last, shown as "Copied" for a moment. */
  protected readonly copied = signal<string | null>(null);

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

  protected copy(key: string, text: string): void {
    void Promise.resolve(navigator.clipboard?.writeText(text)).then(() => {
      this.copied.set(key);
      setTimeout(() => this.copied.update((c) => (c === key ? null : c)), 2000);
    });
  }
}
