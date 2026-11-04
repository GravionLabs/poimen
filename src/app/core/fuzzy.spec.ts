import { describe, expect, it } from "vitest";
import { fuzzyFilter, fuzzyMatch, fuzzyRank, fuzzyScore } from "./fuzzy";

describe("fuzzyScore", () => {
  it("matches characters in order, ignoring case and spaces", () => {
    expect(fuzzyScore("ntm", "New Terminal")).not.toBeNull();
    expect(fuzzyScore("NEW t", "new terminal")).not.toBeNull();
  });

  it("rejects text that lacks a character or has it in the wrong order", () => {
    expect(fuzzyScore("xyz", "New Terminal")).toBeNull();
    expect(fuzzyScore("mt", "Terminal")).toBeNull();
  });

  it("scores an empty query 0", () => {
    expect(fuzzyScore("", "anything")).toBe(0);
    expect(fuzzyScore("  ", "anything")).toBe(0);
  });

  it("prefers consecutive and word-start matches", () => {
    expect(fuzzyScore("term", "Terminal")!).toBeGreaterThan(
      fuzzyScore("term", "Toggle Edit Remote Mode")!,
    );
    expect(fuzzyScore("ns", "New Sidebar")!).toBeGreaterThan(fuzzyScore("ns", "Panes")!);
  });
});

describe("fuzzyMatch", () => {
  it("returns the matched character positions", () => {
    expect(fuzzyMatch("ntm", "New Terminal")?.positions).toEqual([0, 4, 7]);
    expect(fuzzyMatch("TERM", "Terminal")?.positions).toEqual([0, 1, 2, 3]);
  });

  it("has no positions for an empty query and null for no match", () => {
    expect(fuzzyMatch("", "x")).toEqual({ score: 0, positions: [] });
    expect(fuzzyMatch("zz", "New Terminal")).toBeNull();
  });
});

describe("fuzzyRank", () => {
  it("ranks like fuzzyFilter and carries the positions", () => {
    const ranked = fuzzyRank(["Toggle Sidebar", "Open Settings"], "set", (s) => s);
    expect(ranked[0]).toEqual({ item: "Open Settings", positions: [5, 6, 7] });
  });
});

describe("fuzzyFilter", () => {
  const items = ["Close Tab", "Toggle Sidebar", "Open Settings", "New Terminal"];

  it("keeps the input order for an empty query", () => {
    expect(fuzzyFilter(items, "", (s) => s)).toEqual(items);
  });

  it("drops non-matches and ranks the best match first", () => {
    expect(fuzzyFilter(items, "set", (s) => s)[0]).toBe("Open Settings");
    expect(fuzzyFilter(items, "qq", (s) => s)).toEqual([]);
  });
});
