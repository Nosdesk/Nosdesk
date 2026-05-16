//! Shared install pipeline for already-verified plugin archives.
//!
//! Both the HTTP upload handler and the filesystem provisioner
//! verify signatures and resolve trust chains themselves, then pass
//! the verified bytes plus resolved signer fields into this core.
//! Keeping the upsert + bundle-staging + settings + activity-log
//! sequence in one place ensures every entry point lands the same
//! row shape, stages bundles the same way, and logs the same audit
//! trail.

use chrono::Utc;
use ring::digest::{Context, SHA256};
use tracing::{debug, error, warn};
use uuid::Uuid;

use crate::db::DbConnection;
use crate::models::{NewPlugin, Plugin, PluginBundleUpdate, PluginManifest, PluginUpdate};
use crate::repository::plugin_publishers;
use crate::repository::plugins as plugin_repo;
use crate::services::plugins::{manifest_validate, signing, svg_validate, trust, validation};

/// Cap on the staged bundle.js size. The outer archive cap is
/// enforced by `signing::MAX_ENTRY_SIZE` at verification time; this
/// is a tighter practical limit specifically for plugin JS bundles.
pub const MAX_BUNDLE_SIZE: usize = 500 * 1024;

/// Capability token proving "this plugin row is being inserted as
/// part of the verified install pipeline." `plugin_repo::create_plugin`
/// requires one to insert a row, so any code path that wants to
/// create a plugin row must have a route to construct a token, and
/// the only way to do that in production is through the install
/// pipeline (the `new()` constructor is private to this module).
///
/// `#[non_exhaustive]` blocks struct-literal construction from
/// outside this crate even if a future field-visibility loosening
/// would otherwise expose the inner unit. Do NOT add `Default`,
/// `Clone`, or `Copy` derives to this type: each would be a
/// public constructor.
///
/// Tests construct tokens via `for_test()`, gated by `#[cfg(test)]`.
#[non_exhaustive]
pub struct InstallToken;

impl InstallToken {
    fn new() -> Self {
        Self
    }

    #[cfg(test)]
    pub fn for_test() -> Self {
        Self
    }
}

/// Caller policy knobs. The install core is entry-point-agnostic;
/// these fields describe what an install means from the invoking
/// path's perspective.
#[derive(Debug, Clone, Copy)]
pub struct InstallOptions {
    /// Stamped into `plugins.source`. Use `"uploaded"` for HTTP
    /// admin installs, `"provisioned"` for filesystem installs.
    pub source: &'static str,
    /// Admin UUID for audit; `None` for system-initiated installs.
    pub installed_by: Option<Uuid>,
    /// Emit a `plugin_activity` row describing the install/update.
    /// Provisioned installs skip this since they aren't user actions.
    pub log_activity: bool,
    /// Walk manifest settings and pull values from
    /// `PLUGIN_<NAME>_<KEY>` env vars. Only meaningful for
    /// provisioned installs where env is the secret surface.
    pub provision_settings: bool,
    /// If the DB already has the plugin at the same version with a
    /// matching staged bundle hash, return `Unchanged` without
    /// touching any rows. The provisioner runs on every boot and
    /// wants this; the HTTP handler always upserts.
    pub skip_if_unchanged: bool,
}

#[derive(Debug)]
pub enum InstallOutcome {
    Created(Plugin),
    Updated(Plugin),
    Unchanged(Plugin),
}

impl InstallOutcome {
    pub fn plugin(&self) -> &Plugin {
        match self {
            Self::Created(p) | Self::Updated(p) | Self::Unchanged(p) => p,
        }
    }
}

