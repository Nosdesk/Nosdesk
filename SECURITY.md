# Security

## Reporting a vulnerability

Email `security@nosdesk.com` with a write-up. Don't open a
public issue for exploitable bugs. We aim to acknowledge
within 72 hours.

## Deployment guidance

### Secrets that must be rotated for production

The dev `docker.env` (gitignored, not in the repo) ships a
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
| AUD-005 | Medium | Unauthenticated onboarding-restore endpoints (race on first boot) | Tracked. |
| AUD-006 | Medium | SVG uploads not blocked from authed users | Tracked. |
| AUD-007 | Medium | Password reset reveals user existence via timing side-channel | Tracked. |
| AUD-008 | Medium | Backup restore uses string-concatenated SQL (admin-gated) | Tracked. |
| AUD-009 | Medium | Email From-address spoofing via configured channel | Deployment / config concern. |
| AUD-010 | Medium | Image decompression-bomb defence not directly verified | Tracked. |
| AUD-011 | Low | Sibling write handlers (`update_ticket`, `delete_ticket`, etc.) likely need the same visibility check as AUD-001 | **Fixed**. The sweep applied the visibility gate to `watch_ticket`, `list_watchers`, `my_watch_state`, `update_ticket`, `update_ticket_partial`, `get_ticket_activity`, `get_comments_by_ticket_id`, `add_comment_to_ticket`, `set_ticket_tags`, `record_ticket_view`, and full-text `search` results for end-users. `bulk_tickets` is staff-only. `delete_ticket` was already admin-only. |
| AUD-012 | Low | Invitation acceptance rate-limit coverage unverified | Tracked. |
| AUD-013 | Low | Lockout keyed by email only (can DoS a known user) | Tracked. |

### Acknowledged advisories

| Advisory | Severity | Why we ship anyway |
|---|---|---|
| RUSTSEC-2023-0071 (rsa 0.9.10 Marvin Attack) | 5.9 medium | Transitive through `openidconnect`. Our use is signature verification only (validating signed JWTs from Microsoft Entra), not decryption. Marvin targets decryption oracles, not signatures. No upstream fix available. |
| RUSTSEC-2024-0436 (paste 1.0.15 unmaintained) | warning | Transitive through `image → ravif → rav1e`. Warning-only, no CVE. Tracked upstream by image-rs. |

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
