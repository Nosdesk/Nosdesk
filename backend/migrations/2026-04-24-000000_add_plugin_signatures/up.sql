-- Plugin signing and trust-chain schema.
--
-- Every plugin install now carries an Ed25519 signature. The new
-- columns on `plugins` capture which key signed this bundle and what
-- authority chain it resolved through (Nosdesk root key, a registered
-- third-party publisher key, or the instance's own local signing
-- key). The two new tables hold the publisher keylist (populated from
-- the signed registry artifact on nosdesk.com) and the instance's
-- single local signing key (at-rest encrypted via the same path that
-- encrypts MFA secrets).

ALTER TABLE plugins
    ADD COLUMN signer_pubkey TEXT,
    ADD COLUMN signer_source VARCHAR(32),
    ADD COLUMN signature_metadata JSONB;

-- Publishers whose Ed25519 pubkeys the instance trusts, populated
-- from the signed `publishers.json` artifact at nosdesk.com. A
-- publisher that gets removed from the upstream keylist is marked
-- `revoked_at` rather than deleted so historical installs still
-- attribute the correct publisher.
CREATE TABLE plugin_trusted_publishers (
    id              SERIAL PRIMARY KEY,
    pubkey          TEXT NOT NULL UNIQUE,
    display_name    VARCHAR(200) NOT NULL,
    tier            VARCHAR(32) NOT NULL CHECK (tier IN ('verified', 'community')),
    website         TEXT,
    added_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    revoked_at      TIMESTAMPTZ
);

CREATE INDEX idx_plugin_trusted_publishers_pubkey ON plugin_trusted_publishers(pubkey);

-- Single-row table holding this instance's local signing keypair.
-- The private half is encrypted with the MFA encryption key. Used
-- to sign plugins installed through the CLI only; no web path writes
-- signatures with this key.
CREATE TABLE plugin_local_signing_key (
    id              INTEGER PRIMARY KEY CHECK (id = 1),
    pubkey          TEXT NOT NULL,
    encrypted_sk    BYTEA NOT NULL,
    fingerprint     VARCHAR(64) NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

COMMENT ON COLUMN plugins.signer_pubkey IS
    'Base64 Ed25519 public key that signed this bundle, or NULL for dev-mode unsigned installs.';
COMMENT ON COLUMN plugins.signer_source IS
    'Which authority chain recognised this signer: nosdesk-root | verified-publisher | community-publisher | local | dev.';
COMMENT ON COLUMN plugins.signature_metadata IS
    'Full signature envelope captured at install time for audit.';
