/**
 * The live state of agent runs, read from the agent-events core (GravionLabs/agent-events,
 * `SPEC.md`): the backend folds `<git-common-dir>/.agents/events/*.jsonl` (`agent_events.rs`, run
 * state of `run-state.schema.json` plus `tone`, `host`, `agent` and `events`). Displays are core
 * readers: nothing here reads a profile, and a run shows from the core alone. Fallback during the
 * migration: the legacy `.agents/deliver-loop/status.json` (`schemaVersion: 1`), mapped onto the
 * same shape by {@link fromLegacy}. Everything here is pure; the Tauri side is `deliver.ts`.
 */

/** The legacy status file schema version this build understands. */
export const SUPPORTED_SCHEMA = 1;

/** The version of the events this build reads (`v`, as `SUPPORTED_V` of the backend). */
export const SUPPORTED_EVENTS = 1;

/** A run that is `running` and not updated for this long is stale. */
export const STALE_MS = 60 * 60 * 1000;

/** How the writer rated an event or a run; the displays map it to their palette, never the text. */
export type Tone = "neutral" | "info" | "success" | "warning" | "danger";

const TONES: readonly Tone[] = ["neutral", "info", "success", "warning", "danger"];

export type RunStatus = "running" | "finished" | "abandoned";
const STATUSES: readonly RunStatus[] = ["running", "finished", "abandoned"];

export type Outcome = "succeeded" | "failed" | "cancelled" | "skipped";
const OUTCOMES: readonly Outcome[] = ["succeeded", "failed", "cancelled", "skipped"];

export interface Link {
  rel: string;
  url: string;
  title: string | null;
}

/** What is worked on: `kind` is free text (`github.issue`, `github.milestone`, ...). */
export interface Subject {
  kind: string;
  id: string;
  title: string;
  url: string | null;
}

export interface Step {
  name: string;
  n: number | null;
  total: number | null;
  text: string | null;
}

export interface Progress {
  done: number;
  total: number;
  label: string | null;
}

/** A task that was started and not finished. */
export interface OpenTask {
  task: string;
  parent: string | null;
  subject: Subject;
  step: Step | null;
  progress: Progress | null;
  links: Link[];
}

export interface FinishedTask {
  task: string;
  outcome: Outcome;
  subject: Subject;
  reason: string | null;
  links: Link[];
}

export interface Attention {
  id: string;
  task: string | null;
  reason: string;
  needsYou: boolean;
  links: Link[];
}

/** One event of a run, as the Events tab of the bottom panel lists it. */
export interface RunEventRow {
  seq: number | null;
  ts: string | null;
  type: string;
  tone: Tone;
  task: string | null;
  step: string | null;
  outcome: string | null;
  link: string | null;
}

/** A run as the panel shows it: the core run state plus tone and host. */
export interface DeliverStatus {
  run: string;
  status: RunStatus;
  /** `succeeded`, `failed` or `cancelled` once the run finished. */
  outcome: string | null;
  reason: string | null;
  /** What the run as a whole works on (a milestone, a release). */
  subject: Subject | null;
  progress: Progress | null;
  tasks: OpenTask[];
  finished: FinishedTask[];
  attentions: Attention[];
  needsYou: boolean;
  links: Link[];
  tone: Tone;
  /** `host` of `run.started`: namespaced keys (`poimen.terminal`, `herdr.pane`). */
  host: Record<string, string>;
  harness: string | null;
  model: string | null;
  events: RunEventRow[];
  updated: string;
}

/** What the `deliver` event carries for one repository (`Read` of `deliver.rs`). */
export type DeliverRead =
  | { kind: "missing" }
  | { kind: "ok"; status: unknown }
  | { kind: "unsupported"; schemaVersion?: unknown }
  | { kind: "invalid"; error: string }
  | { kind: "events"; runs: unknown[]; unknownVersion?: number | null };

/** The state of one card. */
export type CardState =
  | { kind: "ok"; status: DeliverStatus }
  | { kind: "unsupported"; version: string }
  | { kind: "events-version"; version: string }
  | { kind: "invalid"; error: string };

/** One card of the panel: a run of a repository (`run` is empty for a hint or an error). */
export interface DeliverCard {
  root: string;
  run: string;
  state: CardState;
}

const isObject = (v: unknown): v is Record<string, unknown> =>
  typeof v === "object" && v !== null && !Array.isArray(v);
