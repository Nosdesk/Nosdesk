# Internationalization (i18n)

Both backend and frontend share one set of [Fluent](https://projectfluent.org/)
(`.ftl`) catalogues at workspace root, so a translation lives
in one place and renders identically in transactional email
and the UI.

## Layout

```
i18n/locales/
├── en-US/main.ftl   # canonical — author keys here first
├── en-GB/main.ftl
├── en-AU/main.ftl   # minor wording divergence (G'day, "Time Zone")
├── fr-FR/main.ftl   # DRAFT — formal "vous" register
└── nl-NL/main.ftl   # DRAFT — formal "u" register
```

Keys are `kebab-case` and grouped by feature prefix
(`password-reset-*`, `settings-*`, `notif-*`, `login-*`, etc.).

## Backend loading

`backend/src/utils/i18n.rs` `include_str!`s each FTL file at
compile time. The Dockerfiles `COPY i18n /i18n` so the
`../../../i18n/locales/...` relative path resolves both on the
host (where it lands at workspace-root `i18n/`) and inside the
container's `/i18n`.

`SUPPORTED_LOCALES` in `backend/src/utils/locale.rs` lists the
active tags. Bundles are built once via `OnceLock` so the cost
is amortized on first lookup.

Lookup API:

```rust
crate::utils::i18n::tr_with(
    &locale,
    "password-reset-subject",
    &[("app", branding.app_name.clone().into())],
)
```

Missing keys return `{bracketed-key}` (loud but non-fatal) so
gaps show up in any rendered output without crashing the
response. Missing locales fall through to `DEFAULT_LOCALE`
(`en-US`).

## Frontend loading

`frontend/src/i18n/index.ts` uses Vite's
`import.meta.glob('../../../i18n/locales/*/main.ftl', { eager: true, query: '?raw' })`
to inline every catalogue at build time. Same relative path as
the backend so contributors don't have to learn two trees.

`compose.dev.yaml` bind-mounts the workspace `./i18n` into the
frontend-watch container at `/i18n` (read-only). `vite.config.ts`
sets `build.watch.include = ['src/**', '../i18n/locales/**']`
(gated on the `--watch` CLI flag so plain `vite build` still
exits) so new FTL files trigger an auto-rebuild rather than
needing a watcher restart.

Components consume:

- `$t('key', { arg: value })` in templates (registered as a
  global property by `fluent-vue`).
- `useFluent().$t(...)` in setup-script contexts.

## Resolution chain

`utils/locale::effective_locale` walks:

1. `user_preferences.locale` if set and supported.
2. `site_settings.default_locale` if set and supported.
3. `DEFAULT_LOCALE` (`en-US`).

`/auth/me` returns the resolved value as `effective_locale`
and `effective_timezone`. The frontend's
`dateStore.loadFromUser` seeds both; `useDateFormat` + `DateCell`
read them reactively so flipping a preference re-renders all
date strings live.

## Inbound Content-Language (channels)

Inbound email messages carry an RFC 3282 `Content-Language`
header. The IMAP parser surfaces it on `InboundMessage.content_language`
and the pipeline threads it into `auto_ack::send_auto_ack`. A
French-written guest ticket gets a French acknowledgement
without the recipient ever setting a preference; the resolver
chain still applies (so an inbound `de-DE` with no German
catalogue falls back to site default → en-US).

## Adding a new key

1. Write the canonical version in `en-US/main.ftl` first. Keep
   the key `kebab-case`, group it under its feature prefix.
2. Add equivalents to the other four locales. Fluent falls
   through to en-US for missing keys, so partial translations
   work (they render the English string for the missing key).
3. For HTML contexts, embed `<strong>` (or other formatting
   tags) inside the FTL value. Translators treat them as
   opaque markers around the words they wrap. Variables
   interpolated into HTML must be HTML-escaped at the
   Rust / Vue boundary **before** being passed into Fluent —
   Fluent does not escape arguments.

## Adding a new locale

Three edit points:

1. `backend/src/utils/locale.rs` — add the BCP-47 tag to
   `SUPPORTED_LOCALES`.
2. `backend/src/utils/i18n.rs` — add the matching
   `include_str!` entry in `FTL_SOURCES`.
3. `i18n/locales/<tag>/main.ftl` — create the file. Start by
   copying `en-US/main.ftl` and translating; keys you don't
   translate fall back to en-US at render time.

Frontend Vite picks up the new file automatically via the
eager glob; the `settings-locale-<tag>` label (used by the
settings picker dropdown) goes in the FTL file itself.

If you re-use a previously-unsupported locale tag as a test
fixture's "unsupported example", swap it for something still
outside the set (the existing tests use `de-DE`).

## Native review workflow

fr-FR and nl-NL ship marked **DRAFT — needs native review** in
their file headers. Corrections land as PRs that touch only
the affected FTL file. No code changes are required, no key
additions are required, and CI tests don't need to be updated
to refine an existing translation.

If you're a native speaker reviewing one of these locales:

1. Edit `i18n/locales/<your-locale>/main.ftl` directly.
2. Match each `key = value` line's English counterpart in
   `en-US/main.ftl` — they're in the same order with the same
   keys, so a diff between the two is the truth.
3. Preserve any `<strong>` or `{ $variable }` tokens exactly
   as they appear; they're load-bearing.
4. Open a PR. Drop the `DRAFT` line from the file header in
   your PR if you're confident the catalogue is now
   reviewer-ready.

## Gotchas

- **Backend rebuild required for new keys.** `include_str!`
  embeds FTL at compile time. Adding a key without restarting
  the backend means the lookup returns `{bracketed-key}` until
  the next `cargo run`.
- **User-authored content is not translated.** Comment bodies,
  ticket titles, signature text, and similar user-typed
  strings flow through verbatim. Only connector copy ("From:",
  "View in X", "Reply to this email") gets translated.
- **Admin-customised auto-ack templates bypass Fluent.** When
  `site_settings.channel_auto_ack_template` is set, the
  admin's wording wins outright — no machine translation, no
  template substitution beyond `{{ticket_id}}` and friends.
- **`vite build --watch` only.** The `build.watch.include`
  setting is gated on the `--watch` CLI flag because a non-
  null `build.watch` object forces Vite into watch mode for
  any invocation, including plain `vite build` (which would
  then hang waiting for changes).
