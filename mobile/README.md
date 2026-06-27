# @nosdesk/mobile

The Tauri host for the mobile app. It runs the **existing** `frontend` Vue app
(already mobile-first, bottom nav + safe-area insets) in a Tauri 2 shell and
wires the headless `@nosdesk/core` seams to the native platform (bearer auth +
native HTTP, vs the web's cookies + localStorage). There is no separate mobile
UI; `@nosdesk/core` is what lets the one UI run in both hosts.

## Layout

- `src-tauri/` — the Tauri 2 Rust shell. `tauri.conf.json` builds the frontend
  (`frontendDist: ../../frontend/dist`, `beforeBuildCommand` runs the frontend
  build) and serves its dev server in `tauri dev`. Registers `tauri-plugin-http`.
- `src/` — the TS host layer the frontend loads when running under Tauri:
  - `bootstrap.ts` / `index.ts` — `bootstrapMobile()` + `setSession`/`clearSession`.
  - `transport.ts` — the `bearerAuthStrategy` (Authorization: Bearer + `X-Auth-Mode`).
  - `apiClient.ts` — clears the web interceptors and installs the bearer ones +
    the native-HTTP adapter on core's axios instance.
  - `tauriHttpAdapter.ts` — axios adapter over `@tauri-apps/plugin-http` (the
    `tauri://` origin can't reach the API cross-origin from the webview).
  - `storageSetup.ts` / `loggerSetup.ts` — the general-KV + logger seams.
  - `secureStore.ts` — the keychain `SecureStore` contract + an in-memory impl.

The frontend chooses the host at startup in `frontend/src/platform/index.ts`
(`isTauriRuntime()` → `bootstrapMobile(...)`, else the web setup); the Tauri
branch is a lazy chunk, so the web bundle stays Tauri-free.

## Verified

- `pnpm --filter @nosdesk/mobile run type-check` and `pnpm -C frontend run type-check` green.
- `pnpm -C frontend run build-only` green; Tauri code is code-split out of the web entry chunk.
- `cd src-tauri && cargo check` compiles the Rust shell.

## NOT done (needs the mobile SDKs / a device)

- **Init the mobile targets** (Tauri CLI present, SDKs are not):
  `pnpm --filter @nosdesk/mobile tauri ios init` / `tauri android init`
  (iOS needs Xcode; Android needs the SDK + the `aarch64-*` Rust targets).
  `pnpm --filter @nosdesk/mobile tauri dev` runs the desktop shell.
- **Keychain `SecureStore`** (`secureStore.ts`): only `memorySecureStore` ships
  (no cold-start persistence). Pick a non-biometric-gated keychain plugin
  on-device, NOT Stronghold (deprecated) or the plaintext store plugin.
- **Tune** the placeholder API host (`com.nosdesk.app` identifier, the CSP
  `connect-src` and the `http` capability `allow` URL all point at
  `app.nosdesk.com`) and confirm SSE (EventSource) + collab WS against the real
  deployment, the backend must allowlist the Tauri origin for those (the http
  plugin handles REST natively, but SSE/WS go through the webview).
- App icons (placeholders), signing, store metadata; native enhancements (push,
  biometric unlock, deep links, native passkeys); on-device smoke test
  (login → `/me` → ticket list).