const isString = (v: unknown): v is string => typeof v === "string";
const isCount = (v: unknown): v is number => typeof v === "number" && Number.isInteger(v) && v >= 0;
const str = (v: unknown): string | null => (isString(v) && v !== "" ? v : null);
const isTone = (v: unknown): v is Tone => TONES.includes(v as Tone);

function link(v: unknown): Link | null {
  if (!isObject(v)) return null;
  const rel = str(v["rel"]);
  const url = str(v["url"]);
  return rel && url ? { rel, url, title: str(v["title"]) } : null;
}

const links = (v: unknown): Link[] => (Array.isArray(v) ? v.flatMap((l) => link(l) ?? []) : []);

function subject(v: unknown): Subject | null {
  if (!isObject(v)) return null;
  const id = str(v["id"]) ?? "";
  const title = str(v["title"]) ?? id;
  if (!title) return null;
  return { kind: str(v["kind"]) ?? "", id, title, url: str(v["url"]) };
}

function step(v: unknown): Step | null {
  if (!isObject(v)) return null;
  const name = str(v["name"]);
  if (!name) return null;
  return {
    name,
    n: isCount(v["n"]) ? v["n"] : null,
    total: isCount(v["total"]) ? v["total"] : null,
    text: str(v["text"]),
  };
}

function progress(v: unknown): Progress | null {
  if (!isObject(v) || !isCount(v["done"]) || !isCount(v["total"])) return null;
  return { done: v["done"], total: v["total"], label: str(v["label"]) };
}

/** Tolerant: a malformed entry is dropped, the rest of the run still shows. */
function tasks(v: unknown): OpenTask[] {
  if (!Array.isArray(v)) return [];
  return v.flatMap((t) => {
    if (!isObject(t)) return [];
    const id = str(t["task"]);
    const sub = subject(t["subject"]);
    if (!id || !sub) return [];
    return [
      {
        task: id,
        parent: str(t["parent"]),
        subject: sub,
        step: step(t["step"]),
        progress: progress(t["progress"]),
        links: links(t["links"]),
      },
    ];
  });
}

function finishedTasks(v: unknown): FinishedTask[] {
  if (!isObject(v)) return [];
  return OUTCOMES.flatMap((outcome) => {
    const list = v[outcome];
    if (!Array.isArray(list)) return [];
    return list.flatMap((t) => {
      if (!isObject(t)) return [];
      const id = str(t["task"]);
      const sub = subject(t["subject"]);
      return id && sub
        ? [{ task: id, outcome, subject: sub, reason: str(t["reason"]), links: links(t["links"]) }]
        : [];
    });
  });
}

function attentions(v: unknown): Attention[] {
  if (!Array.isArray(v)) return [];
  return v.flatMap((a) => {
    if (!isObject(a)) return [];
    const id = str(a["id"]);
    if (!id) return [];
    return [
      {
        id,
        task: str(a["task"]),
        reason: str(a["reason"]) ?? "",
        needsYou: a["needsYou"] === true,
        links: links(a["links"]),
      },
    ];
  });
}

function eventRows(value: unknown): RunEventRow[] {
  if (!Array.isArray(value)) return [];
  const rows: RunEventRow[] = [];
  for (const v of value) {
    if (!isObject(v) || !isString(v["type"])) continue;
    rows.push({
      seq: isCount(v["seq"]) ? v["seq"] : null,
      ts: str(v["ts"]),
      type: v["type"],
      tone: isTone(v["tone"]) ? v["tone"] : "neutral",
      task: str(v["task"]),
      step: str(v["step"]),
      outcome: str(v["outcome"]),
      link: str(v["link"]),
    });
  }
  return rows;
}

/**
 * Validates the core run state the panel uses; unknown fields (and profile extensions) are
 * ignored, a wrong known field fails.
 */
export function parseStatus(value: unknown): DeliverStatus | string {
  if (!isObject(value)) return "the run is not an object";
  const run = str(value["run"]);
  if (!run) return "run is missing";
  const status = value["status"];
  if (!STATUSES.includes(status as RunStatus)) return "status is unknown";
  const updated = value["updated"];
  if (!isString(updated) || Number.isNaN(Date.parse(updated))) return "updated is not a date";
  const host: Record<string, string> = {};
  if (isObject(value["host"]))
    for (const [k, v] of Object.entries(value["host"])) if (isString(v)) host[k] = v;
  const agent = isObject(value["agent"]) ? value["agent"] : {};
  return {
    run,
    status: status as RunStatus,
    outcome: str(value["outcome"]),
    reason: str(value["reason"]),
    subject: subject(value["subject"]),
    progress: progress(value["progress"]),
    tasks: tasks(value["tasks"]),
    finished: finishedTasks(value["finished"]),
    attentions: attentions(value["attentions"]),
    needsYou: value["needsYou"] === true,
    links: links(value["links"]),
    tone: isTone(value["tone"]) ? value["tone"] : "neutral",
    host,
    harness: str(agent["harness"]),
    model: str(agent["model"]),
    events: eventRows(value["events"]),
    updated,
  };
}