#[derive(Debug)]
pub enum InstallError {
    MissingManifest,
    InvalidManifest(String),
    BundleTooLarge(usize),
    BundleWriteFailed(String),
    InvalidIcon(svg_validate::SvgValidationError),
    /// One or more manifest schema rules violated (unknown
    /// permission, wrong `manifest_version`, author / publisher
    /// mismatch, etc.). Carries every accumulated error from the
    /// validator so install UX can surface them all at once;
    /// distinct from `InvalidManifest` (which is parse-level) so
    /// the API response can give precise reasons.
    InvalidManifestSchema(Vec<manifest_validate::ManifestValidationError>),
    /// Reinstall over an `Uninstalled`-with-preserve row was
    /// attempted with a signer pubkey that doesn't match the one
    /// captured at the original install. Closes the cross-publisher
    /// data inheritance bypass: the preserved plugin_data belongs
    /// to the original signer, so resurrecting under a different
    /// publisher would silently inherit it.
    ReinstallSignerMismatch {
        existing_fingerprint: String,
        attempted_fingerprint: String,
    },
    /// Plugin-owned collection schema migration failed (manifest
    /// declared a column we couldn't add, schema_version diverged
    /// from DB shape, etc.). Previously this was warn-and-continue,
    /// which left the row marked Installed and the bundle serving
    /// against a half-migrated schema, so plugin reads/writes
    /// blew up at runtime. Now hard-fails the install.
    CollectionSchemaSync(String),
    /// Refused to update a row whose state is `Quarantined`.
    /// Quarantine is an explicit "do not touch" marker; reinstall
    /// is the wrong tool to clear it. Operators must restore the
    /// plugin via the lifecycle action so the activity log records
    /// who decided the trust failure was resolved, and so the
    /// system doesn't silently overwrite the trust info that
    /// triggered the quarantine in the first place.
    RefusedQuarantined,
    Db(diesel::result::Error),
}

impl From<Vec<manifest_validate::ManifestValidationError>> for InstallError {
    fn from(value: Vec<manifest_validate::ManifestValidationError>) -> Self {
        InstallError::InvalidManifestSchema(value)
    }
}

impl std::fmt::Display for InstallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingManifest => write!(f, "zip does not contain manifest.json"),
            Self::InvalidManifest(m) => write!(f, "invalid manifest.json: {m}"),
            Self::BundleTooLarge(sz) => {
                write!(f, "bundle is {sz} bytes, exceeds {MAX_BUNDLE_SIZE}")
            }
            Self::BundleWriteFailed(m) => write!(f, "failed to stage bundle: {m}"),
            Self::InvalidIcon(e) => write!(f, "icon.svg rejected: {e}"),
            Self::InvalidManifestSchema(errs) => {
                write!(f, "manifest rejected: ")?;
                let mut first = true;
                for e in errs {
                    if !first {
                        write!(f, "; ")?;
                    }
                    write!(f, "{e}")?;
                    first = false;
                }
                Ok(())
            }
            Self::CollectionSchemaSync(m) => write!(f, "collection schema sync failed: {m}"),
            Self::RefusedQuarantined => write!(
                f,
                "plugin is quarantined; restore it via the lifecycle action before reinstalling"
            ),
            Self::ReinstallSignerMismatch {
                existing_fingerprint,
                attempted_fingerprint,
            } => write!(
                f,
                "reinstall refused: this plugin name was previously installed by signer {existing_fingerprint}; attempted reinstall is signed by {attempted_fingerprint}. Hard-uninstall (cascade) the existing row before reinstalling under a different publisher."
            ),
            // Categorise without leaking SQL detail. The full
            // Diesel error is on the operator's side via the
            // `error!` log emitted by `From<diesel::Error>`; the
            // category here helps a UI distinguish "retry might
            // work" (deadlock, serialization) from "fix your
            // request" (constraint violation) from "something is
            // wrong with the server" (connection pool, query
            // builder).
            Self::Db(e) => {
                use diesel::result::{DatabaseErrorKind, Error as DE};
                match e {
                    DE::NotFound => write!(f, "not found"),
                    DE::DatabaseError(DatabaseErrorKind::UniqueViolation, _) => {
                        write!(f, "duplicate value violates a unique constraint")
                    }
                    DE::DatabaseError(DatabaseErrorKind::ForeignKeyViolation, _) => {
                        write!(f, "foreign key constraint violated")
                    }
                    DE::DatabaseError(DatabaseErrorKind::CheckViolation, _) => {
                        write!(f, "check constraint violated")
                    }
                    DE::DatabaseError(DatabaseErrorKind::SerializationFailure, _) => {
                        write!(f, "serialization failure (transaction may succeed if retried)")
                    }
                    DE::DatabaseError(DatabaseErrorKind::ReadOnlyTransaction, _) => {
                        write!(f, "read-only transaction")
                    }
                    DE::DatabaseError(_, _) => write!(f, "database constraint error"),
                    DE::QueryBuilderError(_) => write!(f, "query construction error"),
                    DE::DeserializationError(_) => write!(f, "row deserialisation error"),
                    DE::SerializationError(_) => write!(f, "value serialisation error"),
                    DE::RollbackTransaction => write!(f, "transaction rolled back"),
                    DE::AlreadyInTransaction | DE::NotInTransaction => {
                        write!(f, "transaction state error")
                    }
                    _ => write!(f, "database error"),
                }
            }
        }
    }
}

