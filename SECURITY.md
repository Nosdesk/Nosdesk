# Security

## Reporting a vulnerability

Email `security@nosdesk.com` with a write-up. Don't open a
public issue for exploitable bugs. We aim to acknowledge
within 72 hours.

## Deployment guidance

### Secrets that must be rotated for production

The dev `.env` (gitignored, not in the repo) ships a
working set of secrets for local development. Production
deployments must generate fresh values. At minimum:

| Variable | Purpose | How to generate |
|---|---|---|
| `JWT_SECRET` | Signs access + refresh tokens | `openssl rand -base64 32` |
| `MFA_ENCRYPTION_KEY` | AES-256-GCM for TOTP secrets at rest | `openssl rand -hex 32` |
| `POSTGRES_PASSWORD` | Database superuser | `openssl rand -base64 24` |
| `REDIS_PASSWORD` | Redis AUTH | `openssl rand -base64 24` |
| `MS_CLIENT_SECRET` | Microsoft Entra OAuth (if enabled) | From the Azure App Registration |

Rotating `JWT_SECRET` invalidates every existing session, so
plan for a forced re-login. Rotating `MFA_ENCRYPTION_KEY`
makes every existing TOTP secret in `mfa_secret_encrypted`
unreadable, so users will have to re-enrol MFA. Don't rotate
that one lightly.

### History rewrite (May 2026)

Two dev-only secret values used to live in a
`scripts/dev-native.sh` script. That script was in the repo
for a short window before being deleted, but the values were
committed in plain text and stayed reachable in git history.
In May 2026 we ran `git filter-repo` to drop the script and
its hardcoded values out of every commit.

If your deployment pre-dates the rewrite and used those
defaults in production, treat them as compromised and rotate
both `JWT_SECRET` and `MFA_ENCRYPTION_KEY`. Anyone who cloned
the repo before the rewrite still has the values locally; the
only mitigation that helps is rotation.

Forks running off an old clone: a force-push happened on the
rewrite date, so `git pull --rebase` won't fast-forward.
Re-clone, or `git fetch --all && git reset --hard origin/main`
after auditing the new history.

### Outbound email authentication (SPF / DKIM / DMARC)

Nosdesk sets a `From:` header derived from `SMTP_FROM_EMAIL`
and `SMTP_FROM_NAME` and hands the message to your SMTP relay.
It does not sign with DKIM itself, doesn't publish SPF, and
doesn't manage DMARC. Those records live in the DNS zone of
whichever domain you set `SMTP_FROM_EMAIL` to; configuring
them is an operator responsibility.

Without them, an attacker on any sender network can craft an
email whose `From:` matches your Nosdesk domain (`noreply@yourdomain.com`)
and most receivers will deliver it. Recipients see what looks
like a Nosdesk notification carrying the attacker's payload,
which is straightforward credential phishing against your
users. The fix is at the DNS layer, not in this codebase:

