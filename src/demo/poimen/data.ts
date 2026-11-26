// The fixed demo state of the screenshots (#1109): spaces, terminals, runs, machines. Times are
// relative to {@link NOW}, the instant the runner fixes the clock to.
import type { Snapshot, Terminal } from "../../app/core/snapshot";

/** The fixed clock of the screenshots (the runner sets it, the data counts back from it). */
export const NOW = Date.parse("2026-11-21T10:00:00Z");
const ago = (minutes: number): number => NOW - minutes * 60_000;

export const HOME = "/Users/demo";

const term = (id: string, tab: string, space: string, over: Partial<Terminal> = {}): Terminal => ({
  terminal_id: id,
  tab_id: tab,
  space_id: space,
  agent_status: "unknown",
  focused: false,
  survives_restart: true,
  ...over,
});

const leaf = (id: string) => ({ kind: "terminal" as const, terminal_id: id });

export const SNAPSHOT: Snapshot = {
  spaces: [
    {
      space_id: "s1",
      label: "poimen",
      number: 1,
      agent_status: "working",
      cwd: `${HOME}/code/poimen`,
    },
    {
      space_id: "s2",
      label: "billing-api",
      number: 2,
      agent_status: "blocked",
      cwd: `${HOME}/code/billing-api`,
    },
    { space_id: "s3", label: "docs", number: 3, agent_status: "done", cwd: `${HOME}/code/docs` },
    {
      space_id: "s4",
      label: "infra",
      number: 4,
      agent_status: "unknown",
      cwd: `${HOME}/code/infra`,
    },
  ],
  tabs: [
    {
      tab_id: "tab1",
      space_id: "s1",
      label: "claude",
      number: 1,
      agent_status: "working",
      root: {
        kind: "split",
        split_id: "sp1",
        direction: "right",
        ratio: 0.62,
        children: [leaf("t1"), leaf("t2")],
      },
      active_terminal: "t1",
    },
    {
      tab_id: "tab2",
      space_id: "s1",
      label: "tests",
      number: 2,
      agent_status: "unknown",
      root: leaf("t3"),
      active_terminal: "t3",
    },
    {
      tab_id: "tab3",
      space_id: "s2",
      label: "claude",
      number: 1,
      agent_status: "blocked",
      root: leaf("t4"),
      active_terminal: "t4",
    },
    {
      tab_id: "tab4",
      space_id: "s3",
      label: "copilot",
      number: 1,
      agent_status: "done",
      root: leaf("t5"),
      active_terminal: "t5",
    },
    {
      tab_id: "tab5",
      space_id: "s4",
      label: "shell",
      number: 1,
      agent_status: "unknown",
      root: leaf("t6"),
      active_terminal: "t6",
    },
  ],
  terminals: [
    term("t1", "tab1", "s1", {
      agent: "claude",
      hook_agent: "claude",
      agent_session: { value: "demo-session-1" },
      agent_status: "working",
      agent_progress: 60,
      agent_title: "Add the demo backend",
      agent_status_since: ago(12),
      agent_reported_at: ago(1),
      cwd: `${HOME}/code/poimen`,
      focused: true,
      program: "claude",
      label: "claude",
    }),
    term("t2", "tab1", "s1", { cwd: `${HOME}/code/poimen`, program: "zsh", label: "shell" }),
    term("t7", "tab1", "s1", {
      agent: "claude",
      hook_agent: "claude",
      agent_session: { value: "demo-session-2" },
      agent_status: "done",
      started_by: "t1",
      agent_status_since: ago(4),
      cwd: `${HOME}/code/poimen`,
      label: "review",
      program: "claude",
    }),
    term("t3", "tab2", "s1", { cwd: `${HOME}/code/poimen`, program: "pnpm", label: "tests" }),
    term("t4", "tab3", "s2", {
      agent: "claude",
      hook_agent: "claude",
      agent_session: { value: "demo-session-3" },
      agent_status: "blocked",
      agent_kind: "permission",
      agent_message: "Allow `cargo test -p billing`?",
      agent_message_at: ago(3),
      agent_status_since: ago(3),
      cwd: `${HOME}/code/billing-api`,
      program: "claude",
      label: "claude",
    }),
    term("t5", "tab4", "s3", {
      agent: "copilot",
      hook_agent: "copilot",
      agent_session: { value: "demo-session-4" },
      agent_status: "done",
      agent_message: "Rewrote the install guide",
      agent_message_at: ago(20),
      agent_status_since: ago(20),
      cwd: `${HOME}/code/docs`,
      program: "copilot",
      label: "copilot",
    }),
    term("t6", "tab5", "s4", { cwd: `${HOME}/code/infra`, program: "zsh", label: "shell" }),
  ],
};

