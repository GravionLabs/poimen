# ADR 0007: Plugin system with sandboxed worker and view contexts

- Status: Proposed
- Date: 2026-11-08
- Issues: #550, Feature #549
- Supersedes: [ADR 0005](0005-github-views-and-agent-harnesses.md) (in part: the section "No plugin system now, one later")
- Builds on: [ADR 0003](0003-own-terminal-backend-without-herdr.md), [ADR 0005](0005-github-views-and-agent-harnesses.md)

## Context

ADR 0005 built GitHub views, harnesses, tasks and diff review as built-in features and deferred a plugin system to its own ADR, "once the built-in features show which extension points are stable". Those features now exist and share the same entry points: the command palette (`src/app/core/command-palette.ts`, `PoimenCommand`), editor tabs (`src/app/core/editor-tabs.ts`, `TabKind`), the activity bar and sidebar, the settings view and color schemes (ADR 0002). Each new feature so far adds a value to a closed union such as `TabKind` and a case in core code.

The removed herdr integration (#234) showed what not to repeat: plugins as Node processes that only exist on one backend and need a runtime next to poimen on every machine.

A plugin can do real damage in poimen. The app starts agents and terminals, writes into panes and runs `gh` and `git` with the user's logins. The project config already has a trust check for exactly this reason (#255, `src-tauri/src/trust.rs`). The webview currently runs without a content security policy (`"csp": null` in `tauri.conf.json`).

Prior art: [Asyar](https://github.com/Xoshbin/asyar), a Tauri 2 launcher with a mature extension system (docs under `docs/explanation/`: two-tier model, extension runtime, IPC bridge, permission system). Its host (`asyar-launcher`) is **GPL-3.0-only**. Only `asyar-sdk` and `asyar-ext-builder` are MIT, and both are tied to Asyar's service catalog. poimen is MIT, so it takes over Asyar's concepts, not its code.

## Decision

### Two tiers, one contribution API

- **Built-in features** (tier 1) stay Angular code in the app. They register what they contribute (palette commands, tab kinds, sidebar panels, settings sections, themes) in one **contribution registry** instead of extending closed unions in core files. They run unsandboxed.
- **Plugins** (tier 2) are installed from disk and contribute the same kinds of things through the same registry, from a sandbox, over a message bridge.
- A built-in feature can be disabled. Its UI contributions disappear, and its service (for example the shared `gh` client) stays available to plugins that hold the matching permission.

### Manifest and discovery

- Each plugin is a directory `<config dir>/plugins/<id>/` with a `plugin.json`. The directory name equals `id` (reverse domain, for example `com.example.ports`).
- The schema is closed: an unknown field fails discovery (`deny_unknown_fields`). Its fields are `id`, `name`, `version`, `description`, `author`, `minAppVersion`, `type` (`plugin` or `theme`), `contributes` (`commands`, `views`, `settings`), `worker`, `permissions` and `permissionArgs`.
- At startup poimen reads the manifests only. A plugin's code runs only when the plugin is enabled.
- Development: `poimen plugin link <dir>` (CLI of #175) registers a source directory. Version 1 has no store; installing means copying a directory or unpacking a release archive, followed by consent.

### Worker and view contexts

Each enabled plugin gets up to two separate JavaScript contexts that share no state:

| Context | Runs as | Lifetime | Holds |
|---|---|---|---|
| Worker | Web Worker, no DOM | while the plugin is enabled | event subscriptions, palette command handlers, timers, RPC handlers |
| View | `<iframe sandbox="allow-scripts allow-same-origin allow-forms">` inside an editor tab or sidebar panel | while the tab or panel is open, evicted after idle | UI only |

The view calls the worker through an RPC carried by the host (correlation id, default timeout of 5 s, abort signal). Because the view is an iframe, a plugin can use any UI framework. poimen ships the helix-ui design tokens as a CSS file so plugin views match the app.

### Plugin URI scheme

- Plugin files are served from the custom scheme `poimen-plugin://<id>/…`. The Rust handler rejects `..` segments and serves only from the plugin's directory.
- Lookup order is dev link, then built-in, then user directory, so a plugin cannot shadow a built-in id.
- Every response carries a strict CSP: scripts, styles and fonts only from the plugin's own origin, no remote `script-src` or `img-src`. Network access goes through a host service.
- Precondition: the app window itself gets a real CSP instead of `"csp": null`.

### Message bridge and permission gate

- Plugin contexts have **no Tauri IPC**. The Tauri capabilities stay scoped to the app's own window. Every call goes `postMessage` → host router → Tauri command.
- Every message is `{ type, messageId, payload }`, with `type` as `poimen:api:<service>:<method>`. The host answers every message, if necessary with an error, so no call hangs.
- The router runs a fixed pipeline: filter by protocol, identify, check permission, dispatch.
- The caller's identity comes from `event.source` (which worker or iframe sent it), **never from the payload**.
- Permissions are checked twice and fail closed: in the frontend router and again in Rust.

### Permissions and consent

Initial permission set (names are a proposal):

| Permission | Allows | `permissionArgs` |
|---|---|---|
| `spaces:read`, `panes:read` | list spaces, panes, sessions; receive their events | – |
| `agents:read` | agent status events | – |
| `palette`, `views` | contribute commands, tabs and panels | – |
| `panes:write` | send input to a pane | – |
| `process:spawn` | start a program in a new pane | `programs`: argv templates, never shell strings (rule of #235) |
| `files:read` | read files | glob list |
| `github:read` | read through the shared `gh` client | – |
| `network` | HTTP requests through the host | host list |

Before a plugin's first start, the user sees its permissions and their arguments in a consent dialog, similar to the trust dialog. The consent is stored with the SHA-256 of `plugin.json`, as with `trustedProjects` (#255). A changed manifest needs consent again.

### Lifecycle

Each context follows one state machine: `Dormant → Mounting → Ready`, and `Degraded` after three failed mounts within 5 minutes, with a cooldown before the next attempt.

- A **mount token** sent with each mount attempt and echoed in the ready message keeps a late message from a crashed attempt from promoting a new one.
- A **mailbox** holds messages that arrive while a context is mounting and delivers them on ready.
- Errors of one plugin are shown on that plugin, never as an app error.

### Themes without code

`type: "theme"` plugins contain only color schemes in the format of ADR 0002 and get no worker or view. They extend the scheme list of the settings.

### Backends and remote spaces

Plugins run only in the app, never on a remote machine. They reach spaces, panes and agents only through host services that sit on the `Backend` trait (ADR 0003). The same plugin therefore works on the herdr backend, the poimen backend and remote spaces (#11). Plugins as separate processes (the herdr model) are out of scope.

## Consequences

- New built-in features stop extending closed unions in core files. The contribution registry has to be introduced first, and existing features (GitHub, changes, flow, deliver, settings) have to move onto it. This is the largest part of the work and pays off even without a single external plugin.
- Third parties can write plugins in any web stack without Node, Rust or a poimen build.
- The app has to get a CSP, a URI scheme handler, a router, a consent store and a lifecycle manager. Each part needs tests of its security properties: no identity from the payload, path traversal, deny by default.
- Every host service exposed to plugins becomes public API. Changes to it need versioning (`minAppVersion`, an SDK package `@poimen/plugin-sdk` under MIT).
- Write permissions (`panes:write`, `process:spawn`) can steer agents. They come in a later stage than the read-only permissions.
- Code from `asyar-launcher` must not be copied. Contributors read the Asyar docs, not its host sources.
- Open before acceptance:
  - Should plugins receive agent events in the core format of agent-events, the same one the displays read?
  - Which icon set may plugins use?
  - Install from GitHub releases in version 1, or only from a local directory?
  - Exact idle timeout for views.

## Alternatives

- **No plugin system (stay with ADR 0005).** Every extension stays a poimen change. It is simpler, but nobody outside the poimen authors can extend the app, and core files keep growing.
- **Process plugins over stdio NDJSON**, like `docs/protocol.md` or the old herdr plugins. They would allow any language and could run on remote machines, but they need a runtime per machine. They also cannot render views, so poimen would need its own UI description language. Rejected for version 1.
- **Dynamic `import()` of plugin bundles into the app window.** There would be no isolation: the plugin could reach Tauri IPC and the DOM directly, and its bundled SDK copy would clash with the app's. Asyar used this approach and replaced it with iframes for exactly these reasons.
- **WebAssembly components (for example wasmtime in Rust).** They give strong isolation and any language for logic, but they offer no UI, so views would still need iframes, and the plugin toolchain gets harder.
- **Reusing Asyar's code.** The host is GPL-3.0-only and incompatible with poimen's MIT license. The MIT SDK is bound to Asyar's services and brings little.