/** Only a `/pull/<n>` URL is a pull request; the producer once wrote the issue URL (#538). */
const asPullUrl = (v: unknown): string | null =>
  isString(v) && /\/pull\/\d+\/?$/.test(v) ? v : null;

type LegacyResult = "running" | "done" | "max reached" | "blocked" | "no progress" | "abandoned";
const LEGACY_RESULTS: readonly string[] = [
  "running",
  "done",
  "max reached",
  "blocked",
  "no progress",
  "abandoned",
];

/**
 * Maps the legacy `status.json` of the deliver loop onto the core shape: the milestone is the run
 * subject with its progress, the current PBI an open task, delivered PBIs succeeded tasks, the
 * items to decide attentions, the release PR and RC links. A status file has no tone: a blocked
 * result is danger, a running run that asks for the user warning, a finished run success.
 */
export function fromLegacy(value: unknown): DeliverStatus | string {
  if (!isObject(value)) return "the status is not an object";
  const run = str(value["run"]);
  const updated = value["updatedAt"];
  const result = value["result"];
  if (!run) return "run is missing";
  if (!isString(updated) || Number.isNaN(Date.parse(updated))) return "updatedAt is not a date";
  if (!isString(result) || !LEGACY_RESULTS.includes(result)) return "result is unknown";
  const items = (v: unknown): { number: number; url: string; reason: string }[] =>
    Array.isArray(v)
      ? v.flatMap((i) =>
          isObject(i) && isCount(i["number"]) && isString(i["url"]) && isString(i["reason"])
            ? [{ number: i["number"], url: i["url"], reason: i["reason"] }]
            : [],
        )
      : [];
  const blocked = items(value["blocked"]);
  const paused = items(value["paused"]);
  let needsYou = items(value["needsYou"]);
  const milestone = isObject(value["milestone"]) ? value["milestone"] : null;
  const closed = isCount(value["closed"]) ? value["closed"] : 0;
  const total = isCount(value["total"]) ? value["total"] : 0;
  const rel = isObject(value["release"]) ? value["release"] : {};
  // A status file that says "done" while milestone items are still open and nothing is blocked or
  // paused is a producer artifact between two iterations (#537): the loop still runs.
  const premature =
    result === "done" &&
    milestone !== null &&
    closed < total &&
    blocked.length + paused.length === 0 &&
    value["source"] == null;
  const releaseRan = [rel["reviewRound"], rel["rcUrl"], rel["rcVersion"]].some((v) => v != null);
  if (premature && !releaseRan) needsYou = needsYou.filter((i) => !/release pr/i.test(i.reason));
  const state: LegacyResult = premature ? "running" : (result as LegacyResult);
  const issue = (n: number, title: string, url: string): Subject => ({
    kind: "github.issue",
    id: `#${n}`,
    title: `#${n} ${title}`.trim(),
    url,
  });
  const cur = isObject(value["current"]) ? value["current"] : null;
  const curStep = cur && isObject(cur["step"]) ? cur["step"] : null;
  const curPr = cur ? asPullUrl(cur["prUrl"]) : null;
  const open: OpenTask[] =
    cur && isCount(cur["number"]) && isString(cur["title"]) && isString(cur["url"])
      ? [
          {
            task: `#${cur["number"]}`,
            parent: null,
            subject: issue(cur["number"], cur["title"], cur["url"]),
            step: curStep
              ? {
                  name: str(curStep["text"]) ?? "",
                  n: isCount(curStep["n"]) ? curStep["n"] : null,
                  total: isCount(curStep["total"]) ? curStep["total"] : null,
                  text: null,
                }
              : null,
            progress: null,
            links: curPr ? [{ rel: "pull-request", url: curPr, title: null }] : [],
          },
        ]
      : [];
  const done: FinishedTask[] = Array.isArray(value["delivered"])
    ? value["delivered"].flatMap((d) => {
        if (!isObject(d) || !isCount(d["pbi"]) || !isString(d["url"])) return [];
        const pr = asPullUrl(d["prUrl"]);
        return [
          {
            task: `#${d["pbi"]}`,
            outcome: "succeeded" as const,
            subject: issue(d["pbi"], "", d["url"]),
            reason: null,
            links: pr ? [{ rel: "pull-request", url: pr, title: null }] : [],
          },
        ];
      })
    : [];
  const asks = (list: typeof blocked, label: string, needs: boolean): Attention[] =>
    list.map((i) => ({
      id: `${label}-${i.number}`,
      task: null,
      reason: `#${i.number} ${i.reason}`,
      needsYou: needs,
      links: [{ rel: "issue", url: i.url, title: null }],
    }));
  const list = [
    ...asks(needsYou, "needs-you", true),
    ...asks(blocked, "blocked", false),
    ...asks(paused, "paused", false),
  ];
  const runLinks: Link[] = [];
  const relPr = str(rel["prUrl"]);
  const relRc = str(rel["rcUrl"]);
  if (relPr && !(premature && !releaseRan))
    runLinks.push({ rel: "pull-request", url: relPr, title: "Release" });
  if (relRc) runLinks.push({ rel: "release-candidate", url: relRc, title: str(rel["rcVersion"]) });
  const tone: Tone =
    state === "blocked"
      ? "danger"
      : list.length > 0 && state === "running"
        ? "warning"
        : state === "done"
          ? "success"
          : "neutral";
  const outcome = state === "done" ? "succeeded" : state === "max reached" ? "cancelled" : "failed";
  const finished = state !== "running" && state !== "abandoned";
  const ms = milestone && isString(milestone["title"]) ? milestone : null;
  return {
    run,
    status: finished ? "finished" : state === "abandoned" ? "abandoned" : "running",
    outcome: finished ? outcome : null,
    reason: finished && state !== "done" ? state : null,
    subject: ms
      ? {
          kind: "github.milestone",
          id: str(ms["key"]) ?? "",
          title: ms["title"] as string,
          url: str(ms["url"]),
        }
      : null,
    progress: ms ? { done: closed, total, label: null } : null,
    tasks: finished ? [] : open,
    finished: done,
    attentions: list,
    needsYou: needsYou.length > 0,
    links: runLinks,
    tone,
    host: {},
    harness: str(value["harness"]),
    model: str(value["model"]),
    events: [],
    updated,
  };
}

