# @nosdesk/mobile

Host layer for the Tauri app: wires the headless `@nosdesk/core` seams to the
native platform. The mobile twin of the web setup files
(`frontend/src/services/transport.ts`, `frontend/src/utils/{storageSetup,loggerSetup}.ts`,
`frontend/src/services/apiConfig.ts`).

## What it wires

`bootstrapMobile({ apiBaseUrl, collabWsBaseUrl, secureStore, logger? })` (call
once at app start) configures, in order:

- **logger** (`loggerSetup`): min level + user-id resolver.
- **storage** (`storageSetup`): the general KV seam over the webview's
  `localStorage` (drafts, recent items, prefs). Not for secrets.
- **transport** (`transport`): the `bearerAuthStrategy`, `Authorization: Bearer`
  + `X-Auth-Mode: bearer`, `useCredentials: false`, refresh via the rotating
  refresh token. Access token in memory; refresh token in the keychain.
- **api client** (`apiClient`): the transport interceptors on core's shared
  axios instance (base URL, credential mode, auth headers, 401 -> refresh ->
  retry).

The login / sign-out flows call `setSession(access, refresh)` / `clearSession()`.

## Targets the bearer backend

Depends on the additive bearer auth in the backend: login/refresh return tokens
in the body for `X-Auth-Mode: bearer`, and the validator accepts a session JWT
as a bearer credential.

## Stubbed / next steps

- **`SecureStore` keychain impl** (`secureStore.ts`): only `memorySecureStore`
  (dev, non-persistent) ships today. The production `tauriSecureStore` over the
  OS keychain is sketched in a comment, fill it in once the native shell and the
  keychain plugin are chosen.
- **Native Tauri scaffold** (`src-tauri`, `tauri.conf.json`, mobile targets) and
  the **UI layer** are separate, larger pieces, not part of this bootstrap.
- No runtime tests yet (no test runner wired); the modules are type-checked
  against `@nosdesk/core`.