/** `repo_info` per directory (branch, worktree, dirty, ahead). */
export const REPOS: Record<
  string,
  { branch: string; dirty?: number; ahead?: number; behind?: number; worktree?: boolean }
> = {
  [`${HOME}/code/poimen`]: { branch: "feat/1109-demo-screenshots", dirty: 7, ahead: 3, behind: 0 },
  [`${HOME}/code/billing-api`]: { branch: "fix/412-rounding", dirty: 2, ahead: 1, behind: 0 },
  [`${HOME}/code/docs`]: { branch: "main", dirty: 0, ahead: 0, behind: 0 },
  [`${HOME}/code/infra`]: { branch: "main", dirty: 0, ahead: 0, behind: 2 },
};

/** The text a terminal shows once opened (ANSI), by terminal id. */
export const SCREENS: Record<string, string[]> = {
  t1: [
    "\x1b[1;36m● Claude Code\x1b[0m  \x1b[2mopus · ~/code/poimen\x1b[0m",
    "",
    "\x1b[1m>\x1b[0m Add a demo backend for the screenshots, see #1109",
    "",
    "\x1b[32m●\x1b[0m I'll read \x1b[1msrc/app/core/tauri.ts\x1b[0m first, the only place that talks to Tauri.",
    "",
    "\x1b[32m●\x1b[0m \x1b[1mRead\x1b[0m(src/app/core/tauri.ts)",
    "  \x1b[2m⎿  Read 81 lines\x1b[0m",
    "",
    "\x1b[32m●\x1b[0m \x1b[1mWrite\x1b[0m(src/demo/kit/backend.ts)",
    "  \x1b[2m⎿  Wrote 74 lines\x1b[0m",
    "",
    "\x1b[33m✻\x1b[0m Wiring the answers… \x1b[2m(12s · esc to interrupt)\x1b[0m",
  ],
  t2: [
    "\x1b[1;32m~/code/poimen\x1b[0m \x1b[34mfeat/1109-demo-screenshots\x1b[0m \x1b[33m*\x1b[0m",
    "\x1b[1;35m❯\x1b[0m git status -s",
    " \x1b[31mM\x1b[0m angular.json",
    "\x1b[31m??\x1b[0m src/demo/",
    "\x1b[1;35m❯\x1b[0m ",
  ],
  t3: [
    "\x1b[2m$ pnpm test\x1b[0m",
    " \x1b[32m✓\x1b[0m src/demo/kit/backend.spec.ts \x1b[2m(6 tests)\x1b[0m",
    " \x1b[32m✓\x1b[0m src/app/core/settings.spec.ts \x1b[2m(41 tests)\x1b[0m",
    "",
    " Test Files  \x1b[32m2 passed\x1b[0m (2)",
  ],
  t4: [
    "\x1b[1;36m● Claude Code\x1b[0m  \x1b[2msonnet · ~/code/billing-api\x1b[0m",
    "",
    "\x1b[1m>\x1b[0m Fix the rounding of partial refunds, #412",
    "",
    "\x1b[32m●\x1b[0m \x1b[1mBash\x1b[0m(cargo test -p billing)",
    "",
    "  \x1b[33mDo you want to proceed?\x1b[0m",
    "  \x1b[1;36m❯ 1. Yes\x1b[0m",
    "    2. Yes, and don't ask again for cargo test commands",
    "    3. No",
  ],
  t5: [
    "\x1b[1;35m● GitHub Copilot CLI\x1b[0m",
    "",
    "\x1b[32m✓\x1b[0m Rewrote the install guide (3 files changed)",
    "\x1b[2mDone in 4m 12s\x1b[0m",
  ],
  t6: ["\x1b[1;32m~/code/infra\x1b[0m \x1b[34mmain\x1b[0m", "\x1b[1;35m❯\x1b[0m "],
  t7: [
    "\x1b[1;36m● Claude Code\x1b[0m  \x1b[2mreview agent\x1b[0m",
    "",
    "\x1b[32m✓\x1b[0m No blocking findings.",
  ],
};