/** The key of a card: the repository and the run. */
export const cardKey = (card: Pick<DeliverCard, "root" | "run">): string =>
  `${card.root}\n${card.run}`;

function card(root: string, status: DeliverStatus | string): DeliverCard {
  return typeof status === "string"
    ? { root, run: "", state: { kind: "invalid", error: status } }
    : { root, run: status.run, state: { kind: "ok", status } };
}

/** The cards of a `deliver` event: none for a repository without runs, one per run of its events. */
export function cardsOf(root: string, read: DeliverRead): DeliverCard[] {
  switch (read.kind) {
    case "missing":
      return [];
    case "invalid":
      return [{ root, run: "", state: { kind: "invalid", error: read.error } }];
    case "unsupported":
      return [
        {
          root,
          run: "",
          state: {
            kind: "unsupported",
            version:
              read.schemaVersion === undefined || read.schemaVersion === null
                ? "none"
                : String(read.schemaVersion),
          },
        },
      ];
    case "ok":
      return [card(root, fromLegacy(read.status))];
    case "events": {
      const cards = read.runs.map((run) => card(root, parseStatus(run)));
      if (read.unknownVersion !== undefined && read.unknownVersion !== null)
        cards.push({
          root,
          run: "",
          state: { kind: "events-version", version: String(read.unknownVersion) },
        });
      return cards;
    }
  }
}

/** `running` for over {@link STALE_MS} without an update, and `abandoned`, are `stale`. */
export type RunState = "running" | "stale" | "succeeded" | "failed" | "cancelled";

export function runState(status: DeliverStatus, now: number): RunState {
  if (status.status === "abandoned") return "stale";
  if (status.status === "running")
    return now - Date.parse(status.updated) > STALE_MS ? "stale" : "running";
  return status.outcome === "failed" || status.outcome === "cancelled"
    ? status.outcome
    : "succeeded";
}

