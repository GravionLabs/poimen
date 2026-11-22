import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { agentMonogram, derivedMonogram, HARNESS_MONOGRAMS, HOOK_AGENTS } from "./backend";
import { paneKindIcon, paneKindLabel } from "./pane-kind";
import type { Terminal } from "./snapshot";

const pane = (over: Partial<Terminal>): Terminal => ({ terminal_id: "t1", ...over }) as Terminal;

describe("pane kind icon (#1215)", () => {
  it("is the terminal icon for a shell", () => {
    expect(paneKindIcon(pane({}))).toBe("terminal");
    expect(paneKindIcon(undefined)).toBe("terminal");
  });

  it("is a distinct monogram badge per bundled kind", () => {
    const kinds = [...HOOK_AGENTS.map((a) => a.kind), ...Object.keys(HARNESS_MONOGRAMS)];
    const icons = kinds.map((agent) => paneKindIcon(pane({ agent })));
    expect(icons.every((i) => i.startsWith("agent:"))).toBe(true);
    expect(new Set(icons).size).toBe(kinds.length);
    expect(icons).toContain("agent:Cc");
  });

  it("reads agent_kind when `agent` is not set", () => {
    expect(paneKindIcon(pane({ agent_kind: "claude" }))).toBe("agent:Cc");
  });

  it("is the generic agent icon for an unknown kind", () => {
    expect(paneKindIcon(pane({ agent: "mystery" }))).toBe("agent");
  });

  it("derives a monogram from the display name for a harness without one", () => {
    expect(agentMonogram("aider", ["aider"])).toBe("Ai");
    expect(agentMonogram("aider")).toBeUndefined();
    expect(derivedMonogram("Kilo Code")).toBe("KC");
    expect(derivedMonogram("---")).toBe("?");
  });

  it("names the kind and the mode in the tooltip", () => {
    expect(paneKindLabel(pane({ agent: "claude" }), false)).toBe("Claude Code");
    expect(paneKindLabel(pane({ agent: "claude" }), true)).toBe("Claude Code · Read-only");
    expect(paneKindLabel(pane({}), true)).toBe("Terminal · Read-only");
  });

  it("shows the lock only on a read-only tab, next to the kind icon", () => {
    const html = readFileSync("src/app/app.html", "utf8");
    expect(html).toContain('@if (editor.mode(tab.id) !== "control")');
    expect(html).toContain('name="lock"');
    expect(html).toContain('[label]="paneKindLabel(p, editor.mode(tab.id)');
  });
});
