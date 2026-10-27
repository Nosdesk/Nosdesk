# Licensing

Self-hosted Nosdesk runs the **Community** edition, which is limited to a
single workspace. Creating additional workspaces requires a license key,
sold as **Simple** or **Advanced** on the
[pricing page](https://nosdesk.com/pricing) and managed at
manage.nosdesk.com. The server calls either one the **Enterprise** edition:
that is the name in the startup log and in `GET /api/admin/license`. Hosted
(SaaS) deployments provision workspaces through the control plane and are
not affected.

A license key is separate from the source license in [LICENSE](LICENSE),
the Business Source License 1.1, which lets you run Nosdesk in production
unless you offer it as a Hosted Helpdesk Service. Each release carries its
own Change Date, when it converts to the GNU AGPL v3: 2029-10-29 for every
release through 1.1.x, 2030-01-01 from 1.2.0.

The cap is enforced server-side in `admin_workspaces::create_workspace`:
on self-hosted deployments, once the active-workspace count reaches the
edition limit, `POST /api/admin/workspaces` returns `402 license_required`.

## How a license works

A license is an EdDSA-signed JWT carrying a stable customer id (`sub`, a
UUID that survives reissues), a display name (`licensee`), a per-issuance
id (`jti`, required), an active-workspace cap, an expiry, and an optional
`features` list (empty in v1.1; nothing new is gated on it). The public
verification keys are compiled into the binary and the token's `kid` header
selects one; a `kid` the binary does not trust is refused. The private
signing keys are held only by Nosdesk. A verified
license lifts the workspace cap to its `max_workspaces`; one that expires
while the server runs stops granting at `exp`. An absent, malformed,
expired, or wrong-issuer license falls back to Community without failing
startup.

Like any license check in code anyone can read, this can be bypassed by patching the binary. The
license is a genuine signed artifact, not an honor-system flag.

## Minting a license (Nosdesk operators)

Public keys live in `backend/license_keys/`, one PEM per signing key:
`production.pem` is trusted by every build, `staging.pem` only by builds
compiled with `NOSDESK_TRUST_STAGING_LICENSES=1` (the dev stack and the
nosdesk-dev image). Release images trust production alone.

```bash
openssl genpkey -algorithm ed25519 -out license_private.pem
openssl pkey -in license_private.pem -pubout -out backend/license_keys/<name>.pem
```

Sign a license:

```bash
nosdesk-cli license sign \
  --key license_private.pem \
  --customer-id 550e8400-e29b-41d4-a716-446655440000 \
  --licensee "Acme Corp" \
  --max-workspaces 10 \
  --days 365
```

`--customer-id` is required and must be a UUID. Reissues of the same
customer **must** pass the same id; omitting it (or generating a fresh one)
would split the usage meter. Record issued ids deliberately until
control-plane issuance exists. `jti` is generated if `--license-id` is
omitted.

The token (prefixed `nsk_lic_`) is printed to stdout.

## Applying a license (self-hosters)

Paste the key in **Admin › License**. It is verified on the server before
it is stored (encrypted) and takes effect without a restart.

Or set it in the environment, which wins over a pasted key and makes the
admin page read-only:

```bash
NOSDESK_LICENSE_KEY=nsk_lic_...
```

The startup line `Edition resolved edition=enterprise` confirms it.
`GET /api/admin/license` reports the edition, source, cap and expiry.

Push mode works the same way: choose it on the same page, or set
`NOSDESK_PUSH_MODE` to pin it.

## Rotating the key

Add the new PEM to `backend/license_keys/` and to the trusted key lists in
`backend/src/license.rs` beside the old one, and release; tokens pick their
key by `kid`. Reissue current licenses from the control plane, then drop the
old key in a later release. A unit test fails if a PEM in the directory is
not embedded.
