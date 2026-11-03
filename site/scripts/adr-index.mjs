/** `# 0002: Color schemes` -> `{ number: "0002", title: "Color schemes" }`; the file name is the fallback. */
export function adrHeading(file, text) {
  const found = /^#\s+(\d{4}):\s*(.+?)\s*$/m.exec(text);
  if (found) return { number: found[1], title: found[2] };
  const number = file.slice(0, 4);
  return { number, title: file.slice(5, -3).replace(/-/g, " ") };
}

/** `- Status: Accepted (decided ...)` -> `Accepted`; empty when the record has no status line. */
export function adrStatus(text) {
  const found = /^-\s+Status:\s*([A-Za-z]+)/m.exec(text);
  return found ? found[1] : "";
}

/** The index page of the decision records: a table in the order of the numbers. */
export function adrIndex(records) {
  const rows = records.map(({ file, text }) => {
    const { number, title } = adrHeading(file, text);
    return `| [${number}](./${file.slice(0, -3)}) | ${title} | ${adrStatus(text) || "-"} |`;
  });
  return [
    "# Decisions",
    "",
    "Architecture decision records of poimen. They are kept in the repository (`docs/adr/`) and published here unchanged.",
    "",
    "| ADR | Decision | Status |",
    "| --- | --- | --- |",
    ...rows,
    "",
  ].join("\n");
}
