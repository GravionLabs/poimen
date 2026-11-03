// Copies the architecture decision records of docs/adr into the site (site/adr, not committed), so the
// records stay the single source and are part of the site. Also writes the index page of the section.
import { mkdirSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { adrIndex } from "./adr-index.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const source = join(here, "..", "..", "docs", "adr");
const target = join(here, "..", "adr");

rmSync(target, { recursive: true, force: true });
mkdirSync(target, { recursive: true });

const records = [];
for (const name of readdirSync(source).filter((f) => /^\d{4}-.+\.md$/.test(f)).sort()) {
  const text = readFileSync(join(source, name), "utf8");
  writeFileSync(join(target, name), text);
  records.push({ file: name, text });
}
writeFileSync(join(target, "index.md"), adrIndex(records));
console.log(`site/adr: ${records.length} decision records`);
