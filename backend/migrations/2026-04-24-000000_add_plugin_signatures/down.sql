DROP TABLE IF EXISTS plugin_local_signing_key;
DROP TABLE IF EXISTS plugin_trusted_publishers;

ALTER TABLE plugins
    DROP COLUMN IF EXISTS signature_metadata,
    DROP COLUMN IF EXISTS signer_source,
    DROP COLUMN IF EXISTS signer_pubkey;
