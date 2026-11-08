import { describe, expect, it } from "vitest";
import { formOf, patchOf } from "./config";

describe("formOf", () => {
  it("reads the known keys and ignores the rest", () => {
    const { form, error } = formOf(
      JSON.stringify({ setup: "a", layout: { panes: [{ name: "x" }] }, other: 1 }),
    );
    expect(error).toBeNull();
    expect(form.setup).toBe("a");
    expect(form.teardown).toBe("");
    expect(JSON.parse(form.layout)).toEqual({ panes: [{ name: "x" }] });
  });

  it("gives an empty form for no text and an error for broken text", () => {
    expect(formOf("")).toEqual({ form: { setup: "", teardown: "", layout: "" }, error: null });
    expect(formOf("{ x").error).toContain("not valid JSON");
    expect(formOf("[1]").error).toContain("not a JSON object");
  });
});

describe("patchOf", () => {
  it("round-trips a form to a patch", () => {
    const { form } = formOf(JSON.stringify({ setup: "s", layout: { panes: [{ name: "x" }] } }));
    expect(patchOf(form)).toEqual({
      patch: { setup: "s", teardown: "", layout: { panes: [{ name: "x" }] } },
      errors: [],
    });
  });

  it("removes a blank layout and reports one that is not JSON", () => {
    expect(patchOf({ setup: "", teardown: "", layout: " " }).patch["layout"]).toBeNull();
    expect(patchOf({ setup: "", teardown: "", layout: "{" }).errors[0].field).toBe("layout");
  });
});