export const MACHINES = [
  {
    name: "build-box",
    ssh: "build-box",
    reposDir: "~/src/{owner}/{repo}",
    state: "connected",
    serverVersion: "0.31.0",
  },
  {
    name: "gpu-lab",
    ssh: "ops@gpu-lab.example.net",
    reposDir: "~/src/{owner}/{repo}",
    state: "offline",
    error: "ssh: connect to host gpu-lab.example.net port 22: Connection timed out",
  },
];

/** What `machine_snapshot` answers for the connected machine (its own, unqualified ids). */
export const MACHINE_SNAPSHOT: Snapshot = {
  spaces: [
    {
      space_id: "s1",
      label: "ci-runner",
      number: 1,
      agent_status: "working",
      cwd: "/home/demo/src/acme/ci-runner",
    },
  ],
  tabs: [
    {
      tab_id: "tab1",
      space_id: "s1",
      label: "claude",
      number: 1,
      agent_status: "working",
      root: { kind: "terminal", terminal_id: "t1" },
      active_terminal: "t1",
    },
  ],
  terminals: [
    term("t1", "tab1", "s1", {
      agent: "claude",
      hook_agent: "claude",
      agent_session: { value: "remote-session-1" },
      agent_status: "working",
      agent_status_since: ago(6),
      cwd: "/home/demo/src/acme/ci-runner",
      program: "claude",
      label: "claude",
    }),
  ],
};

export const HARNESSES = [
  {
    id: "claude",
    command: ["claude"],
    models: ["opus", "sonnet", "haiku"],
    defaultModel: null,
    exitInput: "/exit",
    resumeArgs: ["--resume", "{session}"],
    printArgs: ["-p"],
    slashSkills: true,
    available: true,
  },
  {
    id: "copilot",
    command: ["copilot"],
    models: [],
    defaultModel: null,
    exitInput: "/exit",
    resumeArgs: ["--resume={session}"],
    printArgs: ["-p"],
    slashSkills: true,
    available: true,
  },
];

const iso = (minutes: number): string => new Date(ago(minutes)).toISOString();
const issue = (n: number, title: string) => ({
  kind: "github.issue",
  id: `#${n}`,
  title: `#${n} ${title}`,
  url: `https://github.com/acme/poimen/issues/${n}`,
});

/** The run the Runs view shows: a milestone with two delivered issues, one in progress, one waiting for the user. */
export const RUN = {
  run: "20261121T083000-4711",
  status: "running",
  updated: iso(1),
  subject: {
    kind: "github.milestone",
    id: "M031",
    title: "M031 Workbench polish",
    url: "https://github.com/acme/poimen/milestone/31",
  },
  progress: { done: 2, total: 5, label: "issues" },
  tone: "info",
  needsYou: true,
  agent: { harness: "claude", model: "opus" },
  host: { "poimen.terminal": "t1" },
  tasks: [
    {
      task: "#1109",
      parent: null,
      subject: issue(1109, "docs(site): new screenshots from component fixtures"),
      step: { name: "implement", n: 3, total: 5, text: "Writing the demo backend" },
      progress: null,
      links: [],
    },
  ],
  finished: {
    succeeded: [
      {
        task: "#1101",
        subject: issue(1101, "feat(sidebar): space cards"),
        reason: null,
        text: "Merged",
        links: [
          { rel: "pull-request", url: "https://github.com/acme/poimen/pull/1120", title: "#1120" },
        ],
      },
      {
        task: "#1102",
        subject: issue(1102, "feat(tabs): tab strip in the shell"),
        reason: null,
        text: "Merged",
        links: [
          { rel: "pull-request", url: "https://github.com/acme/poimen/pull/1121", title: "#1121" },
        ],
      },
    ],
  },
  attentions: [
    {
      id: "a1",
      task: "#1105",
      reason: "needs-decision",
      text: "#1105 asks which tooltip delay to use",
      tone: "warning",
      needsYou: true,
      links: [],
    },
  ],
  links: [],
  events: [
    { seq: 1, ts: iso(95), type: "run.started", tone: "info", subject: "M031 Workbench polish" },
    {
      seq: 2,
      ts: iso(80),
      type: "task.started",
      tone: "info",
      task: "#1101",
      subject: "#1101 feat(sidebar): space cards",
    },
    {
      seq: 3,
      ts: iso(55),
      type: "task.finished",
      tone: "success",
      task: "#1101",
      outcome: "succeeded",
      text: "Merged",
    },
    {
      seq: 4,
      ts: iso(50),
      type: "task.started",
      tone: "info",
      task: "#1102",
      subject: "#1102 feat(tabs): tab strip in the shell",
    },
    {
      seq: 5,
      ts: iso(30),
      type: "task.finished",
      tone: "success",
      task: "#1102",
      outcome: "succeeded",
      text: "Merged",
    },
    {
      seq: 6,
      ts: iso(28),
      type: "attention.raised",
      tone: "warning",
      task: "#1105",
      text: "#1105 asks which tooltip delay to use",
    },
    {
      seq: 7,
      ts: iso(26),
      type: "task.started",
      tone: "info",
      task: "#1109",
      subject: "#1109 docs(site): new screenshots from component fixtures",
    },
    { seq: 8, ts: iso(12), type: "task.step", tone: "neutral", task: "#1109", step: "implement" },
  ],
};

