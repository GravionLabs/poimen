import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { defineConfig, type Plugin } from "vitest/config";

/**
 * Specs compile components with JIT and without the Angular resource transform, so a
 * `templateUrl` is never loaded. This plugin inlines the file as `template` when Vitest loads a
 * component source (not a spec), as the Angular build does at compile time. `styleUrl` stays
 * as it is.
 */
function inlineTemplateUrls(): Plugin {
  const templateUrl = /\btemplateUrl:\s*(["'])(\.{1,2}\/[^"']+\.html)\1/g;
  return {
    name: "poimen:inline-template-urls",
    enforce: "pre",
    transform(code, id) {
      const file = id.split("?")[0];
      const component = file.endsWith(".ts") && !file.endsWith(".spec.ts");
      if (!component || file.includes("/node_modules/") || !code.includes("templateUrl"))
        return null;
      let changed = false;
      const out = code.replace(templateUrl, (_match, _quote, path: string) => {
        const html = resolve(dirname(file), path);
        this.addWatchFile(html);
        changed = true;
        return `template: ${JSON.stringify(readFileSync(html, "utf8"))}`;
      });
      return changed ? { code: out, map: null } : null;
    },
  };
}

export default defineConfig({
  plugins: [inlineTemplateUrls()],
  test: {
    include: ["src/**/*.spec.ts"],
    environment: "node",
    setupFiles: ["./vitest.setup.ts"],
  },
});