/** "just now", "12 min ago", "3 h ago", "2 d ago". */
export function ageText(iso: string, now: number): string {
  const ms = now - Date.parse(iso);
  if (Number.isNaN(ms)) return "";
  const min = Math.floor(Math.max(ms, 0) / 60000);
  if (min < 1) return "just now";
  if (min < 60) return `${min} min ago`;
  const hours = Math.floor(min / 60);
  if (hours < 24) return `${hours} h ago`;
  return `${Math.floor(hours / 24)} d ago`;
}

/** The line under the title: "running, updated 3 min ago", "finished 12 min ago", ... */
export function stateText(status: DeliverStatus, now: number): string {
  const age = ageText(status.updated, now);
  const why = status.reason ? `: ${status.reason}` : "";
  switch (runState(status, now)) {
    case "running":
      return `running, updated ${age}`;
    case "stale":
      return `stale: no update for ${age.replace(" ago", "")}, the run may have stopped`;
    case "succeeded":
      return `finished ${age}`;
    case "failed":
      return `failed ${age}${why}`;
    case "cancelled":
      return `cancelled ${age}${why}`;
  }
}

/** Progress in percent, 0 to 100. */
export function percent(progress: Pick<Progress, "done" | "total">): number {
  if (progress.total <= 0) return 0;
  return Math.min(100, Math.round((progress.done / progress.total) * 100));
}

/** The words of a progress: "3/9 closed", "1/3". */
export const progressText = (p: Progress): string =>
  `${p.done}/${p.total}${p.label ? ` ${p.label}` : ""}`;

/** Whether the tone of the run asks for a look: warning or danger (never read from text). */
export function needsAttention(status: DeliverStatus): boolean {
  return status.tone === "warning" || status.tone === "danger";
}

/** "implement 2/3", "review": the step name with its position when the writer gave one. */
export function stepText(step: Step): string {
  return step.n !== null && step.total !== null
    ? `${step.name} ${step.n}/${step.total}`
    : step.name;
}

/** `HH:MM` of an ISO timestamp in local time; empty when it is missing or not a date. */
export function clockText(ts: string | null | undefined): string {
  const date = ts ? new Date(ts) : null;
  if (!date || Number.isNaN(date.getTime())) return "";
  const two = (n: number) => String(n).padStart(2, "0");
  return `${two(date.getHours())}:${two(date.getMinutes())}`;
}

/** The last segment of a path (either separator), without a trailing separator. */
const folderName = (root: string): string =>
  root
    .replace(/[\\/]+$/, "")
    .split(/[\\/]/)
    .pop() ?? "";

/**
 * What a run is called: its subject title, else `<repository folder> · <harness> · <start HH:MM>`
 * (the known parts; the start is the `ts` of the first event), the run id only as the last
 * fallback.
 */
export function runTitle(status: DeliverStatus, root?: string): string {
  if (status.subject?.title) return status.subject.title;
  const parts = [
    root ? folderName(root) : "",
    status.harness ?? "",
    clockText(status.events.find((e) => e.ts)?.ts),
  ].filter((p) => p !== "");
  return parts.length > 0 ? parts.join(" · ") : status.run;
}

/** The compact status bar item. */
export interface BarSummary {
  text: string;
  title: string;
  warning: boolean;
}

/**
 * `▶ M5 7/19` per running run, `▶ n runs` for several; `null` without a running run. A fresh run
 * that ended with a warning or danger tone still shows until it is stale, so a stop is not
 * missed; the warning mark follows the tone.
 */
export function barSummary(cards: readonly DeliverCard[], now: number): BarSummary | null {
  const active: DeliverStatus[] = [];
  for (const c of cards) {
    if (c.state.kind !== "ok") continue;
    const state = runState(c.state.status, now);
    if (
      state === "running" ||
      (state !== "stale" &&
        needsAttention(c.state.status) &&
        now - Date.parse(c.state.status.updated) < STALE_MS)
    )
      active.push(c.state.status);
  }
  if (active.length === 0) return null;
  const label = (s: DeliverStatus) =>
    `${runTitle(s)}${s.progress ? ` ${s.progress.done}/${s.progress.total}` : ""}`;
  const warning = active.some(needsAttention);
  const title = active
    .map((s) => {
      const t = s.tasks[0];
      const doing = t ? `: ${t.subject.title}${t.step ? ` ${t.step.name}` : ""}` : "";
      return `${label(s)}${doing}${s.needsYou ? " (needs you)" : ""}`;
    })
    .join("\n");
  const text =
    active.length === 1
      ? `${warning ? "⚠" : "▶"} ${label(active[0])}`
      : `${warning ? "⚠" : "▶"} ${active.length} runs`;
  return { text, title, warning };
}
