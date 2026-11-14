import { describe, expect, it } from "vitest";
import {
  startableKinds,
  statusNote,
  agentName,
  agentSupport,
  hasTranscripts,
  appVersionLabel,
  backendVersionLabel,
  decodeBase64,
  protocolWarning,
  windowTitle,
} from "./backend";

describe("decodeBase64", () => {
  it("decodes ANSI bytes including non-ASCII", () => {
    // "\x1b[2J" followed by "λ" (UTF-8 0xCE 0xBB)
    expect(Array.from(decodeBase64("G1sySs67"))).toEqual([0x1b, 0x5b, 0x32, 0x4a, 0xce, 0xbb]);
  });
});

describe("windowTitle", () => {
  it("names the RC build and the waiting agents", () => {
    expect(windowTitle("stable", 0)).toBe("poimen");
    expect(windowTitle("stable", 2)).toBe("poimen (2 waiting)");
    expect(windowTitle("rc", 0)).toBe("poimen RC");
    expect(windowTitle("rc", 1)).toBe("poimen RC (1 waiting)");
  });
});

describe("version labels", () => {
  const info = { version: "0.9.5", protocol: 22, supported: 22 };
  it("names the versions", () => {
    expect(backendVersionLabel(info)).toBe("backend herdr 0.9.5");
    expect(appVersionLabel("0.2.0")).toBe("frontend poimen 0.2.0");
  });
  it("shows ? when unknown", () => {
    expect(backendVersionLabel({ version: "0.1", supported: 22 })).toBe("backend herdr 0.1");
    expect(backendVersionLabel(null)).toBe("backend herdr ?");
    expect(backendVersionLabel(undefined)).toBe("backend herdr ?");
    expect(backendVersionLabel({ ...info, version: "0.9.0" }, "poimen")).toBe(
      "backend poimen 0.9.0",
    );
    expect(backendVersionLabel(null, "poimen")).toBe("backend poimen ?");
    expect(protocolWarning({ ...info, protocol: 1, supported: 2 }, "poimen")).toBe(
      "poimen protocol 1, poimen supports 2",
    );
    expect(appVersionLabel(null)).toBe("frontend poimen ?");
  });
  it("warns only on a protocol mismatch", () => {
    expect(protocolWarning(info)).toBe("");
    expect(protocolWarning(null)).toBe("");
    expect(protocolWarning({ version: "0.1", supported: 22 })).toBe("");
    expect(protocolWarning({ ...info, protocol: 23 })).toBe(
      "herdr protocol 23, poimen supports 22",
    );
  });
});

describe("startableKinds", () => {
  it("offers only the kinds that report through hooks on the poimen backend", () => {
    expect(startableKinds("poimen")).toEqual(["claude", "copilot", "gemini"]);
  });

  it("adds the other harnesses of the registry on the poimen backend, once", () => {
    expect(startableKinds("poimen", ["claude", "codex", "aider"])).toEqual([
      "claude",
      "copilot",
      "gemini",
      "codex",
      "aider",
    ]);
  });

  it("labels a kind without hooks as having no status", () => {
    expect(statusNote("codex", "poimen")).toBe(" (no status)");
    expect(statusNote("claude", "poimen")).toBe("");
    expect(statusNote("codex", "herdr")).toBe("");
  });

  it("keeps herdr's full list on the herdr backend", () => {
    const kinds = startableKinds("herdr");
    expect(kinds).toHaveLength(24);
    expect(kinds.slice(0, 5)).toEqual(["claude", "codex", "gemini", "opencode", "copilot"]);
    expect(kinds).toContain("muse");
  });
});

describe("hook agent kinds", () => {
  it("names the kinds and what each one gives", () => {
    expect(agentName("gemini")).toBe("Gemini CLI");
    expect(agentName("codex")).toBe("codex");
    expect(agentSupport("claude")).toBe("status, transcript");
    expect(agentSupport("gemini")).toBe("status");
    expect(agentSupport("codex")).toBe("no status");
    expect(hasTranscripts("copilot")).toBe(true);
    expect(hasTranscripts("gemini")).toBe(false);
    expect(hasTranscripts("codex")).toBe(false);
  });
});
