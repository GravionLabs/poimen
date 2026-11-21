import { describe, expect, it } from "vitest";

import { parseAgentActions } from "../core/settings";

describe("parseAgentActions", () => {
  it("reads actions and skips malformed entries", () => {
    expect(
      parseAgentActions({
        actions: [
          {
            id: "a",
            label: "A",
            on: "pr",
            prompt: "p",
            run: "ask",
            harness: "x",
            model: "opus",
            when: { label: ["bug"], noChildren: true, checks: "failed" },
            source: "project",
          },
          { id: "b", on: "issue", prompt: "q", source: "user" },
          { id: "c", on: "repo", prompt: "q", source: "user" },
          { id: "d", on: "issue", prompt: "q", source: "other" },
          null,
        ],
      }),
    ).toEqual([
      {
        id: "a",
        label: "A",
        on: "pr",
        prompt: "p",
        run: "ask",
        harness: "x",
        model: "opus",
        when: {
          level: [],
          label: ["bug"],
          noChildren: true,
          checks: "failed",
          conflicting: false,
          state: [],
          review: null,
        },
        batch: "each",
        source: "project",
      },
      {
        id: "b",
        label: "b",
        on: "issue",
        prompt: "q",
        run: "repoRoot",
        harness: null,
        model: null,
        when: {
          level: [],
          label: [],
          noChildren: false,
          checks: null,
          conflicting: false,
          state: [],
          review: null,
        },
        batch: "each",
        source: "user",
      },
    ]);
    expect(parseAgentActions("nope")).toEqual([]);
  });
});
