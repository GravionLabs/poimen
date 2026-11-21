import {
  ChangeDetectionStrategy,
  Component,
  computed,
  inject,
  output,
  signal,
} from "@angular/core";
import { type Terminal } from "../core/snapshot";
import { HOOK_AGENT_KINDS, startableKinds, statusNote } from "../core/backend";
import { TauriBridge } from "../core/tauri";
import { parseHarnesses, type HarnessInfo } from "../core/settings";
import { PaneActions, type PaneClosed } from "../core/pane-actions";
import { DialogShell } from "../ui/dialog-shell/dialog-shell";
import { Icon } from "../ui/icon/icon";
import { HxButton } from "@gravionlabs/helix-ui";

/** The question for a new pane (its type) or a pane close (confirmation), and action errors. */
@Component({
  selector: "app-pane-dialog",
  imports: [DialogShell, HxButton, Icon],
  changeDetection: ChangeDetectionStrategy.OnPush,
  templateUrl: "./pane-dialog.html",
  styleUrl: "./pane-dialog.css",
})
export class PaneDialog {
  protected readonly actions = inject(PaneActions);
  /** A terminal was created; the host opens it. */
  readonly created = output<Terminal>();
  /** A pane was closed; the host drops its tab or hands it to the successor. */
  readonly closed = output<PaneClosed>();
  /** Only the kinds that can report (#410). */
  private readonly bridge = inject(TauriBridge);
  private readonly harnesses = signal<HarnessInfo[]>([]);
  protected readonly kinds = computed(() => startableKinds(this.harnesses().map((h) => h.id)));
  protected readonly note = (kind: string) => statusNote(kind);
  /** The types of a new pane besides Terminal: the installed harnesses (#1049). */
  protected readonly agents = computed(() => this.harnesses().filter((h) => h.available));

  constructor() {
    this.bridge
      .invoke<unknown>("get_harnesses")
      .then((raw) => this.harnesses.set(parseHarnesses(raw)))
      .catch(() => this.harnesses.set([]));
  }

  protected async create(form: HTMLFormElement): Promise<void> {
    const type = String(new FormData(form).get("type") ?? "terminal");
    const harness = type === "terminal" ? undefined : this.agents().find((h) => h.id === type);
    // A harness without hooks runs its own program and arguments, not its id as the program.
    const [program = type, ...base] = harness?.command ?? [];
    const agent = !harness
      ? undefined
      : HOOK_AGENT_KINDS.includes(type) || harness.command.length === 0
        ? { kind: type, args: [] }
        : { kind: program, args: base };
    const pane = await this.actions.confirmNew(agent);
    if (pane) this.created.emit(pane);
  }

  protected async close(): Promise<void> {
    const closed = await this.actions.confirmClose();
    if (closed) this.closed.emit(closed);
  }

  protected async restart(): Promise<void> {
    const closed = await this.actions.confirmRestart();
    if (closed) this.closed.emit(closed);
  }

  protected async startAgent(form: HTMLFormElement): Promise<void> {
    const data = new FormData(form);
    const field = (key: string) => String(data.get(key) ?? "");
    const kind = field("kind").trim();
    const args = field("args");
    // A harness without hooks runs its own program and arguments, not its id as the program.
    const harness = this.harnesses().find((h) => h.id === kind);
    if (harness && harness.command.length > 0 && !HOOK_AGENT_KINDS.includes(kind)) {
      const [program = kind, ...base] = harness.command;
      await this.actions.confirmStartAgent(program, field("name"), [...base, args].join(" "));
      return;
    }
    await this.actions.confirmStartAgent(kind, field("name"), args);
  }

  protected async saveLayout(form: HTMLFormElement): Promise<void> {
    await this.actions.confirmSaveLayout(String(new FormData(form).get("name") ?? ""));
  }

  protected cancel(): void {
    this.actions.cancel();
  }
}