impl std::error::Error for InstallError {}

impl From<diesel::result::Error> for InstallError {
    fn from(value: diesel::result::Error) -> Self {
        error!(error = %value, "Plugin install DB error");
        InstallError::Db(value)
    }
}

/// Run a verified archive through the upsert + bundle-staging
/// pipeline. Signature verification + trust resolution are the
/// caller's job; by the time the bytes get here, they've been
/// vouched for.
///
/// `tier` is the resolved trust tier from `trust::resolve`. It's
/// load-bearing because the manifest validator uses it for author
/// binding (official → "Nosdesk", verified/community → matches
/// publishers.json display name, local → skipped).
pub fn install_verified(
    conn: &mut DbConnection,
    files: &[signing::ArchiveEntry],
    signer: trust::PluginSignerFields,
    tier: trust::ResolvedTier,
    options: InstallOptions,
) -> Result<InstallOutcome, InstallError> {
    let manifest_bytes =
        signing::find_entry(files, "manifest.json").ok_or(InstallError::MissingManifest)?;
    let manifest: PluginManifest = serde_json::from_slice(manifest_bytes)
        .map_err(|e| InstallError::InvalidManifest(e.to_string()))?;

    // Plugin name is validated as part of manifest_validate::validate
    // below; no need for a separate first-pass check.

    // Manifest schema check. Looks up the publisher's display name
    // when needed for author binding; `find_publisher_by_pubkey`
    // returns `Ok(None)` for tiers that don't have a publisher row
    // (official, local), and validate() handles those branches.
    let publisher_display_name: Option<String> = match tier {
        trust::ResolvedTier::Verified | trust::ResolvedTier::Community => signer
            .signer_pubkey
            .as_ref()
            .map(|pk| plugin_publishers::find_publisher_by_pubkey(conn, pk))
            .transpose()?
            .flatten()
            .map(|p| p.display_name),
        _ => None,
    };
    manifest_validate::validate(
        &manifest,
        &manifest_validate::ValidationContext {
            tier: &tier,
            publisher_display_name: publisher_display_name.as_deref(),
            nosdesk_version: env!("CARGO_PKG_VERSION"),
        },
    )?;

    let bundle_bytes = signing::find_entry(files, "bundle.js");
    if let Some(b) = bundle_bytes {
        if b.len() > MAX_BUNDLE_SIZE {
            return Err(InstallError::BundleTooLarge(b.len()));
        }
    }

    // Optional icon. Validated here so the same rules apply on
    // both the registry-install path and the zip-upload path.
    let icon_bytes: Option<Vec<u8>> = match signing::find_entry(files, "icon.svg") {
        Some(bytes) => {
            svg_validate::validate(bytes).map_err(InstallError::InvalidIcon)?;
            Some(bytes.to_vec())
        }
        None => None,
    };

    let manifest_json = serde_json::to_value(&manifest)
        .map_err(|e| InstallError::InvalidManifest(e.to_string()))?;

    // Every DB write below happens inside one transaction so the
    // install is all-or-nothing: row insert/update, bundle bytes
    // (now inline as BYTEA), collection schema sync, settings
    // provisioning, and the activity log either all commit or
    // all roll back. No half-applied installs, no orphan files.
    use diesel::Connection;
    conn.transaction::<_, InstallError, _>(|tx| {
        // Postgres advisory lock keyed on the plugin name. Two
        // concurrent installs of the same plugin (e.g. an admin
        // upload racing the registry installer) serialize on this
        // lock; concurrent installs of different names proceed in
        // parallel. The lock releases on transaction commit or
        // rollback automatically.
        use diesel::sql_query;
        use diesel::sql_types::Text;
        use diesel::RunQueryDsl;
        sql_query("SELECT pg_advisory_xact_lock(hashtext($1))")
            .bind::<Text, _>(&manifest.name)
            .execute(tx)?;

        let existing = plugin_repo::get_plugin_by_name(tx, &manifest.name).ok();

        let outcome = match existing {
            Some(existing) if options.skip_if_unchanged => {
                // No-op gate: every field that defines the install
                // identity must match. If a publisher re-signs with
                // a new key (different signer_pubkey) or edits the
                // manifest body without bumping version, we still
                // want to apply the update.
                let version_same = matches!(
                    existing.parse_manifest(),
                    Ok(em) if em.version == manifest.version
                );
                let bundle_same = !bundle_hash_differs(&existing, bundle_bytes);
                let icon_same = existing.icon_svg.as_deref() == icon_bytes.as_deref();
                let manifest_same = existing.manifest == manifest_json;
                let signer_same = existing.signer_pubkey == signer.signer_pubkey;
                if version_same && bundle_same && icon_same && manifest_same && signer_same {
                    if options.provision_settings {
                        provision_settings_from_env(tx, &existing, &manifest);
                    }
                    return Ok(InstallOutcome::Unchanged(existing));
                }
                InstallOutcome::Updated(update_row(
                    tx,
                    existing,
                    &manifest,
                    manifest_json,
                    &signer,
                    &icon_bytes,
                    options.installed_by,
                )?)
            }
            Some(existing) => InstallOutcome::Updated(update_row(
                tx,
                existing,
                &manifest,
                manifest_json,
                &signer,
                &icon_bytes,
                options.installed_by,
            )?),
            None => InstallOutcome::Created(create_row(
                tx,
                &manifest,
                manifest_json,
                signer.clone(),
                &options,
                icon_bytes.clone(),
            )?),
        };

        let plugin = outcome.plugin();

        if let Some(bytes) = bundle_bytes {
            stage_bundle(tx, plugin, bytes)?;
        }

        if !manifest.collections.is_empty() {
            // Hard-fail on schema sync errors. The previous warn-
            // and-continue behaviour left the row marked Installed
            // with a half-applied schema, so subsequent reads/writes
            // from the plugin blew up at runtime against a column
            // that didn't exist. Surface the failure at install
            // time so the operator can investigate before the
            // plugin starts serving requests.
            validation::sync_collection_schemas(tx, plugin.id, &manifest)
                .map_err(InstallError::CollectionSchemaSync)?;
        }

        if options.provision_settings {
            provision_settings_from_env(tx, plugin, &manifest);
        }

        if options.log_activity {
            // Activity log is inside the same transaction as the
            // state change. A logging failure rolls the install
            // back; an install with no audit trail would be the
            // exact case auditors want loud.
            let action = match &outcome {
                InstallOutcome::Created(_) => "installed",
                InstallOutcome::Updated(_) => "updated",
                InstallOutcome::Unchanged(_) => "unchanged",
            };
            plugin_repo::log_plugin_activity(
                tx,
                plugin.id,
                action.to_string(),
                Some(serde_json::json!({
                    "version": manifest.version,
                    "source": options.source,
                    "has_bundle": bundle_bytes.is_some(),
                    "signer_source": signer.signer_source,
                    "trust_level": signer.trust_level,
                })),
                options.installed_by,
            )?;
        }

        Ok(outcome)
    })
}

