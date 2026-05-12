# Security

## Reporting a vulnerability

Email `REDACTED@example.invalid` (or the project's current maintainer
address) with a write-up. Please don't open a public issue for
exploitable bugs. We aim to acknowledge within 72 hours.

## Deployment guidance

### Secrets that MUST be rotated for production

The dev `docker.env` (gitignored, not in the repo) contains a
working set of secrets for local development. Production
deployments MUST generate fresh values. At minimum:

| Variable | Purpose | How to generate |
|---|---|---|
| `JWT_SECRET` | Signs access + refresh tokens | `openssl rand -base64 32` |
| `MFA_ENCRYPTION_KEY` | AES-256-GCM for TOTP secrets at rest | `openssl rand -hex 32` |
| `POSTGRES_PASSWORD` | Database superuser | `openssl rand -base64 24` |
| `REDIS_PASSWORD` | Redis AUTH | `openssl rand -base64 24` |
| `MS_CLIENT_SECRET` | Microsoft Entra OAuth (if enabled) | From the Azure App Registration |

Rotating `JWT_SECRET` invalidates all existing sessions — operators
should plan for a forced re-login.

Rotating `MFA_ENCRYPTION_KEY` makes every existing TOTP secret in
`mfa_secret_encrypted` unreadable. Users will need to re-enrol
MFA. Don't rotate this lightly.

### History exposure (acknowledged)

Two legacy values shipped briefly in `scripts/dev-native.sh`
(introduced in `fe2d65c`, removed in `cade9ea`, Aug 12 2025):

* `JWT_SECRET = CPfynq2V6...mA=`
* `MFA_ENCRYPTION_KEY = c44b4a1d...93e5`

These values remain reachable in git history. Any deployment that
ever used `dev-native.sh`'s output as its production config should
be treated as compromised: forge sessions, decrypt at-rest MFA
secrets. The fix is rotation, not history rewriting (the values
are already public to anyone who has cloned the repo).

If a future maintainer wants a clean break, `git filter-repo
--invert-paths --path scripts/dev-native.sh` removes the file from
all of history. This breaks every existing fork / clone and only
makes sense if the leak isn't already broadly distributed.

### Reverse proxy expectations

The backend trusts `X-Forwarded-For` for rate-limit keying and
security-event logging. A direct connection to the backend (no
proxy in front) can forge any client IP and bypass per-IP
limits. In production:

* Bind the backend to a private interface (default: `127.0.0.1`).
* Place a reverse proxy (nginx, Caddy, Cloudflare, etc.) in front.
* Configure the proxy to set `X-Forwarded-For` itself, stripping any
  client-supplied value.

A planned hardening pass will gate `X-Forwarded-For` on a
`TRUSTED_PROXIES` CIDR allowlist; until then the deployment
guidance is the security control.

## Threat model

Nosdesk is a single-tenant helpdesk. Users are partitioned into:

* **Admin** — full administrative authority.
* **Technician** — operates the helpdesk; can read all tickets,
  modify assigned tickets, and use internal-only features.
* **User** — end-customer / employee submitting tickets. Sees only
  tickets they're involved with (requester / watcher).
* **Guest** — unauthenticated, optional. Can submit tickets via the
  public portal when the admin enables guest access.

The system is not designed for **multi-tenant isolation**. Two
orgs sharing one Nosdesk instance is out of scope; deploy
separate instances.

## Audit log

A v1 pre-launch security audit was completed against the
`b663765` commit (post-bounce work, before group-based visibility).
Audit covered: AuthN, AuthZ, SQL injection, HTML sanitization,
file upload, SSRF, info disclosure, HTTP headers, rate limiting,
outbound email, plugin sandboxing, CSRF, secrets in logs, TLS,
backup/restore.

### Findings shipped before v1

| ID | Severity | Surface | Status |
|---|---|---|---|
| AUD-001 | High | IDOR on `GET /api/tickets/{id}` | **Fixed by group-based visibility** (this commit) |
| AUD-002 | High | TOTP replay key uses non-crypto `DefaultHasher` | Tracked as task |
| AUD-003 | Medium | Webhook + plugin-bundle SSRF (no internal-IP denylist) | Tracked as task |
| AUD-004 | Medium | `X-Forwarded-For` trusted unconditionally for rate-limit keys | Tracked as task |
| AUD-005 | Medium | Unauthenticated onboarding-restore endpoints (race on first boot) | Tracked as task |
| AUD-006 | Medium | SVG uploads not blocked from authed users | Tracked as task |
| AUD-007 | Medium | Password reset reveals user existence via timing side-channel | Tracked as task |
| AUD-008 | Medium | Backup restore uses string-concatenated SQL (admin-gated) | Tracked as task |
| AUD-009 | Medium | Email From-address spoofing via configured channel | Deployment / config concern |
| AUD-010 | Medium | Image decompression-bomb defence not directly verified | Tracked as task |
| AUD-011 | Low | Sibling write handlers (`update_ticket`, `delete_ticket`, etc.) likely need same visibility check as AUD-001 | Tracked as task |
| AUD-012 | Low | Invitation acceptance rate-limit coverage unverified | Tracked as task |
| AUD-013 | Low | Lockout keyed by email only (can DoS a known user) | Tracked as task |

### Acknowledged advisories

| Advisory | Severity | Why we ship anyway |
|---|---|---|
| RUSTSEC-2023-0071 (rsa 0.9.10 Marvin Attack) | 5.9 medium | Transitive via `openidconnect`. Our use is signature verification only (verifying signed JWTs from Microsoft Entra), not decryption. Marvin targets decryption oracles, not signatures. No upstream fix available. |
| RUSTSEC-2024-0436 (paste 1.0.15 unmaintained) | warning | Transitive via `image → ravif → rav1e`. Warning-only, no CVE. Tracked upstream by image-rs. |

### Confirmed-good surfaces

* HTTP security headers (CSP, HSTS, frame-ancestors, X-CTO, COOP,
  CORP, Permissions-Policy) — `backend/src/middleware/security_headers.rs`.
* CORS allowlist (no wildcard) — `backend/src/main.rs`.
* JWT validation pins HS256 with `exp` + `nbf` (30s leeway), role
  recheck against DB, session lookup — `backend/src/utils/jwt.rs`.
* Refresh tokens are 32 random bytes, SHA-256 hashed at rest, with
  family/session linkage.
* Password hashing: bcrypt cost 12.
* File upload validator: magic-byte detection, executable blocklist,
  guest allowlist, size caps (50 MB authed / 10 MB guest),
  filename sanitisation — `backend/src/utils/file_validation.rs`.
* Plugin bundles require Ed25519 signature verification before
  install — `backend/src/services/plugins/signing.rs`.
* CSRF middleware wired; SameSite cookies on auth tokens —
  `backend/src/middleware/csrf.rs`.
* Outbound queue + bounce handling + suppression list shipped with
  duplicate-DSN dedup and conservative hard-bounce classification
  per RFC 3463.
* HTML sanitisation: every `v-html` consumer routes through
  DOMPurify via `frontend/src/composables/useSanitise.ts`. Email HTML
  rendered in a sandboxed iframe.
* `npm audit` clean at all severity levels.
