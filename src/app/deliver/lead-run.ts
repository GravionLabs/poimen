import { qualifyId, type Terminal } from "../core/snapshot";
import type { ActiveGrant } from "../core/agent-grants";
import {
  cardKey,
  runState,
  runTitle,
  stateText,
  type DeliverCard,
  type DeliverStatus,
  type Tone,
} from "./deliver-status";
import { taskTerminals } from "./task-terminal";

/** A started loop of a lead run (`orchestrate.member`). */
export interface LeadMember {
  task: string;
  repository: string;
  deliverRun: string | null;
  terminal: string | null;
  machine: string | null;
}

/** `task` waits for `blockedBy` (`orchestrate.dependency`). */
export interface LeadDependency {
  task: string;
  blockedBy: string;
  note: string | null;
}

/** What the run file records of a lead run (`Run.lead` of the backend); the file is the only store. */
export interface LeadRecord {
  members: LeadMember[];
  dependencies: LeadDependency[];
  /** The id of the grant the lead recorded; the live grant is the server's. */
  grantId: string | null;
  /** The terminal of the newest `orchestrate.resume`. */
  resumedIn: string | null;
}

const isObject = (v: unknown): v is Record<string, unknown> =>
  typeof v === "object" && v !== null && !Array.isArray(v);
const text = (v: unknown): string | null => (typeof v === "string" && v !== "" ? v : null);

/** The lead record of one run as the backend sent it; `null` for a run that has none. */
export function parseLead(run: unknown): LeadRecord | null {
  const lead = isObject(run) ? run["lead"] : undefined;
  if (!isObject(lead)) return null;
  const list = (v: unknown): Record<string, unknown>[] =>
    Array.isArray(v) ? v.filter(isObject) : [];
  const members: LeadMember[] = [];
  for (const m of list(lead["members"])) {
    const task = text(m["task"]);
    const repository = text(m["repository"]);
    if (!task || !repository) continue;
    members.push({
      task,
      repository,
      deliverRun: text(m["deliverRun"]),
      terminal: text(m["terminal"]),
      machine: text(m["machine"]),
    });
  }
  const dependencies: LeadDependency[] = [];
  for (const d of list(lead["dependencies"])) {
    const task = text(d["task"]);
    const blockedBy = text(d["blockedBy"]);
    if (task && blockedBy) dependencies.push({ task, blockedBy, note: text(d["note"]) });
  }
  const grant = isObject(lead["grant"]) ? lead["grant"] : null;
  return {
    members,
    dependencies,
    grantId: grant ? text(grant["id"]) : null,
    resumedIn: text(lead["resumedIn"]),
  };
}

/** What the lead's run says of a member: still open, or how it ended; `planned` before either. */
export type MemberState =
  | "running"
  | "blocked"
  | "succeeded"
  | "failed"
  | "cancelled"
  | "skipped"
  | "planned";

/** The `hx-tag` severity of a member state. */
export const memberSeverity = (state: MemberState): "success" | "warn" | "danger" | "secondary" =>
  state === "succeeded"
    ? "success"
    : state === "blocked"
      ? "warn"
      : state === "failed"
        ? "danger"
        : "secondary";

/** A session of a member that waits for the user. */
export interface BlockedSession {
  id: string;
  text: string;
}

/** One loop of a lead run, one row of the view. */
export interface MemberRow {
  task: string;
  /** The repository folder. */
  repository: string;
  state: MemberState;
  /** The member's deliver run, when it is among the runs poimen reads. */
  run: DeliverCard | null;
  /** "running, updated 3 min ago"; empty without a run. */
  runText: string;
  /** The task the member's run works on now. */
  currentPbi: string | null;
  blocked: BlockedSession[];
  /** The tasks this member waits for that are not done yet. */
  waitingOn: string[];
  /** The terminal of the session that works now (#745), else the member's own terminal. */
  sessionTerminal: string | null;
  /** The lead's terminal, when it is open. */
  leadTerminal: string | null;
}

/** A dependency across repositories and whether it is still open. */
export interface DependencyRow {
  task: string;
  blockedBy: string;
  note: string | null;
  open: boolean;
}

/** A lead run, the model of one section of the view. */
export interface LeadRunView {
  key: string;
  card: DeliverCard;
  title: string;
  status: DeliverStatus;
  stateText: string;
  tone: Tone;
  leadTerminal: string | null;
  /** The live grant the lead recorded, with its revoke action; `null` once it ended. */
  grant: ActiveGrant | null;
  members: MemberRow[];
  dependencies: DependencyRow[];
}