fn update_row(
    conn: &mut DbConnection,
    existing: Plugin,
    manifest: &PluginManifest,
    manifest_json: serde_json::Value,
    signer: &trust::PluginSignerFields,
    icon_bytes: &Option<Vec<u8>>,
    installed_by: Option<Uuid>,
) -> Result<Plugin, InstallError> {
    // Re-installing a previously uninstalled plugin is a state
    // transition (`Uninstalled -> Installed`); route it through
    // the lifecycle module so signer continuity is enforced and
    // the activity log is written atomically. The lifecycle
    // module refuses the reinstall if the new signer's pubkey
    // doesn't match the one captured at the original install,
    // which is the architectural fix for the cross-publisher
    // data-inheritance bypass.
    if existing.state == crate::models::PluginState::Quarantined {
        return Err(InstallError::RefusedQuarantined);
    }

    if existing.state == crate::models::PluginState::Uninstalled {
        use crate::services::plugins::lifecycle::{apply, ActionError, PluginAction};
        let action = PluginAction::Reinstall {
            signer_pubkey: signer.signer_pubkey.clone(),
        };
        match apply(conn, existing.uuid, action, installed_by) {
            Ok(_) => {}
            Err(ActionError::SignerMismatch {
                existing_fingerprint,
                attempted_fingerprint,
            }) => {
                return Err(InstallError::ReinstallSignerMismatch {
                    existing_fingerprint,
                    attempted_fingerprint,
                });
            }
            Err(ActionError::Db(e)) => return Err(InstallError::Db(e)),
            Err(ActionError::NoSuchPlugin) => {
                return Err(InstallError::Db(diesel::result::Error::NotFound));
            }
            Err(e @ ActionError::InvalidTransition { .. }) => {
                // Should be unreachable: we just checked the state
                // is `Uninstalled` and lifecycle accepts that for
                // Reinstall. Treat as a programming error.
                return Err(InstallError::InvalidManifest(format!(
                    "lifecycle refused reinstall: {e}"
                )));
            }
        }
    }

    // Tri-state icon write:
    //   None             -> leave the column alone (icon unchanged)
    //   Some(Some(bytes)) -> write new icon bytes
    //   Some(None)       -> clear the column (previous version had
    //                       an icon, this one doesn't)
    // Avoids re-writing the BYTEA column on every reinstall when
    // the icon is unchanged.
    let icon_update = if existing.icon_svg.as_deref() == icon_bytes.as_deref() {
        None
    } else {
        Some(icon_bytes.clone())
    };

    let update = PluginUpdate {
        display_name: Some(manifest.display_name.clone()),
        version: Some(manifest.version.clone()),
        description: manifest.description.clone(),
        manifest: Some(manifest_json),
        // State changes flow through `lifecycle::apply` (above);
        // never write `state` directly from the install path.
        state: None,
        trust_level: Some(signer.trust_level.clone()),
        signer_pubkey: signer.signer_pubkey.clone(),
        signer_source: signer.signer_source.clone(),
        signature_metadata: signer.signature_metadata.clone(),
        icon_svg: icon_update,
    };
    Ok(plugin_repo::update_plugin_by_uuid(
        conn,
        existing.uuid,
        update,
    )?)
}

