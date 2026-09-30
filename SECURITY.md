# Security

## Reporting a vulnerability

Report vulnerabilities privately, by email to `security@nosdesk.com` or through
[GitHub private vulnerability reporting](https://github.com/Nosdesk/Nosdesk/security/advisories/new).
Don't open a public issue.

Include what's affected (version or commit, self-hosted or the hosted service),
the steps to reproduce it, the impact, and any mitigation you know of.

We acknowledge reports within 72 hours and keep you updated while we work on a
fix. Once a release fixes the issue, we publish a GitHub Security Advisory that
credits you, unless you'd rather not be named. Please give us 90 days before
you publish details yourself. There is no paid bug bounty.

We won't pursue legal action over research that follows this policy. Test only
against your own installation or account, and don't access, change or keep
other people's data.

## Scope

In scope: the code in this repository, and the hosted service at `nosdesk.com`.

Out of scope: the configuration of a particular self-hosted installation (tell
its operator), denial of service by traffic volume, social engineering, and
missing SPF, DKIM or DMARC records on domains we don't operate.

## Supported versions

Security fixes ship in the latest release. Older releases and release
candidates don't receive backported fixes, so upgrade to stay covered.

## Running Nosdesk securely

### Secrets

Generate fresh values for every deployment: `JWT_SECRET`
(`openssl rand -base64 32`), `MFA_KEK_V1` (`openssl rand -hex 32`),
`POSTGRES_PASSWORD`, `REDIS_PASSWORD`, and the client secret of any identity
provider you connect. In production the server refuses to start with the
example values.

Rotating `JWT_SECRET` signs everyone out. `MFA_KEK_V1` encrypts MFA secrets,
channel credentials and plugin secrets at rest. To rotate it, add `MFA_KEK_V2`
and set `MFA_KEK_VERSION=2`. Keep `MFA_KEK_V1` set as well: existing rows are
re-encrypted only when they're next written.

### Reverse proxy

Bind Nosdesk to a private interface behind a reverse proxy that sets
`X-Forwarded-For` itself, and list the proxy's network in `TRUSTED_PROXIES`.
Without that setting, Nosdesk ignores `X-Forwarded-For` and rate-limits by the
connecting address.

### Access logs

The event stream (`/api/events/stream`) and the collaboration WebSocket carry a
two-minute connection token in the query string, because browsers can't send
headers on those connections. Don't log query strings for those paths. On
nginx, log `$uri` instead of `$request`.

### Outbound email

Nosdesk doesn't sign mail. Publish SPF, DKIM and DMARC records for the domain
in `SMTP_FROM_EMAIL`, or anyone can send mail that looks like yours. The admin
email settings check these records for you.

### Outbound requests

Webhooks, plugins and integrations refuse private and internal addresses. Allow
specific internal hosts, such as an LDAP or IMAP server, with
`NOSDESK_OUTBOUND_ALLOWED_HOSTS`.

## Threat model

An instance serves one organisation. Owners and admins manage the workspace,
agents handle tickets, and members see only the requests they're part of or
that are shared with them. Guests can submit requests through the portal when
it's enabled. Hosting separate organisations on one instance is an Enterprise
capability and outside this model.

## How we keep Nosdesk secure

- Every minor release gets a security review of the codebase before it ships,
  checked against the findings of earlier reviews. We fix release-blocking
  findings first and track the rest until they're resolved. A subsystem that
  changes substantially gets its own review in between.
- 1.0 had a pre-launch audit. Before 1.1: security audits in June and August
  2026, a full code review, and separate reviews of the plugin system and
  logging.
- CI runs CodeQL, `cargo deny` (RustSec advisories, licences and sources),
  OSV-Scanner and zizmor. Dependabot proposes dependency and security updates.
- We patch critical and high advisories that have a fix before the next
  release. `backend/deny.toml` lists the advisories we accept, each with its
  reason.
- Production logs keep only allowlisted fields such as IDs, counts and timings,
  and drop email addresses, names, customer text and credentials. The allowlist
  and its tests are in `backend/src/utils/tracing_redact.rs`.
