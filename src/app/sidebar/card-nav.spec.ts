import { describe, expect, it } from "vitest";
import { cardItems, cardNav, tabStop, type CardNode } from "./card-nav";

const cards: CardNode[] = [
  {
    key: "a",
    expanded: true,
    hasBody: true,
    rows: [
      { key: "a1", depth: 0 },
      { key: "a2", depth: 1 },
    ],
  },
  { key: "b", expanded: false, hasBody: true, rows: [{ key: "b1", depth: 0 }] },
  { key: "c", expanded: true, hasBody: true, rows: [] },
];
const items = cardItems(cards);

describe("cardItems", () => {
  it("lists headers and the rows of expanded cards in display order, with levels", () => {
    expect(items.map((i) => i.key)).toEqual(["a", "a1", "a2", "b", "c"]);
    expect(
      items.filter((i) => i.kind === "row").map((i) => (i as { level: number }).level),
    ).toEqual([2, 3]);
  });
});

describe("tabStop", () => {
  it("is the cursor when visible, else the first item", () => {
    expect(tabStop(items, "a2")).toBe("a2");
    expect(tabStop(items, "b1")).toBe("a");
    expect(tabStop(items, null)).toBe("a");
    expect(tabStop([], null)).toBeNull();
  });
});

describe("cardNav", () => {
  it("moves down and up over headers and rows, clamped", () => {
    expect(cardNav(items, "a", "ArrowDown")).toEqual({ action: "focus", key: "a1" });
    expect(cardNav(items, "a2", "ArrowDown")).toEqual({ action: "focus", key: "b" });
    expect(cardNav(items, "c", "ArrowDown")).toEqual({ action: "focus", key: "c" });
    expect(cardNav(items, "b", "ArrowUp")).toEqual({ action: "focus", key: "a2" });
    expect(cardNav(items, "a", "ArrowUp")).toEqual({ action: "focus", key: "a" });
  });

  it("jumps with Home and End", () => {
    expect(cardNav(items, "b", "Home")).toEqual({ action: "focus", key: "a" });
    expect(cardNav(items, "a", "End")).toEqual({ action: "focus", key: "c" });
  });

  it("Right expands a collapsed card, else moves to the first row", () => {
    expect(cardNav(items, "b", "ArrowRight")).toEqual({
      action: "toggle",
      key: "b",
      expanded: true,
    });
    expect(cardNav(items, "a", "ArrowRight")).toEqual({ action: "focus", key: "a1" });
    expect(cardNav(items, "c", "ArrowRight")).toBeNull();
    expect(cardNav(items, "a1", "ArrowRight")).toBeNull();
  });

  it("Left collapses an expanded card, or moves from a row to its header", () => {
    expect(cardNav(items, "a", "ArrowLeft")).toEqual({
      action: "toggle",
      key: "a",
      expanded: false,
    });
    expect(cardNav(items, "b", "ArrowLeft")).toBeNull();
    expect(cardNav(items, "a2", "ArrowLeft")).toEqual({ action: "focus", key: "a" });
  });

  it("Enter opens a row and toggles a header", () => {
    expect(cardNav(items, "a1", "Enter")).toEqual({ action: "open", key: "a1" });
    expect(cardNav(items, "a", "Enter")).toEqual({ action: "toggle", key: "a", expanded: false });
    expect(cardNav(items, "b", "Enter")).toEqual({ action: "toggle", key: "b", expanded: true });
  });

  it("ignores other keys and an empty list", () => {
    expect(cardNav(items, "a", "x")).toBeNull();
    expect(cardNav([], null, "ArrowDown")).toBeNull();
  });

  it("starts at the ends without a cursor", () => {
    expect(cardNav(items, null, "ArrowDown")).toEqual({ action: "focus", key: "a" });
    expect(cardNav(items, null, "ArrowUp")).toEqual({ action: "focus", key: "c" });
  });
});
