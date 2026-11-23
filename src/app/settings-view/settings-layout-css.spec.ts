import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

const css = (path: string): string => readFileSync(path, "utf8");

/**
 * `settings-view.css` is shared by the settings view and its child pages (layouts, templates, import
 * panel). A bare `:host` there made every child a flex row with `overflow: hidden`: text clipped
 * and fields squeezed. The shell rule must only match the view that sets `settings-root`.
 */
describe("settings page styles", () => {
  it("scopes the flex shell to the settings view itself", () => {
    const text = css("src/app/settings-view/settings-view.css");
    expect(text).not.toMatch(/(^|\n):host\s*\{/);
    expect(text).toMatch(/:host\(\.settings-root\)\s*\{[^}]*display:\s*flex/);
  });

  it("caps the page width with one shared rule, wider for opted-in pages", () => {
    const text = css("src/app/settings-view/settings-view.css");
    const page = /\n\.page\s*\{([^}]*)\}/.exec(text)?.[1] ?? "";
    expect(page).toMatch(/max-width:\s*800px/);
    const wide = /\n\.page\.wide\s*\{([^}]*)\}/.exec(text)?.[1] ?? "";
    expect(wide).toMatch(/max-width:\s*1200px/);
  });
});

/** A tab is as wide as its content; `--tab-width` is only the maximum (no empty space after the close button). */
describe("tab width", () => {
  it("sizes a tab by its content up to the shared maximum", () => {
    const text = css("src/app/ui/helix-candidates/tab-bar/tab-bar.css");
    const tab = /\n\.tab\s*\{([^}]*)\}/.exec(text)?.[1] ?? "";
    expect(tab).toMatch(/width:\s*auto/);
    expect(tab).toMatch(/max-width:\s*var\(--tab-width\)/);
    expect(tab).not.toMatch(/(^|[^-])width:\s*var\(--tab-width\)/);
  });
});
