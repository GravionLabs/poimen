import { describe, expect, it } from "vitest";
import {
  emptyTemplate,
  isValidDir,
  matchLabel,
  parseSpaceTemplates,
  templateConfig,
  templateErrors,
  type SpaceTemplate,
} from "./space-templates";

const base: SpaceTemplate = {
  name: "Work",
  dir: "~/src",
  remote: null,
  agent: "claude",
  model: null,
  layout: null,
};

describe("space templates", () => {
  it("parses the wire shape and skips malformed entries", () => {
    expect(
      parseSpaceTemplates([
        { name: "a", dir: "/x", remote: null, agent: "claude", model: "opus", layout: null },
        { name: "b", remote: { provider: "github", owner: "o" }, agent: "copilot" },
        { name: 1, agent: "x" },
        "nope",
      ]),
    ).toEqual([
      { name: "a", dir: "/x", remote: null, agent: "claude", model: "opus", layout: null },
      {
        name: "b",
        dir: null,
        remote: { provider: "github", owner: "o" },
        agent: "copilot",
        model: null,
        layout: null,
      },
    ]);
    expect(parseSpaceTemplates(undefined)).toEqual([]);
  });

  it("writes trimmed values and leaves out empty optional fields", () => {
    expect(templateConfig({ ...base, name: " Work ", dir: " ~/src ", model: " " })).toEqual({
      name: "Work",
      match: { dir: "~/src" },
      agent: "claude",
    });
    expect(
      templateConfig({
        ...base,
        dir: null,
        remote: { provider: "azure-devops", organization: " o ", project: "p" },
        layout: { panes: [{ name: "main", command: "ls", split: null }] },
      }),
    ).toEqual({
      name: "Work",
      match: { remote: { provider: "azure-devops", organization: "o", project: "p" } },
      agent: "claude",
      layout: { panes: [{ name: "main", command: "ls" }] },
    });
  });

  it("accepts absolute and home directories only", () => {
    for (const ok of ["~", "~/a", "/a", "C:\\a", "c:/a", "\\\\srv\\share"])
      expect(isValidDir(ok), ok).toBe(true);
    for (const bad of ["a/b", "~user", "C:", ""]) expect(isValidDir(bad), bad).toBe(false);
  });

  it("mirrors the backend validation", () => {
    const ids = ["claude", "copilot"];
    expect(templateErrors(base, [], ids)).toEqual({});
    expect(templateErrors({ ...base, name: " " }, [], ids).name).toContain("required");
    expect(templateErrors(base, ["Work"], ids).name).toContain("Another template");
    expect(templateErrors({ ...base, dir: null }, [], ids).match).toBeDefined();
    expect(templateErrors({ ...base, dir: "rel" }, [], ids).dir).toBeDefined();
    expect(templateErrors({ ...base, agent: "x" }, [], ids).agent).toContain("Unknown harness");
    expect(templateErrors({ ...base, agent: "" }, [], ids).agent).toBeDefined();
    expect(
      templateErrors({ ...base, remote: { provider: "github", owner: " " } }, [], ids).remote,
    ).toContain("owner");
    expect(
      templateErrors(
        { ...base, layout: { panes: [{ name: "", command: null, split: null }] } },
        [],
        ids,
      ).layout,
    ).toBeDefined();
    expect(templateErrors({ ...base, layout: { panes: [] } }, [], ids).layout).toBeDefined();
  });

  it("describes the match and starts a new draft empty", () => {
    expect(matchLabel(base)).toBe("~/src");
    expect(matchLabel({ ...base, remote: { provider: "github", owner: "acme" } })).toBe(
      "~/src + github.com/acme",
    );
    expect(emptyTemplate("claude")).toMatchObject({ name: "", dir: null, agent: "claude" });
  });
});
