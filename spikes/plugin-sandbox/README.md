# Plugin sandbox spike 0 (throwaway)

Go/no-go for `docs/plugin-sandbox-plan.md`. Proves the core loading + isolation
mechanic of an opaque-origin sandboxed iframe. **Throwaway — do not merge.**

Two local origins stand in for the app and the sandbox:
- host ("app"):    http://localhost:5310/host.html
- sandbox:         http://127.0.0.1:5311  (different origin: localhost vs 127.0.0.1)

Both are secure contexts (localhost), so this exercises real cross-origin behaviour.

## Run

```
node spikes/plugin-sandbox/serve.mjs
```

Open http://localhost:5310/host.html in **each** browser and read the results
table: **Chrome, Firefox, Safari (macOS)**. Record which pass. The iframe uses
`sandbox="allow-scripts"` (opaque origin), `credentialless`, and a strict CSP.

## What each row proves

- **M1** the opaque-origin iframe imports `runtime.js` + dynamically imports the
  plugin `bundle.js` and calls its `mount()` — under `script-src <sandbox-host>`
  (not `'self'`, which is meaningless for an opaque origin) with cross-origin
  CORS. This is the mechanic that sinks naive sandboxes; it must pass.
- **M2** a `MessageChannel` round-trip works (the bridge transport). Note: the
  host transfers the port to the specific iframe `contentWindow`; it does NOT
  validate `event.origin`, because an opaque-origin frame reports
  `event.origin === "null"` (a real refinement to the plan's handshake).
- **M3a/b** the sandbox cannot read `document.cookie` / `localStorage` (opaque
  origin throws) — no host session reachable.
- **M3c** a direct `fetch` to the host origin is blocked by `connect-src 'none'`
  — the plugin has no network except the bridge.

If M1 fails, read the mount-result text and the browser console: it distinguishes
a CSP block from a CORS failure from an opaque-origin module-loading limitation.

## iOS (the real question — M5, next)

Desktop passing is necessary but not sufficient. The Tauri iOS WKWebView is the
make-or-break (WebKit `credentialless` support, cross-origin iframe under a custom
scheme). That is tracked by the parallel WebKit research pass and a follow-up that
loads this harness inside the iOS app. Do not conclude go/no-go until iOS is tested.