fn create_row(
    conn: &mut DbConnection,
    manifest: &PluginManifest,
    manifest_json: serde_json::Value,
    signer: trust::PluginSignerFields,
    options: &InstallOptions,
    icon_bytes: Option<Vec<u8>>,
) -> Result<Plugin, InstallError> {
    let new_plugin = NewPlugin {
        name: manifest.name.clone(),
        display_name: manifest.display_name.clone(),
        version: manifest.version.clone(),
        description: manifest.description.clone(),
        manifest: manifest_json,
        state: crate::models::PluginState::Installed,
        trust_level: signer.trust_level,
        installed_by: options.installed_by,
        source: options.source.to_string(),
        signer_pubkey: signer.signer_pubkey,
        signer_source: signer.signer_source,
        signature_metadata: signer.signature_metadata,
        icon_svg: icon_bytes,
    };
    // The install pipeline is the only production constructor of
    // `InstallToken`, so this call is the unique entry point for
    // new plugin rows in the codebase.
    Ok(plugin_repo::create_plugin(
        conn,
        new_plugin,
        InstallToken::new(),
    )?)
}

/// Persist pre-verified bundle bytes inline on the plugin row.
/// Replaces the previous on-disk staging: `bundle_js` BYTEA, plus
/// the denormalised `bundle_hash` / `bundle_size` /
/// `bundle_uploaded_at` metadata, all written in one transactional
/// row update. Atomicity for free; no torn writes between two
/// backing stores; no orphan files to garbage-collect on uninstall.
fn stage_bundle(
    conn: &mut DbConnection,
    plugin: &Plugin,
    content: &[u8],
) -> Result<(), InstallError> {
    let mut ctx = Context::new(&SHA256);
    ctx.update(content);
    let hash = hex::encode(ctx.finish().as_ref());

    let update = PluginBundleUpdate {
        bundle_js: Some(content.to_vec()),
        bundle_hash: Some(hash),
        bundle_size: Some(content.len() as i32),
        bundle_uploaded_at: Some(Utc::now().naive_utc()),
    };
    plugin_repo::update_plugin_bundle(conn, plugin.uuid, update)?;

    debug!("Staged bundle for plugin: {}", plugin.name);
    Ok(())
}

