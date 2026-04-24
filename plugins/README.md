# Plugins

Drop signed plugin `.zip` files in this directory to provision them
on backend startup. Each zip must contain a valid
`nosdesk-signature.json` envelope and the signer pubkey must resolve
against one of the trust roots the instance recognises:

- the baked-in Nosdesk root pubkey (set at build time via
  `NOSDESK_ROOT_PUBKEY`)
- a non-revoked entry in `plugin_trusted_publishers`
- the instance's `plugin_local_signing_key` (auto-generated on first
  boot, fingerprint shown in startup logs)

Producing a signed zip
----------------------

Use the `nosdesk-plugin` CLI from your plugin source directory:

    nosdesk-plugin sign --in ./my-plugin --out ./my-plugin-1.0.0.zip \
        --key ~/.nosdesk/signing.key

Or for local development (debug builds only) set
`NOSDESK_DEV_MODE=1` in `docker.env` and use
`nosdesk-plugin sign --dev` to produce an unsigned zip.

Zip layout
----------

    my-plugin-1.0.0.zip
    ├── manifest.json            (required)
    ├── bundle.js                (optional — the frontend module)
    └── nosdesk-signature.json   (signing envelope, required in prod)

The zip file's name does not have to match the plugin name; the
plugin identity comes from `manifest.json`. Re-uploading a zip with
the same plugin name and a different version upserts the row and
refreshes signer provenance.
