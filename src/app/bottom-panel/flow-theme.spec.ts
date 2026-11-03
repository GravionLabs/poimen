import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

const FLOW_THEME = "node_modules/@foblex/flow/styles/default.scss";

describe("flow theme", () => {
  // Without the @foblex/flow theme the connection paths have no stroke: the flow shows nodes only.
  it("is part of the global build styles, before the app styles", () => {
    const config = JSON.parse(readFileSync("angular.json", "utf8")) as {
      projects: Record<string, { architect: { build: { options: { styles: string[] } } } }>;
    };
    for (const project of Object.values(config.projects)) {
      const styles = project.architect.build.options.styles;
      expect(styles).toContain(FLOW_THEME);
      expect(styles.indexOf(FLOW_THEME)).toBeLessThan(styles.indexOf("src/styles.css"));
    }
  });
});
