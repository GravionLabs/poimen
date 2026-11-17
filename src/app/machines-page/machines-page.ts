import {
  ChangeDetectionStrategy,
  Component,
  Input,
  OnInit,
  computed,
  inject,
  signal,
} from "@angular/core";
import { HxButton } from "@gravionlabs/helix-ui";
import { ContextMenu } from "../ui/context-menu/context-menu";
import { plainGroups } from "../ui/context-menu/menu-model";
import {
  DEFAULT_REPOS_DIR,
  Machines,
  checkPassed,
  machineWarnings,
  type MachineCheck,
  type MachineFieldError,
  type MachineStatus,
} from "../core/machines";
import { SettingsStore } from "../core/settings";
import { SpacesStore } from "../core/spaces-store";
import { UiState } from "../core/ui-state";
import { DialogShell } from "../ui/dialog-shell/dialog-shell";

/** The form to add a machine or edit `original`; the errors are the validation of `machines.rs`. */
@Component({
  selector: "app-machine-form",
  imports: [DialogShell, HxButton],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <app-dialog-shell
      [heading]="original ? 'Edit machine' : 'Add machine'"
      width="520px"
      (cancelled)="machines.closeForm()"
    >
      <form (submit)="submit($event)" novalidate>
        <label>
          Name
          <input
            name="name"
            autocomplete="off"
            [value]="name()"
            (input)="name.set($any($event.target).value)"
          />
        </label>
        @if (fieldError("name"); as e) {
          <p class="error" role="alert">{{ e }}</p>
        }
        <label>
          SSH target
          <input
            name="ssh"
            autocomplete="off"
            placeholder="me@host or a Host alias"
            [value]="ssh()"
            (input)="ssh.set($any($event.target).value)"
          />
        </label>
        <p class="hint">One host as in ~/.ssh/config; no options. No password is stored.</p>
        @if (fieldError("ssh"); as e) {
          <p class="error" role="alert">{{ e }}</p>
        }
        <label>
          Repositories directory
          <input
            name="reposDir"
            autocomplete="off"
            [placeholder]="defaultReposDir"
            [value]="reposDir()"
            (input)="reposDir.set($any($event.target).value)"
          />
        </label>
        @if (fieldError("reposDir"); as e) {
          <p class="error" role="alert">{{ e }}</p>
        }
        @if (fieldError(null); as e) {
          <p class="error" role="alert">{{ e }}</p>
        }
        <div class="dialog-buttons">
          <button
            type="button"
            hx-button
            variant="text"
            severity="secondary"
            class="cancel"
            (click)="machines.closeForm()"
          >
            Cancel
          </button>
          <button type="submit" hx-button class="confirm" [disabled]="busy()">Save</button>
        </div>
      </form>
    </app-dialog-shell>
  `,
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

  ngOnInit(): void {
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
  imports: [ContextMenu, DialogShell, HxButton, MachineForm],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <section aria-label="Machines">
      <p class="hint">
        Remote machines over SSH. Right-click a machine to check its prerequisites; the commands
        listed for a missing item are for you to run on the machine.
      </p>
      <div class="toolbar">
        <button type="button" hx-button size="small" class="add" (click)="machines.openForm()">
          Add machine
        </button>
      </div>
      @for (w of warnings(); track $index) {
        <p class="error warning" role="alert">{{ w }}</p>
      }
      @if (machines.machines().length === 0) {
        <p class="hint">No machine is configured.</p>
        <p class="hint">
          A machine needs SSH access without a password prompt (a key or the agent) and
          <code>poimen-server</code> installed on it. poimen stores no secret; it runs
          <code>ssh &lt;target&gt; poimen-server stdio</code>.
        </p>
        @if (ui.machinesHidden()) {
          <button
            type="button"
            hx-button
            size="small"
            variant="text"
            severity="secondary"
            class="show-sidebar"
            (click)="ui.setMachinesHidden(false)"
          >
            Show Machines in the sidebar
          </button>
        }
      } @else {
        <ul class="rows" aria-label="Machines">
          @for (m of machines.machines(); track m.name) {
            <li class="row-item machine" (contextmenu)="openMenu($event, m)">
              <span class="name">{{ m.name }}</span>
              <span class="state">{{ stateText(m) }}</span>
              <span class="actions">
                <button
                  type="button"
                  hx-button
                  size="small"
                  variant="outlined"
                  severity="secondary"
                  class="edit"
                  [attr.aria-label]="'Edit ' + m.name"
                  (click)="machines.openForm(m.name)"
                >
                  Edit
                </button>
                <button
                  type="button"
                  hx-button
                  size="small"
                  variant="outlined"
                  severity="danger"
                  class="remove"
                  [attr.aria-label]="'Remove ' + m.name"
                  (click)="removing.set(m.name)"
                >
                  Remove
                </button>
              </span>
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
                          <button
                            type="button"
                            hx-button
                            size="small"
                            variant="text"
                            severity="secondary"
                            class="copy"
                            (click)="copy(c.fix)"
                          >
                            Copy
                          </button>
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
      @if (machines.form(); as f) {
        <app-machine-form [original]="f.original" />
      }
      @if (removing(); as name) {
        <app-dialog-shell
          heading="Remove machine?"
          width="520px"
          [alert]="true"
          [cancelDefault]="true"
          (cancelled)="removing.set(null)"
        >
          <p class="hint">
            Remove <strong>{{ name }}</strong> from the settings? poimen disconnects from it; its
            terminals keep running on the machine and are not ended.
          </p>
          @if (openSpaces(name); as spaces) {
            @if (spaces.length) {
              <p class="hint">Open spaces on it: {{ spaces.join(", ") }}.</p>
            }
          }
          @if (removeError(); as e) {
            <p class="error" role="alert">{{ e }}</p>
          }
          <div class="dialog-buttons">
            <button
              type="button"
              hx-button
              variant="text"
              severity="secondary"
              class="cancel"
              (click)="removing.set(null)"
            >
              Cancel
            </button>
            <button
              type="button"
              hx-button
              severity="danger"
              class="confirm"
              (click)="confirmRemove(name)"
            >
              Remove
            </button>
          </div>
        </app-dialog-shell>
      }
      @if (menu(); as m) {
        <app-context-menu
          [at]="m"
          [groups]="menuGroups"
          (picked)="checkMachine()"
          (closed)="closeMenu()"
        />
      }
    </section>
  `,
  styles: `
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
