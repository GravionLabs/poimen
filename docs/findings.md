# Review findings

The Changes tab shows the findings of a review (an agent review or a skill such as `code-review`)
as read-only comments at their lines in the diff. A finding can be sent to the agent like an own
comment.

## File

`<worktree>/.agents/state/findings.json`, schema: [`findings.schema.json`](findings.schema.json).
poimen reads it from the worktree of the tab each time the changes are read (open, refresh button,
an agent finishing); it never writes it. The file must be a regular file below `.agents/state/`
(symlinks are not followed) and at most 1 MiB.

```json
{
  "version": 1,
  "sha": "0123456789abcdef0123456789abcdef01234567",
  "source": "deliver",
  "findings": [
    {
      "file": "src/app/a.ts",
      "line": 12,
      "endLine": 14,
      "severity": "blocking",
      "title": "Unchecked input",
      "why": "The value is used before it is validated."
    }
  ]
}
```

Top level: `version` (`1`), `sha` (the reviewed commit, full or abbreviated, 7 to 64 hexadecimal
digits), `source` (free text, who made the review, for example `deliver`) and `findings`.

A finding:

- `file`: path relative to the repository root. Optional.
- `line`: first line (1-based, new side of the diff). Optional, needs `file`.
- `endLine`: last line of a range, not before `line`. Optional, needs `line`.
- `severity`: `blocking` or `note`.
- `title`: short title. `why`: the explanation.
- `rule`: optional, `adr-missing` or `spec-drift` (as in the `code-review` schema).

Unknown fields make the file invalid.

## Relation to `review.schema.json` (code-review skill)

The format is a superset of the review result
`{ blocking: [{file, line, title, why, rule?}], notes: [string] }`: each `blocking` entry maps to a
finding with `severity: "blocking"` and the same fields; each note string becomes a finding with
`severity: "note"` and the string as `title` and `why`. The wrapper adds the reviewed `sha` and the
`source`.

## What the tab shows

- A finding with `file` and `line` in a changed file is a box below its line (the range when
  `endLine` is given) on the new side, with severity, title and text.
- Everything else is listed under "Other findings": files without a diff (including deleted
  files and binary or too large ones), findings without a line or without a file, lines beyond
  the end of the file.
- "Send to agent" adds the finding to the pending comments of the worktree; they are sent with
  the other comments. A finding without a line is sent as its path only, one without a file as
  `(general)`.
- Stale: when `sha` is not `HEAD` of the worktree (compared as prefixes) the tab says so; the
  findings are still shown.
- Invalid: when the file does not parse or breaks the schema, the tab shows a note with the
  validation error and ignores the file; the diff works as usual.
