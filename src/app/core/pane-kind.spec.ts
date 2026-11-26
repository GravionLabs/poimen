import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { agentMonogram, derivedMonogram, HARNESS_MONOGRAMS, HOOK_AGENTS } from "./backend";
import { paneKindIcon, paneKindLabel } from "./pane-kind";
import type { Terminal } from "./snapshot";

const pane = (over: Partial<Terminal>): Terminal => ({ terminal_id: "t1", ...over }) as Terminal;
const hooks = (kind: string): Terminal => pane({ agent: kind, hook_agent: kind });

describe("pane kind icon (#1215)", () => {
  it("is the shell icon for a shell", () => {
    expect(paneKindIcon(pane({}))).toBe("terminal");
    expect(paneKindIcon(undefined)).toBe("terminal");
  });

  it("is a monogram badge in capitals per bundled kind", () => {
    const kinds = [...HOOK_AGENTS.map((a) => a.kind), "codex", "opencode"];
    expect(kinds.map((k) => paneKindIcon(hooks(k)))).toEqual([
      "agent:CC",
      "agent:CP",
      "agent:GM",
      "agent:CX",
      "agent:OC",
    ]);
    expect(Object.keys(HARNESS_MONOGRAMS)).toEqual(["codex", "opencode"]);
  });

  it("is the shell icon for a pane with only a program status (#1314)", () => {
    const shell = pane({
      agent: "deliver",
      agent_status: "blocked",
      agent_kind: "question",
    });
    expect(paneKindIcon(shell)).toBe("terminal");
    expect(paneKindLabel(shell, false)).toBe("Terminal");
    expect(paneKindLabel({ ...shell, agent_title: "Deliver M016" }, true)).toBe(
      "Deliver M016 · Read-only",
    );
  });

  it("keeps the hook agent's badge while a program status is active (#1314)", () => {
    const claude = pane({
      agent: "deliver",
      hook_agent: "claude",
      agent_status: "working",
      agent_title: "Deliver M016",
    });
    expect(paneKindIcon(claude)).toBe("agent:CC");
    expect(paneKindLabel(claude, false)).toBe("Claude Code");
  });

  it("derives the badge for a harness without a monogram, generic icon when nothing is usable", () => {
    expect(paneKindIcon(hooks("aider"))).toBe("agent:AI");
    expect(paneKindIcon(hooks("kilo-code"))).toBe("agent:KC");
    expect(paneKindIcon(hooks("---"))).toBe("agent");
  });

  it("derives a monogram from the display name for a harness without one", () => {
    expect(agentMonogram("aider", ["aider"])).toBe("AI");
    expect(agentMonogram("aider")).toBeUndefined();
    expect(derivedMonogram("Aider")).toBe("AI");
    expect(derivedMonogram("Kilo Code")).toBe("KC");
    expect(derivedMonogram("---")).toBe("?");
  });

  it("names the kind and the mode in the tooltip", () => {
    expect(paneKindLabel(hooks("claude"), false)).toBe("Claude Code");
    expect(paneKindLabel(hooks("claude"), true)).toBe("Claude Code · Read-only");
    expect(paneKindLabel(pane({}), true)).toBe("Terminal · Read-only");
  });

  it("shows the lock only on a read-only tab, next to the kind icon", () => {
    const html = readFileSync("src/app/app.html", "utf8");
    expect(html).toContain('@if (editor.mode(tab.id) !== "control")');
    expect(html).toContain('name="lock"');
    expect(html).toContain('[label]="paneKindLabel(p, editor.mode(tab.id)');
  });
});
