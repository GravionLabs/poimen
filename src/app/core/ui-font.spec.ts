// @vitest-environment jsdom
import { describe, expect, it } from "vitest";
import { applyUiFont, sizeError } from "./ui-font";

describe("applyUiFont", () => {
  it("sets --font-ui and the root size variable", () => {
    const root = document.createElement("div");
    applyUiFont(root, "Inter, sans-serif", 15);
    expect(root.style.getPropertyValue("--font-ui")).toBe("Inter, sans-serif");
    expect(root.style.getPropertyValue("--font-size-ui")).toBe("15px");
  });

  it("removes --font-ui for the default family", () => {
    const root = document.createElement("div");
    applyUiFont(root, "Inter", 13);
    applyUiFont(root, null, 13);
    expect(root.style.getPropertyValue("--font-ui")).toBe("");
    expect(root.style.getPropertyValue("--font-size-ui")).toBe("13px");
  });
});

describe("sizeError", () => {
  it("accepts the bounds and rejects outside and non-numbers", () => {
    expect(sizeError("X", 10, 10, 20)).toBeNull();
    expect(sizeError("X", 20, 10, 20)).toBeNull();
    expect(sizeError("X", 21, 10, 20)).toBe("X must be a number from 10 to 20");
    expect(sizeError("X", Number.NaN, 10, 20)).not.toBeNull();
  });
});
