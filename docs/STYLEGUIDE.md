# Style guide

Generated text is written in US English. The UI style rules are in `AGENTS.md` (buttons, tags, fields, dialogs); this file covers what the lint enforces.

## Helix lint (#1367)

`pnpm lint` runs the `@gravionlabs/helix-lint` preset, and CI runs `pnpm lint` for frontend paths (`scripts/changed-paths.json`).

- **Colors and font sizes.** Component styles (`src/**/*.css`) use no hex, `rgb()`, `rgba()`, `hsl()` or `hsla()` color; use a token (`var(--h-*)`, `var(--app-*)`) or a keyword. `font-size` is `var(--h-font-size-*)`, a keyword, or `calc()`/`clamp()` over those tokens.
- **Controls.** A template uses no native `<button>`, `<input>`, `<select>`, `<textarea>` or `<progress>` without the Helix directive (`hx-button`, `hx-input`, `hx-checkbox`, `hx-radio`, `hx-switch`, `hx-select`, `hx-progress-bar`).

### Allowlist

An exception goes in `scripts/lint/helix-allow.json`, in `stylelint` or `eslint`:

```json
{ "files": ["src/app/legacy/**"], "rules": ["helix/no-raw-control"], "reason": "Why this cannot move yet" }
```

Every entry needs `files`, `rules` and a `reason`; without a reason `helixAllow` throws and `pnpm lint` fails. Prefer fixing the file and deleting the entry. `src/testing/lint-probe/` holds one failing sample of each rule (checked by `src/testing/lint-probe.spec.ts`).