* **SPF**: publish a TXT record at `yourdomain.com` listing
  the IPs / hostnames allowed to send for you (typically your
  SMTP relay's recommended `include:`).
* **DKIM**: enable DKIM signing at your relay (SES, Postmark,
  SendGrid, Mailgun, Postal, etc. all expose this; for direct
  SMTP, set up `opendkim` on the relay host). Publish the
  public key as a TXT record at
  `<selector>._domainkey.yourdomain.com`.
* **DMARC**: publish a TXT record at `_dmarc.yourdomain.com`
  with at minimum `v=DMARC1; p=quarantine` (or `p=reject` once
  you've confirmed alignment from real sends). Start with
  `p=none; rua=mailto:...` to collect reports before tightening.

Verify with `dig TXT yourdomain.com`, `dig TXT <selector>._domainkey.yourdomain.com`,
and `dig TXT _dmarc.yourdomain.com`, or use `mail-tester.com`.

If your deployment can't publish DNS records (running on an
unowned domain, dev sandboxes, etc), set `SMTP_FROM_EMAIL` to
an address on a domain that *does* have these records, even
if it's a shared one. The application can't substitute for
DNS-level authentication.

### Reverse proxy expectations

`X-Forwarded-For` is gated on a `TRUSTED_PROXIES` CIDR
allowlist (env var). Set it to the network(s) your reverse
proxy connects from. Leave it unset on direct-bind
deployments, in which case XFF is ignored and the rate-limit
key is always the TCP peer.

A direct-internet bind without a proxy still works, but
binding to a private interface (default `127.0.0.1`) and
fronting with nginx / Caddy / Cloudflare is the recommended
shape. The proxy should set `X-Forwarded-For` itself and
strip any client-supplied value. The `TRUSTED_PROXIES` gate
is the second layer; the private bind is the first. See
AUD-004 and `utils::client_ip`.

#### Don't log query strings

Two endpoints carry a credential in the URL: the SSE stream
(`/api/events/stream`) and the collaboration WebSocket. This
isn't a design preference. `EventSource` and `WebSocket`
can't set an `Authorization` header, so the connection token
has to ride a query parameter.

Those tokens are deliberately short-lived (two minutes,
`CONNECTION_TOKEN_TTL_SECS`), scoped to a single channel, and
bound to one workspace. They're only accepted at connection
setup. But most proxies, CDNs, and load balancers log the
full request line including the query string by default, and
the collab token is write-capable, so an access log that
retains them is a store of replayable credentials for the
length of that TTL.

Configure your proxy to strip or redact query strings for
those paths, or at minimum keep access logs to the same
retention and access controls you'd give an auth log. On
nginx a custom `log_format` using `$uri` rather than
`$request` is enough. Cloudflare and most CDNs have an
equivalent redaction setting.

## Threat model

Nosdesk is a single-tenant helpdesk. There are four user
roles, in descending order of trust.

Admins have full administrative authority. Technicians run
the helpdesk: they can read every ticket, modify assigned
tickets, and use the internal-only features (notes,
suppression lists, audit log, etc). Users are the
end-customers or employees who submit tickets. They only see
tickets they're attached to as a requester or watcher.
Guests are unauthenticated and optional. When the admin
enables the public portal, guests can submit tickets but
nothing else.

This BSL build is single-tenant by design. Multi-tenant
isolation, where multiple operating organisations share one
instance, is reserved for the Enterprise plan and outside what
the BSL grants. The guest portal isn't multi-tenancy: guests
are end-customers of the licensee's own staff, the same as any
external sender hitting a support email address. The
restriction is on who operates the software, not on who
interacts with it.

## Audit log

A v1 pre-launch security audit was completed at the close of
the email-channel work, before the group-based visibility
refactor. It covered: AuthN, AuthZ, SQL injection, HTML
sanitisation, file upload, SSRF, info disclosure, HTTP
headers, rate limiting, outbound email, plugin sandboxing,
CSRF, secrets in logs, TLS, backup / restore.

### Visibility gate coverage

"Which tickets is this user allowed to read?" is answered in
exactly one place: `repository::ticket_visibility`. Two
consumers feed off it.

* The `extractors::TicketAccess` Actix extractor. Every
  ticket-scoped handler takes this as a parameter instead of
  raw `web::Path<i32>` + `AuthContext`. The visibility check
  runs during request extraction, so the handler body is
  unreachable for callers who can't read the ticket. A new
  ticket-scoped endpoint that "forgets" the gate won't
  compile; it would have to declare `web::Path<i32>` instead,
  which is the kind of thing code review catches.
* `visible_tickets_query(ctx)` and `visible_ticket_ids(conn,
  ctx, ids)`, used by `search` to drop ticket and comment
  results an end-user shouldn't see.

Denies return `404`, not `403`, per the OWASP IDOR
Cheatsheet. A `403` leaks ticket-id existence and enables
enumeration.

### Findings shipped before v1

| ID | Severity | Surface | Status |
|---|---|---|---|
| AUD-001 | High | IDOR on `GET /api/tickets/{id}` | **Fixed** by the group-aware visibility primitive. |
| AUD-002 | High | TOTP replay key uses non-crypto `DefaultHasher` | **Fixed**. Replay-cache key is now SHA-256 via `ring`, so it's deterministic across Rust toolchain bumps. |
| AUD-003 | Medium | Webhook + plugin-bundle SSRF (no internal-IP denylist) | **Fixed**. `utils::safe_http::client()` pins a custom `reqwest::dns::Resolve` that refuses internal IPs at resolution time. The client cannot dial RFC1918, loopback, link-local, CGNAT, or reserved ranges (v4 and v6, plus mapped-v4). One factory feeds webhook delivery, plugin registry, and plugin proxy. A one-line `reject_unsafe_ip_literal()` covers IP-literal URLs that bypass DNS. Operator allowlist via `NOSDESK_OUTBOUND_ALLOWED_HOSTS`. |
| AUD-004 | Medium | `X-Forwarded-For` trusted unconditionally for rate-limit keys | **Fixed**. `utils::client_ip` is the source of truth for "what IP is this request from." Honors XFF only when the TCP peer is inside `TRUSTED_PROXIES`. Walks XFF right-to-left and stops at the first non-trusted hop, so a spoofed leftmost entry can't bypass the gate. Consumed by rate limiters, API-token middleware, security-event logger, session record, MFA logger, passkey + password-reset rate keys, guest-submission audit. |
| AUD-005 | Medium | Unauthenticated onboarding-restore endpoints (race on first boot) | **Fixed**. The unauth restore endpoints are deleted; restore now ships as `nosdesk-cli db restore <file>`, gated on shell access (the pattern used by Sentry, Mastodon, Vault, etc.). `setup_initial_admin` wraps its count check and inserts in one transaction held by `pg_advisory_xact_lock`, so concurrent setup attempts serialise. The same endpoint now requires a one-shot bootstrap token written to `${UPLOAD_DIR}/bootstrap.token` (mode 0600) at first boot, deleted after success. Operators retrieve it via `docker compose exec backend cat ...`; network attackers reaching the listener can't proceed without it. |
| AUD-006 | Medium | SVG uploads not blocked from authed users | **Fixed**. `image/svg+xml` joins `BLOCKED_MIME_TYPES` and `.svg` / `.svgz` join `BLOCKED_EXTENSIONS` in `utils::file_validation`. A content sniffer runs ahead of `infer` (which often classifies SVG as `text/xml` and would slip past the MIME blocklist) and rejects anything whose first 1KB looks like SVG, including BOM- or whitespace-prefixed payloads renamed to non-SVG extensions. The sniffer is consulted by both the authed `validate_file` and the guest `validate_guest_upload`, so XSS-bearing SVG can't reach storage through any upload path. Plugin bundles are signed zips and don't touch this validator.
| AUD-007 | Medium | Password reset reveals user existence via timing side-channel | **Fixed**. The login family (`login`, `mfa_login`, `mfa_setup_login`, `mfa_enable_login`, `recovery_login`) routes through `utils::login_timing::verify_credentials`, which always runs `bcrypt::verify` against either the real hash or a random-bytes dummy hash with the same cost. Missing users, SSO-only users, and wrong passwords are indistinguishable in wall-clock time. The dummy hash is generated at startup so the cost matches `DEFAULT_COST` exactly; a statistical test asserts median delta < 20ms. `request_password_reset` runs token-issue + email-send in a detached `tokio::spawn` so the response is constant-latency. `start_passkey_login` always returns a discoverable-shape challenge with `sessionId`, regardless of whether the email maps to a user with passkeys; finish-time credential matching is the gate. Registration enumeration via the "email already exists" response is a separate concern (most products accept this leak; flagged in audit notes). |
| AUD-008 | Medium | Backup restore uses string-concatenated SQL (admin-gated) | **Fixed**. Restore is admin-gated but a poisoned backup uploaded by an admin (or by an attacker who phished one) was a SQL-injection vector via JSON column keys. The fix is layered: table names come from a hardcoded `ALLOWED_RESTORE_TABLES` list and every interpolation goes through `assert_table_allowed`; column names from backup JSON keys are filtered against `information_schema.columns` for the target table, so a hostile key in the backup file is silently dropped before reaching the INSERT; row values stay through the existing quote-doubling escape as the second layer. A DB-backed test verifies that a row with hostile keys (`"name); DROP TABLE users; --"`) inserts only the legitimate columns, leaving the schema untouched. |
| AUD-009 | Medium | Email From-address spoofing via configured channel | **Documented**. The application sets a `From:` header from `SMTP_FROM_EMAIL` and hands the message to your SMTP relay; SPF / DKIM / DMARC are operator responsibilities on the DNS for whichever domain that address belongs to. Without those records, an attacker can spoof Nosdesk-shaped emails against your users (credential phishing). The application can't substitute for DNS-level authentication. See the new "Outbound email authentication" section under "Deployment guidance" for the records to publish and verification commands. |
| AUD-010 | Medium | Image decompression-bomb defence not directly verified | **Fixed**. `utils::image::load_image_with_orientation` is the single decode chokepoint that avatar, banner, and thumbnail handlers all route through. It now calls `decoder.set_limits(decode_limits())` with strict `max_image_width = max_image_height = 16384` (covers any realistic photo or banner; 8K is 7680) and `max_alloc = 256 MiB`. The image-rs width/height limits are strict and are checked before any pixel buffer is allocated, so a tiny PNG header claiming `100_000 × 100_000` is rejected at parse time rather than after a 40 GB allocation. Tests cover a small image decoding normally, an image at the dimension limit decoding, an image one pixel past the limit being rejected, and the `decode_limits()` constants themselves. |
| AUD-011 | Low | Sibling write handlers (`update_ticket`, `delete_ticket`, etc.) likely need the same visibility check as AUD-001 | **Fixed**. The sweep applied the visibility gate to `watch_ticket`, `list_watchers`, `my_watch_state`, `update_ticket`, `update_ticket_partial`, `get_ticket_activity`, `get_comments_by_ticket_id`, `add_comment_to_ticket`, `set_ticket_tags`, `record_ticket_view`, and full-text `search` results for end-users. `bulk_tickets` is staff-only. `delete_ticket` was already admin-only. |
| AUD-012 | Low | Invitation acceptance rate-limit coverage unverified | **Verified + structural follow-up shipped**. Verification: invitation tokens are 32 random bytes (256-bit entropy, brute-force infeasible) and the `/api/auth` scope is wrapped in `RateLimiter::default()` keyed on `utils::client_ip`, so resource-exhaustion via flood is already covered. The investigation surfaced a separate concurrency bug: `validate_and_consume_token` was a non-atomic check-then-update, so two concurrent requests with the same token could both pass the `is_used = false` check and both call `accept_invitation`, leaving the account in an indeterminate state (the second password write wins). Replaced with a single `UPDATE reset_tokens SET is_used = true ... WHERE is_used = false AND expires_at > now() AND token_type = $1 RETURNING user_uuid`, so exactly one claim ever succeeds; all other failure modes collapse to the same "Invalid or expired token" message. Also closes a small message-distinguishability leak (the old code returned "Token has already been used" vs "Invalid token type"; now it doesn't). Four DB-backed tests cover succeed-once, second-attempt-fails, wrong-type-doesn't-consume, and missing-token-fails. The fix applies to password reset complete too, which shares the same primitive. |
| AUD-013 | Low | Lockout keyed by email only (can DoS a known user) | **Fixed**. `RateLimiter::login_attempt_key` now takes `(email, client_ip)` and the key shape is `login_attempts:{email}:{ip}`. An attacker who deliberately fails logins against a known email locks out their own IP without affecting the legitimate user's IP. Email stays in the key so an attacker can't trivially rotate IPs across every targeted account. The IP comes from `utils::client_ip` (the trusted-proxy-resolved helper from AUD-004), so behind a reverse proxy the lockout still keys on the real client; on a direct bind it falls back to the TCP peer. Tests cover key shape, lowercasing, the no-IP-known fallback, and the structural property that two different IPs against the same email get different keys. Every login-family handler (`login`, `recovery_login`, `mfa_setup_login`, `mfa_enable_login`, passkey setup-login start + finish) was migrated to the new signature.

### Dependency audit + triage policy

We run two scanners before each release:

* `cargo deny check` against `backend/deny.toml`, which covers
  advisories (RustSec DB), licence allowlist, source restriction
  to crates.io, and duplicate-version warnings.
* `npm audit` against the frontend lockfile. Currently clean at
  every severity level.

The `cargo-deny` config is the canonical record; every advisory
in its `ignore` list mirrors a row in the table below.

Triage policy:

| Class | Action |
|---|---|
| Critical / High vulnerability with a fixed version | Patch before release. No exceptions. |
| Medium / Low vulnerability with no upstream fix | Acknowledged in the table below with severity, attack pre-conditions, and threat-model rationale. Mirrored as an `ignore` entry in `deny.toml`. |
| Unmaintained advisory on a transitive-only dep | Tracked but not gated. Re-evaluated when a direct dep starts pulling it directly, or quarterly. |
| Unmaintained advisory on a direct dep | Treated as a migration ticket with an explicit deadline. |

### Acknowledged advisories

| Advisory | Severity | Why we ship anyway |
|---|---|---|
| RUSTSEC-2023-0071 (rsa 0.9.10 Marvin Attack) | 5.9 medium | Transitive through `openidconnect`. Our use is signature verification only (validating signed JWTs from Microsoft Entra), not decryption. Marvin targets decryption oracles, not signatures. No upstream fix available. Codified in `deny.toml`. |
| RUSTSEC-2024-0436 (paste 1.0.15 unmaintained) | warning | Was previously transitive via `image → ravif → rav1e`. Trimmed out by switching `image` to `default-features = false` and re-enabling only `webp`, `jpeg`, `png`, and `rayon`; `cargo tree -i -p paste` now returns empty. The crate remains in `Cargo.lock` as a conditional dep that would only activate if a consumer turned the `avif` feature back on, which is why `cargo audit` still surfaces it. Not in our shipped binary. |

### Confirmed-good surfaces

* HTTP security headers (CSP, HSTS, frame-ancestors, X-CTO,
  COOP, CORP, Permissions-Policy) in
  `backend/src/middleware/security_headers.rs`.
* CORS allowlist with no wildcard, in `backend/src/main.rs`.
* JWT validation pins HS256 with `exp` + `nbf` (30s leeway),
  rechecks role against the DB, and looks up the session.
  Lives in `backend/src/utils/jwt.rs`.
* Refresh tokens are 32 random bytes, SHA-256 hashed at rest,
  with family / session linkage.
* Password hashing: bcrypt cost 12.
* File upload validator: magic-byte detection, executable
  blocklist, guest allowlist, size caps (50 MB authed,
  10 MB guest), filename sanitisation. See
  `backend/src/utils/file_validation.rs`.
* Plugin bundles require Ed25519 signature verification
  before install. See `backend/src/services/plugins/signing.rs`.
* CSRF middleware wired; SameSite cookies on auth tokens. See
  `backend/src/middleware/csrf.rs`.
* Outbound queue + bounce handling + suppression list ship
  with duplicate-DSN dedup and conservative hard-bounce
  classification per RFC 3463.
* HTML sanitisation: every `v-html` consumer routes through
  DOMPurify via `frontend/src/composables/useSanitise.ts`.
  Email HTML renders in a sandboxed iframe.
* `npm audit` clean at all severity levels.
* Structured-log field allowlist for production output.
  `backend/src/utils/tracing_redact.rs` is the canonical
  policy — see "Log redaction" below.

## Log redaction

The default `tracing_subscriber` JSON formatter would emit
every field on every event — including any `email`,
`user_name`, `ticket_title`, etc. that a call site happens to
attach. For a multi-tenant helpdesk shipping logs to a hosted
log store, that's the moment the operator becomes a
sub-processor of every tenant's customer text.

The production output path runs through a field-allowlist
layer in `backend/src/utils/tracing_redact.rs`. Fields whose
name is not in `ALLOWED_FIELDS` are dropped before the JSON
line is written; an aggregate `redacted` counter is attached
to the event so operators can observe the rate without
inspecting the values. Span ancestry is walked and span
fields are subject to the same allowlist, so a
`#[instrument(fields(email = %u.email))]` is dropped even
though the call site looks fine.

The layer activates on `LOG_FORMAT=json` (the production
Docker configuration). Local development keeps the pretty
formatter — developer laptops aren't sub-processors of
anyone's data.

### What's allowed

| Category | Examples |
|---|---|
| Tenant-internal stable IDs | `ticket_id`, `comment_id`, `asset_id`, `workspace_id`, `user_uuid`, `requester_uuid`, `assignee_uuid`, `cycle_id`, `policy_id`, `provider_id`, `state_id`, `id` |
| Bounded enums | `status`, `priority`, `role`, `recurrence`, `category`, `event_type`, `op`, `aggregate`, `kind` |
| Counts / timings / outcomes | `count`, `elapsed_ms`, `latency_ms`, `error`, `error_kind`, `code`, `stamped` |
| HTTP context | `method`, `route` |
| Trace correlation | `request_id`, `span_id`, `trace_id` |
| Tracing internals | `message`, `log.file`, `log.line`, `log.module_path`, `log.target` |

### What's denied (silently dropped)

Anything not in the allowlist. The classes the audit
specifically called out:

- **User-identifying free text** — `email`, `user_email`,
  `user_name`, `display_name`, `user_principal_name`,
  `requester_name`, `assignee_name`, `name`.
- **User-typed content** — `title`, `description`, `body`,
  `content`, `comment_body`, `subject`, `original_filename`.
- **Network identifiers** — `ip`, `ip_hash`, `user_agent`,
  `host`.
- **Credentials** — `token`, `access_token`, `refresh_token`,
  `api_key`, `secret`, `password`.
- **Whole-payload splats** — `request_body`, `entities`,
  `ticket_update`, `update`.

A unit test pair (`allowed_fields_pass` +
`pii_fields_are_redacted`) enforces both halves at every CI
run. The audit trail for any future change is the PR that
touches `ALLOWED_FIELDS`.

### Adding a field

Open a PR that adds the field name to `ALLOWED_FIELDS` and to
`allowed_fields_pass`. In the PR description, answer: *is
this field always safe to log regardless of which tenant the
request is inside?* If the answer isn't an obvious yes, log
the stable ID for the underlying row and look up the
sensitive text via `audit_log` when investigating. That PR is
the audit surface SOC 2 CC6.7 (data classification) expects.

### Where redacted data still lives

The allowlist only governs `tracing` output. PII / customer
text is still persisted in the application database and in
`audit_log` rows. Operator access to those tables is scoped
by RBAC and recorded — that's the intentional channel for
forensic lookup. `tracing` is the wrong place to reach for
raw customer text.

### Operational notes

- `EnvFilter` is attached via `Layer::with_filter`, not on
  the registry, so the registry retains every span for
  `LookupSpan`. `tracing-actix-web`'s per-request span
  (carrying `request_id`) is reachable via `ctx.event_scope`
  even when its bare "served" event is suppressed (by target
  check inside `on_event`).
- Failure mode is silent drop. A field outside the
  allowlist produces no warning, just an increment to the
  per-event `redacted` counter. A noisy redaction warning
  would itself become an information leak.
- Redaction applies at all levels including `DEBUG`. If a
  debug session needs the raw value, query `audit_log` or
  attach a debugger; don't loosen the policy.
