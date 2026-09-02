# Licensing

Self-hosted Nosdesk runs the **Community** edition, which is limited to a
single workspace. Creating additional workspaces requires an **Enterprise**
license. Hosted (SaaS) deployments provision workspaces through the control
plane and are not affected.

The cap is enforced server-side in `admin_workspaces::create_workspace`:
on self-hosted deployments, once the active-workspace count reaches the
edition limit, `POST /api/admin/workspaces` returns `402 license_required`.

## How a license works

A license is an EdDSA-signed JWT carrying a stable customer id (`sub`, a
UUID that survives reissues), a display name (`licensee`), a per-issuance
id (`jti`, required), an active-workspace cap, an expiry, and an optional
`features` list (empty in v1.1 — nothing new is gated on it). The public
verification key is compiled into the binary; the private signing key is
held only by Nosdesk. The server reads `NOSDESK_LICENSE_KEY` at boot,
verifies it, and lifts the workspace cap to the license's `max_workspaces`.
An absent, malformed, expired, or wrong-issuer license falls back to
Community without failing startup.

Like any open-source gate, this is bypassable by patching the binary. The
license is a genuine signed artifact, not an honor-system flag.

## Minting a license (Nosdesk operators)

Generate the signing keypair once. Keep the private key offline; commit and
embed the public key (`backend/license_pubkey.pem`).

```bash
openssl genpkey -algorithm ed25519 -out license_private.pem
openssl pkey -in license_private.pem -pubout -out backend/license_pubkey.pem
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

Set the token in the backend environment and restart:

```bash
NOSDESK_LICENSE_KEY=nsk_lic_...
```

The startup log line `Edition resolved edition=enterprise` confirms it
verified. `GET /api/admin/edition` reports the active edition, cap,
customer id, features, and current workspace count.

## Rotating the key

Regenerate with the `openssl` commands above, replace
`backend/license_pubkey.pem`, and recompile. Existing licenses signed with the
old key stop verifying, so reissue them.
