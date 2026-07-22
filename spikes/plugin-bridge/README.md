# plugin-bridge harness

Proves the host↔plugin **Comlink round-trip** for the plugin sandbox, using the
real `@nosdesk/plugin-sdk` (`createRemoteHostApi` + `connectToHost`) and real
`@nosdesk/plugin-runtime`, through an opaque-origin sandboxed iframe. Sibling to
`../plugin-sandbox/` (which proves *isolation*); this one proves the *bridge*.

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
