# plugin-bridge harness

Proves the host↔plugin **Comlink round-trip** for the plugin sandbox, using the
real `@nosdesk/plugin-sdk` (`createRemoteHostApi` + `connectToHost`) and real
`@nosdesk/plugin-runtime`, through an opaque-origin sandboxed iframe.

Its sibling `../plugin-sandbox/` proved *isolation* and was deleted once the
sandbox shipped (every trust tier now runs in the opaque-origin frame). This
harness is kept because it exercises the real SDK and runtime rather than a
stand-in.

```bash
npm install
npx playwright test        # chromium + webkit
```

The flow: the host embeds `sandbox="allow-scripts"` iframe → the runtime
`connectToHost()`s and imports the token'd bundle → the plugin calls
`api.tickets.get(42)` (host→plugin return) then `api.notify(title)`
(plugin→host arg). The host stub records the notify arg into `#result` on the
host origin, so the round-trip is asserted without reaching into the opaque
frame. `build.mjs` esbuilds all three actors from real source (SDK aliased to
its TS; comlink resolves from the SDK's node_modules).

2026-07-22: **PASS on Chromium + WebKit.** WebKit is the Safari/iOS signal.