/** `id` as the snapshot names it: qualified for a machine, as is on this Mac. */
const onMachine = (machine: string | undefined, id: string): string =>
  machine === undefined ? id : qualifyId(machine, id);

const folder = (root: string): string => root.split(/[\\/]/).filter(Boolean).pop() ?? root;

/** The lead's terminal when it is in the snapshot: the newest takeover, else the run's host. */
function leadTerminalOf(
  card: DeliverCard,
  status: DeliverStatus,
  lead: LeadRecord,
  open: ReadonlySet<string>,
): string | null {
  const resumed = lead.resumedIn ? onMachine(card.machine, lead.resumedIn) : null;
  const host = status.host["poimen.terminal"];
  return [resumed, host].find((id): id is string => !!id && open.has(id)) ?? null;
}

/** The member's deliver run: its id on the member's machine (else the lead's), the repository first. */
function runOfMember(
  member: LeadMember,
  lead: DeliverCard,
  cards: readonly DeliverCard[],
): DeliverCard | null {
  if (!member.deliverRun) return null;
  const machine = member.machine ?? lead.machine;
  const same = cards.filter(
    (c) => c.run === member.deliverRun && c.machine === machine && c.state.kind === "ok",
  );
  return same.find((c) => c.root === member.repository) ?? same[0] ?? null;
}

/** The member's state from the lead's own run: open task, or the outcome it finished with. */
function stateOfMember(task: string, status: DeliverStatus): MemberState {
  const open = status.tasks.some((t) => t.task === task);
  if (open) {
    return status.attentions.some((a) => a.task === task && a.needsYou) ? "blocked" : "running";
  }
  return status.finished.find((f) => f.task === task)?.outcome ?? "planned";
}

/** The lead runs among `cards`: the runs with a record in `leads` (by {@link cardKey}). */
export function leadRuns(
  cards: readonly DeliverCard[],
  leads: ReadonlyMap<string, LeadRecord>,
  terminals: readonly Terminal[],
  grants: readonly ActiveGrant[],
  now: number,
): LeadRunView[] {
  const open = new Set(terminals.map((t) => t.terminal_id));
  const views: LeadRunView[] = [];
  for (const card of cards) {
    const lead = leads.get(cardKey(card));
    if (card.state.kind !== "ok" || !lead) continue;
    const status = card.state.status;
    const leadTerminal = leadTerminalOf(card, status, lead, open);
    const states = new Map(lead.members.map((m) => [m.task, stateOfMember(m.task, status)]));
    const members = lead.members.map((m): MemberRow => {
      const run = runOfMember(m, card, cards);
      const runStatus = run?.state.kind === "ok" ? run.state.status : null;
      const current = runStatus?.tasks[0] ?? null;
      const sessions =
        run && current
          ? taskTerminals(
              terminals,
              { run: run.run, machine: run.machine, offline: run.offline },
              current.task,
            )
          : [];
      const own = m.terminal ? onMachine(m.machine ?? card.machine, m.terminal) : null;
      const blocked: BlockedSession[] = [
        ...status.attentions.filter((a) => a.task === m.task && a.needsYou),
        ...(runStatus?.attentions.filter((a) => a.needsYou) ?? []),
      ].map((a) => ({ id: a.id, text: a.text ?? a.reason }));
      const state = states.get(m.task) ?? "planned";
      return {
        task: m.task,
        repository: folder(m.repository),
        state: state === "running" && runStatus?.needsYou && blocked.length > 0 ? "blocked" : state,
        run,
        runText: runStatus ? stateText(runStatus, now) : "",
        currentPbi: current ? current.subject.title || current.task : null,
        blocked,
        waitingOn: lead.dependencies
          .filter((d) => d.task === m.task && !isDone(states.get(d.blockedBy)))
          .map((d) => d.blockedBy),
        sessionTerminal: sessions[0]?.terminal_id ?? (own && open.has(own) ? own : null),
        leadTerminal,
      };
    });
    views.push({
      key: cardKey(card),
      card,
      title: runTitle(status, card.root),
      status,
      stateText: stateText(status, now),
      tone: runState(status, now) === "stale" ? "neutral" : status.tone,
      leadTerminal,
      grant: grants.find((g) => g.id === lead.grantId) ?? null,
      members,
      dependencies: lead.dependencies.map((d) => ({
        task: d.task,
        blockedBy: d.blockedBy,
        note: d.note,
        open: !isDone(states.get(d.blockedBy)),
      })),
    });
  }
  return views;
}

const isDone = (state: MemberState | undefined): boolean =>
  state === "succeeded" || state === "skipped";