export const MACHINE_CHECK = {
  version: "0.31.0",
  protocol: 1,
  checks: [
    { id: "ssh", status: "ok", detail: "connected as demo", fix: null },
    { id: "poimen-server", status: "ok", detail: "0.31.0", fix: null },
    { id: "git", status: "ok", detail: "2.47.1", fix: null },
    { id: "gh", status: "warning", detail: "not signed in", fix: "gh auth login" },
    { id: "claude", status: "ok", detail: "2.1.0", fix: null },
  ],
};

const gh = (n: number, title: string, labels: string[], over: Record<string, unknown> = {}) => ({
  number: n,
  title,
  url: `https://github.com/acme/poimen/issues/${n}`,
  state: "OPEN",
  labels,
  assignees: [],
  milestone: "M031 Workbench polish",
  parent: null,
  subTotal: 0,
  subCompleted: 0,
  prs: [],
  ...over,
});

export const MILESTONES = [
  {
    number: 31,
    title: "M031 Workbench polish",
    url: "https://github.com/acme/poimen/milestone/31",
    dueOn: "2026-11-30T00:00:00Z",
    description: "Sidebar, tabs and tooltips",
    open: 3,
    closed: 2,
    current: true,
  },
];

export const OPEN_ISSUES = [
  gh(1105, "[PBI] feat(ui): tooltip delay", ["pbi"]),
  gh(1109, "[PBI] docs(site): new screenshots from component fixtures", ["pbi"], {
    assignees: ["demo"],
  }),
  gh(1110, "[Bug] fix(sidebar): card height jumps while an agent works", ["bug"]),
];

export const CLOSED_ISSUES = [
  gh(1101, "[PBI] feat(sidebar): space cards", ["pbi"], { state: "CLOSED" }),
  gh(1102, "[PBI] feat(tabs): tab strip in the shell", ["pbi"], { state: "CLOSED" }),
];

/** The skill the palette offers in the poimen repository. */
export const DETECTED = [{ source: "skills", name: "deliver", description: "Ship an issue" }];

/** The Changes tab of the poimen repository: the changed files and the content of each. */
export const CHANGES = {
  base: "main",
  scope: "base",
  from: "1a2b3c4",
  files: [
    ["angular.json", "modified", 12, 1],
    ["src/demo/kit/backend.ts", "added", 74, 0],
    ["src/demo/tauri.demo.ts", "added", 18, 0],
  ].map(([path, status, additions, deletions]) => ({
    path,
    old_path: null,
    status,
    additions,
    deletions,
    binary: false,
    staged: false,
    conflicted: false,
  })),
};

export const CHANGED_FILE = {
  state: "text",
  old: '{\n  "serve": {\n    "configurations": {}\n  }\n}\n',
  new: '{\n  "serve": {\n    "configurations": {\n      "demo": { "port": 1422 }\n    }\n  }\n}\n',
};

/** The main agent's calls of terminal `t1`, by tool use id: the skills and the subagent it started. */
export const MAIN_CALLS = [
  {
    tool_use_id: "c1",
    name: "Skill",
    target: "github-issue",
    detail: "1109",
    started: new Date(ago(11)).toISOString(),
    ended: new Date(ago(10)).toISOString(),
    is_error: false,
    context: 18_000,
  },
  {
    tool_use_id: "c2",
    name: "Skill",
    target: "implementation",
    detail: "",
    started: new Date(ago(9)).toISOString(),
    ended: null,
    is_error: false,
    context: 42_000,
  },
];

export const TOKENS: Record<
  string,
  { input: number; output: number; cache_read: number; cache_creation: number }
> = {
  t1: { input: 84_200, output: 12_900, cache_read: 1_204_000, cache_creation: 96_400 },
  t7: { input: 21_300, output: 3_100, cache_read: 310_000, cache_creation: 28_800 },
};