/// True when new bundle bytes differ from what the `plugins` row
/// records OR the staged copy is missing (stale row after a backup
/// restore). Applied by both entry points so the no-op fast
/// path doesn't skip a re-stage when the row's bytes are stale
/// or absent.
fn bundle_hash_differs(plugin: &Plugin, new_bytes: Option<&[u8]>) -> bool {
    let Some(new_bytes) = new_bytes else {
        return false;
    };
    let mut ctx = Context::new(&SHA256);
    ctx.update(new_bytes);
    let new_hash = hex::encode(ctx.finish().as_ref());

    plugin.bundle_hash.as_ref() != Some(&new_hash) || plugin.bundle_js.is_none()
}

// Plugin name validation lives in `manifest_validate::validate_name`
// and is invoked as part of the schema check above. Keeping the
// rules in one place means HTTP / registry / CLI / provisioning
// installs can't drift apart.

/// Provision plugin settings from environment variables using the
/// pattern `PLUGIN_{PLUGIN_NAME}_{SETTING_KEY}=value`. Plugin name
/// and setting key are uppercased with hyphens converted to
/// underscores. Secret settings are encrypted before storage.
fn provision_settings_from_env(
    conn: &mut DbConnection,
    plugin: &Plugin,
    manifest: &PluginManifest,
) {
    use crate::utils::encryption;

    let env_prefix = format!("PLUGIN_{}_", plugin.name.to_uppercase().replace('-', "_"));
    let mut settings_count = 0;

    for setting_def in &manifest.settings {
        let env_key = format!(
            "{}{}",
            env_prefix,
            setting_def.key.to_uppercase().replace('-', "_")
        );
        let Ok(value) = std::env::var(&env_key) else {
            continue;
        };

        let is_secret = setting_def.setting_type == "secret";
        let json_value = if is_secret {
            match encryption::encrypt(&value) {
                Ok(encrypted) => serde_json::Value::String(encrypted),
                Err(e) => {
                    warn!(
                        "Failed to encrypt secret setting {} for plugin {}: {}. Ensure ENCRYPTION_KEY is set.",
                        setting_def.key, plugin.name, e
                    );
                    continue;
                }
            }
        } else {
            serde_json::Value::String(value)
        };

        match plugin_repo::set_plugin_setting(
            conn,
            plugin.id,
            setting_def.key.clone(),
            Some(json_value),
            is_secret,
        ) {
            Ok(_) => {
                settings_count += 1;
                debug!(
                    "Provisioned {}setting from env: {} -> {}",
                    if is_secret { "secret " } else { "" },
                    env_key,
                    setting_def.key
                );
            }
            Err(e) => {
                warn!(
                    "Failed to provision setting {} for plugin {}: {}",
                    setting_def.key, plugin.name, e
                );
            }
        }
    }

    if settings_count > 0 {
        tracing::info!(
            "Provisioned {} settings from environment for plugin: {}",
            settings_count,
            plugin.name
        );
    }
}
