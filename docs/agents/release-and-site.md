# Updater and site

Self-update and the public site. Read before you touch these areas. Notes on a change go here, not in `AGENTS.md`.

- `src-tauri/src/updater.rs`: self-update through `tauri-plugin-updater` (`update_check`, `update_install`; a failed install keeps the update pending; `enabled(CHANNEL)`: an rc build registers no plugin and hides the update item). Only on a user click, no other network call; frontend `core/updater.ts` and the status bar; signing and flow in `docs/updater.md`.
- `site/`: the public site on GitHub Pages (`https://gravionlabs.github.io/poimen/`), VitePress, self-contained (own npm lockfile, excluded from the app's eslint and prettier). The ADRs of `docs/adr/` are not published there; a dead link fails `npm run build`. Download links are resolved client-side from the latest release (`site/downloads.ts`, tested). Workflow `.github/workflows/pages.yml`: dry-run build on pull requests touching `site/`, deploy on push to `main`; one job calling `GravionLabs/ci` `vitepress-pages.yml@v1` (`site-path: site`, its input `deploy` defaults to `true`).
