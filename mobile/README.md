# @nosdesk/mobile

The Tauri 2 host for the mobile app. There is no separate mobile UI: this runs
the existing `frontend` Vue app and rebinds the headless `@nosdesk/core` seams
to native (bearer auth and native HTTP, where web uses cookies and localStorage).

## Layout

`src-tauri/` is the Rust shell. `tauri.conf.json` points `frontendDist` at
`../../frontend/dist` and rebuilds it via `beforeBuildCommand`.

`src/` is the TS host layer the frontend loads under Tauri:

| | |
|---|---|
| `bootstrap.ts`, `index.ts` | `bootstrapMobile()`, `setSession`, `clearSession` |
| `transport.ts` | bearer auth strategy (`Authorization`, `X-Auth-Mode`) |
| `apiClient.ts` | swaps core's web interceptors for the bearer ones |
| `tauriHttpAdapter.ts` | axios adapter over `@tauri-apps/plugin-http` |
| `serverConfig.ts` | server picker, base URLs, `validateServer()` |
| `secureStore.ts` | `SecureStore` contract (iOS Keychain / Android Keystore) |
| `storageSetup.ts`, `loggerSetup.ts` | general KV and logger seams |

`frontend/src/platform/index.ts` picks the host at startup. The Tauri branch is
a lazy chunk, so the web bundle stays Tauri-free.

## Device builds

```bash
pnpm --filter @nosdesk/mobile exec tauri ios build --debug --export-method debugging
xcrun devicectl device install app --device <UUID> src-tauri/gen/apple/build/arm64/Nosdesk.ipa
```

`--export-method debugging` produces a standalone app. `tauri ios dev` instead
tethers the webview to the laptop's vite server, so the app stops working as
soon as the laptop does.

Gotchas:

- The bundled UI comes from `frontend/dist`, so the app always ships a
  production frontend build rather than whatever the dev stack has in
  `backend/public`.
- Touch `src-tauri/src/lib.rs` after a frontend-only change. The Rust side embeds
  the assets and silently ships the previous bundle if it has no reason to
  recompile.
- Unlock the login keychain first or `CodeSign` fails at the end of an otherwise
  clean build: `security -v unlock-keychain ~/Library/Keychains/login.keychain-db`.
- Target the device by UUID (`xcrun devicectl list devices`). Device names often
  contain a curly apostrophe, so passing a name with a straight quote
  (`--device "Sam's iPhone"`) will not match.

## Simulator

`tauri ios build --target aarch64-sim` fails with `failed to rename app ...
Directory not empty` when a device archive exists. Xcode still built the app, so
skip Tauri's packaging and install its output:

```bash
xcrun simctl boot <SIM-UUID> && open -a Simulator
xcrun simctl install <SIM-UUID> \
  ~/Library/Developer/Xcode/DerivedData/app-*/Build/Products/debug-iphonesimulator/Nosdesk.app
xcrun simctl launch <SIM-UUID> com.nosdesk.app
```

`simctl` installs, launches and screenshots but cannot tap or swipe. Driving the
Simulator over AppleScript needs Accessibility permission and otherwise hangs
until `AppleEvent timed out (-1712)`. Gesture testing needs a real device.

## Open

- Backend must allowlist the Tauri origin per server for SSE and collab WS. The
  http plugin covers REST natively, but those two go through the webview.
- App icons are placeholders. Signing and store metadata are not set up.
- Native work not started: biometric unlock, native passkeys.
