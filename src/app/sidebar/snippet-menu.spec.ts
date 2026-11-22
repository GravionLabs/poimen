import { describe, expect, it } from "vitest";
import type { Terminal } from "../core/snapshot";
import type { Snippet } from "../core/snippets-model";
import { snippetMenuGroup, snippetOf } from "./snippet-menu";

const tests: Snippet = { label: "tests", text: "cargo test", enter: true, target: "terminal" };
const deliver: Snippet = { label: "deliver", text: "/deliver", enter: true, target: "agent" };
const shell = { terminal_id: "t1" } as Terminal;
const agent = { terminal_id: "t2", agent: "claude" } as Terminal;

describe("snippet menu", () => {
  it("offers an agent snippet for an agent pane only", () => {
    expect(snippetMenuGroup([tests, deliver], agent)?.items.map((i) => i.label)).toEqual([
      "tests",
      "deliver",
    ]);
    expect(snippetMenuGroup([tests, deliver], shell)?.items.map((i) => i.label)).toEqual(["tests"]);
    expect(snippetMenuGroup([deliver], shell)).toBeNull();
    expect(snippetMenuGroup([], agent)).toBeNull();
  });

  it("maps a picked entry back to its snippet", () => {
    expect(snippetOf("snippet:tests", [tests, deliver])).toBe(tests);
    expect(snippetOf("snippet:gone", [tests])).toBeNull();
    expect(snippetOf("rename", [tests])).toBeNull();
  });
});
