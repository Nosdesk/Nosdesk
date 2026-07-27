//! Manifest schema enforcement.
//!
//! Runs after the plugin's signature has been verified and the
//! signer resolved against the trust chain. Refuses any manifest
//! that doesn't match the v1 schema rules: unknown permissions,
//! unknown event names, unknown slot identifiers, unknown setting
//! types, unsupported `manifest_version`, unsatisfied `engines`
//! constraints, and (for non-official plugins) author / publisher
//! mismatches.
//!
//! Allowlists are central here on purpose. Adding a new permission
//! is one line. Renaming an existing one is a breaking change for
//! any plugin in the wild and should be paired with a deprecation
//! window. The trust chain plus this allowlist together let the
//! manifest evolve safely without ambiguity.

use crate::models::{
    PluginComponentConfig, PluginComponentKind, PluginManifest, PluginSettingDefinition,
};
use crate::services::plugins::trust::ResolvedTier;

/// Highest manifest schema version this binary understands.
/// Plugins declaring a higher value get refused with a clear
/// upgrade-required message rather than producing surprising
/// behaviour from a half-parsed unknown shape.
pub const SUPPORTED_MANIFEST_VERSION: u32 = 1;

/// Plugin runtime API major version exposed to plugin code via
/// `api.version` on the JS side. Plugins declaring
/// `engines.plugin_api` MUST equal this exactly. Bumping is a
/// breaking-change signal: every plugin must opt in to v2.
pub const SUPPORTED_PLUGIN_API_VERSION: &str = "1";

// Permission allowlist: the orchestrating doc previously sat above
// a `&[&str]` constant that we replaced with the typed `Permission`
// enum in `crate::services::plugins::types`. Unknown permission
// strings fail to deserialise into the enum, so they never reach
// the validator. The list of recognised capabilities lives there;
// adding a new permission means extending that enum plus the
// matching handler enforcement, not editing a list here.

// Slot identifiers come from the single source of truth in
// `packages/core/src/types/pluginSlots.ts`, generated into
// `plugin_slots.generated.json` and read via `slot_registry`. A plugin may
// target any canonical name or legacy alias in the registry; a slot with no
// live mount point yet (`status: "reserved"`) is a silent no-op until one
// lands (the VS Code contribution-point model). Adding a slot is a one-place
// edit in the TS registry followed by `build:slots`.

/// Context types a component can request. The runtime passes the
/// matching object on the `context` prop.
pub(crate) const KNOWN_CONTEXTS: &[&str] = &[
    "ticket",
    // Delivered to `asset-*` slots as `context.asset`.
    "asset",
    // Reserved:
    "user",
    "comment",
    "documentation_page",
];

/// Events plugins can subscribe to. Canonical taxonomy mirrored
/// from `frontend/src/types/plugin.ts::PLUGIN_EVENTS`.
///
/// Subscribing to an event whose dispatch site doesn't yet exist
/// is a silent no-op (handler never fires), the same pub/sub
/// "loose subscription" pattern industry plugin systems use.
/// New events are added by extending the dispatcher's SSE map or
/// `TICKET_FIELD_TO_EVENT` table on the frontend, then mirroring
/// the literal here.
pub(crate) const KNOWN_EVENTS: &[&str] = &[
    "ticket:created",
    "ticket:updated",
    "ticket:status_changed",
    "ticket:assigned",
    "ticket:comment_added",
    "document:created",
    "document:updated",
    "asset:created",
    "asset:updated",
];

/// Setting types accepted on `settings[].type`. Each maps to a
/// frontend renderer + a backend storage policy (notably:
/// `secret` is encrypted at rest).
pub(crate) const KNOWN_SETTING_TYPES: &[&str] = &[
    "string",
    "number",
    "boolean",
    "secret",
    "select",
    // Reserved for future use; settings UI will need to render
    // them, but reserving the names here means a forward-looking
    // plugin can declare them now and we just refuse install
    // until the UI catches up.
    "multiline_string",
    "select_multi",
    "json",
    "url",
    "date",
];

/// Categories the registry browse UI groups by. Plugins declaring
/// unknown categories are refused; this keeps the taxonomy curated
/// rather than letting it drift into a long tail of one-off names.
pub(crate) const KNOWN_CATEGORIES: &[&str] = &[
    "integrations",
    "automation",
    "analytics",
    "communication",
    "developer-tools",
    "productivity",
    "security",
    "ui",
];

#[derive(Debug)]
pub enum ManifestValidationError {
    UnsupportedManifestVersion(u32),
    InvalidName(String),
    InvalidEvent(String),
    InvalidSlot(String),
    InvalidContext(String),
    InvalidComponentKind(String),
    UnsupportedComponentKind(String),
    InvalidSettingType(String),
    InvalidCategory(String),
    InvalidScreenshotPath(String),
    UnsupportedCollectionSchemaVersion {
        collection: String,
        version: u32,
    },
    EngineNotSatisfied {
        kind: &'static str,
        requirement: String,
        current: String,
    },
    /// Distinct from `EngineNotSatisfied` so error UX can tell a
    /// plugin author "your version constraint is malformed" from
    /// "your constraint doesn't match this Nosdesk version".
    InvalidEngineRequirement {
        kind: &'static str,
        requirement: String,
    },
    PluginApiMismatch {
        requested: String,
        supported: &'static str,
    },
    AuthorMismatch {
        manifest_author: String,
        publisher_display_name: String,
    },
    AuthorRequired,
    AuthRefersToUndeclaredHost(String),
    InvalidDependencyVersion {
        plugin: String,
        requirement: String,
    },
    UnsupportedSettingScope {
        key: String,
    },
    /// `commands` / `menus` / `url_handlers` / `extensions` arrays
    /// are reserved manifest keys; they parse but the runtime
    /// hasn't shipped yet, so non-empty values are refused.
    ReservedFieldNotEmpty {
        field: &'static str,
    },
    /// A localizable field uses `%key%` but the key isn't defined in
    /// `i18n["en-US"]`, so it has no fallback and would render as the literal
    /// `%key%`.
    UnresolvedI18nKey {
        location: &'static str,
        key: String,
    },
    /// An `i18n` string table has an empty value.
    EmptyI18nValue {
        locale: String,
        key: String,
    },
    InvalidContactUrl(String),
    InvalidBugsUrl(String),
}

impl std::fmt::Display for ManifestValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedManifestVersion(v) => write!(
                f,
                "manifest_version {v} is not supported by this Nosdesk build (expected {SUPPORTED_MANIFEST_VERSION})"
            ),
            Self::InvalidName(n) => write!(f, "plugin name {n:?} is invalid"),
            Self::InvalidEvent(e) => write!(f, "unknown event {e:?}"),
            Self::InvalidSlot(s) => write!(f, "unknown slot {s:?}"),
            Self::InvalidContext(c) => write!(f, "unknown component context {c:?}"),
            Self::InvalidComponentKind(k) => write!(f, "invalid component kind {k:?}"),
            Self::UnsupportedComponentKind(k) => write!(
                f,
                "component kind {k:?} is reserved but not yet implemented in this Nosdesk version"
            ),
            Self::InvalidSettingType(t) => write!(f, "unknown setting type {t:?}"),
            Self::InvalidCategory(c) => write!(f, "unknown category {c:?}"),
            Self::InvalidScreenshotPath(p) => {
                write!(f, "screenshot path {p:?} is invalid (must not contain `..` or absolute paths)")
            }
            Self::UnsupportedCollectionSchemaVersion { collection, version } => write!(
                f,
                "collection {collection:?} declares schema_version {version} but only 1 is supported"
            ),
            Self::EngineNotSatisfied { kind, requirement, current } => write!(
                f,
                "engines.{kind} requirement {requirement:?} not satisfied by current version {current:?}"
            ),
            Self::InvalidEngineRequirement { kind, requirement } => write!(
                f,
                "engines.{kind} requirement {requirement:?} is not a valid semver expression"
            ),
            Self::PluginApiMismatch { requested, supported } => write!(
                f,
                "engines.plugin_api {requested:?} not supported (expected {supported:?})"
            ),
            Self::AuthorMismatch { manifest_author, publisher_display_name } => write!(
                f,
                "manifest.author {manifest_author:?} does not match registered publisher {publisher_display_name:?}"
            ),
            Self::AuthorRequired => write!(
                f,
                "manifest.author is required for non-local plugins (must match the registered publisher)"
            ),
            Self::AuthRefersToUndeclaredHost(h) => write!(
                f,
                "manifest.auth declares host {h:?} but no matching `network:{h}` permission is granted"
            ),
            Self::InvalidDependencyVersion { plugin, requirement } => write!(
                f,
                "dependencies[{plugin:?}] requirement {requirement:?} is not a valid semver range"
            ),
            Self::UnsupportedSettingScope { key } => write!(
                f,
                "settings[{key:?}] declares scope=user; per-user setting storage is reserved but not yet implemented in this Nosdesk version"
            ),
            Self::ReservedFieldNotEmpty { field } => write!(
                f,
                "manifest.{field} is a reserved field and is not yet honoured by this Nosdesk version; remove or set to empty"
            ),
            Self::UnresolvedI18nKey { location, key } => write!(
                f,
                "{location} uses %{key}% but i18n[\"en-US\"][\"{key}\"] is not defined; every localisation key must have an en-US fallback"
            ),
            Self::EmptyI18nValue { locale, key } => write!(
                f,
                "i18n[{locale:?}][{key:?}] is empty"
            ),
            Self::InvalidContactUrl(s) => write!(
                f,
                "manifest.support_contact {s:?} must be a URL or contain `@` (email)"
            ),
            Self::InvalidBugsUrl(s) => write!(f, "manifest.bugs {s:?} must be a URL"),
        }
    }
}

impl std::error::Error for ManifestValidationError {}

/// Context the validator needs that doesn't live on the manifest
/// itself: the trust tier, the publisher's display name (for
/// author binding), and the running Nosdesk version (for the
/// engines check). Built by the install pipeline after trust
/// resolution.
pub struct ValidationContext<'a> {
    pub tier: &'a ResolvedTier,
    /// `Some(name)` for verified/community publishers, `None` for
    /// official (root) and local installs.
    pub publisher_display_name: Option<&'a str>,
    /// e.g. `env!("CARGO_PKG_VERSION")` of the running backend.
    pub nosdesk_version: &'a str,
}

/// Run every check the v1 manifest schema requires.
///
/// Errors accumulate into a `Vec` so install UX can surface every
/// problem at once; an author fixing one issue doesn't need a
/// retry to learn about the next. The exception is
/// `manifest_version`: if it doesn't match this binary's schema,
/// none of the downstream checks would interpret correctly, so we
/// short-circuit. The same applies to the `engines.plugin_api`
/// constant compare inside `validate_engines`. Everything else is
/// independent and accumulates.
pub fn validate(
    manifest: &PluginManifest,
    ctx: &ValidationContext,
) -> Result<(), Vec<ManifestValidationError>> {
    // Hard gate: if the wrong schema version, every downstream
    // check is meaningless. Return a single-element Vec so callers
    // can treat the error type uniformly.
    if manifest.manifest_version != SUPPORTED_MANIFEST_VERSION {
        return Err(vec![ManifestValidationError::UnsupportedManifestVersion(
            manifest.manifest_version,
        )]);
    }

    let mut errors: Vec<ManifestValidationError> = Vec::new();

    if let Err(e) = validate_name(&manifest.name) {
        errors.push(e);
    }
    if let Err(e) = validate_engines(manifest, ctx) {
        errors.push(e);
    }
    if let Err(e) = validate_author(manifest, ctx) {
        errors.push(e);
    }

    // Permissions are now typed (`Vec<Permission>`); deserialisation
    // already rejected unknown strings and malformed `network:`
    // patterns before we got here. We just collect the network
    // patterns for the auth cross-check below.

    for event in &manifest.events {
        if !KNOWN_EVENTS.contains(&event.as_str()) {
            errors.push(ManifestValidationError::InvalidEvent(event.clone()));
        }
    }

    for component in manifest.components.values() {
        if let Err(e) = validate_component(component) {
            errors.push(e);
        }
    }

    for setting in &manifest.settings {
        if let Err(e) = validate_setting(setting) {
            errors.push(e);
        }
    }

    for category in &manifest.categories {
        if !KNOWN_CATEGORIES.contains(&category.as_str()) {
            errors.push(ManifestValidationError::InvalidCategory(category.clone()));
        }
    }

    for screenshot in &manifest.screenshots {
        if let Err(e) = validate_screenshot_path(screenshot) {
            errors.push(e);
        }
    }

    for (collection_name, def) in &manifest.collections {
        if def.schema_version != 1 {
            errors.push(
                ManifestValidationError::UnsupportedCollectionSchemaVersion {
                    collection: collection_name.clone(),
                    version: def.schema_version,
                },
            );
        }
    }

    // Auth cross-check: every declared auth host must be covered
    // by at least one `network:<pattern>` permission. `Host` and
    // `HostPattern` are pre-normalised (lowercase, syntactically
    // valid) so a string-byte mismatch can't sneak through.
    let network_patterns: Vec<&crate::services::plugins::types::HostPattern> = manifest
        .permissions
        .iter()
        .filter_map(crate::services::plugins::types::Permission::network_pattern)
        .collect();
    for host in manifest.auth.keys() {
        let auth_pattern = crate::services::plugins::types::HostPattern::Exact(host.clone());
        if !network_patterns.iter().any(|p| p.covers(&auth_pattern)) {
            errors.push(ManifestValidationError::AuthRefersToUndeclaredHost(
                host.as_str().to_string(),
            ));
        }
    }

    for (plugin_name, requirement) in &manifest.dependencies {
        if semver::VersionReq::parse(requirement.trim()).is_err() {
            errors.push(ManifestValidationError::InvalidDependencyVersion {
                plugin: plugin_name.clone(),
                requirement: requirement.clone(),
            });
        }
    }

    // URL-shaped fields. `bugs` must be a `WebUrl` (https-only).
    // `support_contact` accepts either a `WebUrl` or an email
    // (the at-sign is the cheap check; we don't validate RFC 5322
    // compliance). The https-only constraint closes a class of
    // dangerous-scheme links rendered in the registry UI
    // (`javascript:`, `file:`, `data:`).
    if let Some(bugs) = &manifest.bugs {
        if crate::services::plugins::types::WebUrl::parse(bugs).is_err() {
            errors.push(ManifestValidationError::InvalidBugsUrl(bugs.clone()));
        }
    }
    if let Some(contact) = &manifest.support_contact {
        let looks_like_url = crate::services::plugins::types::WebUrl::parse(contact).is_ok();
        let looks_like_email = contact.contains('@')
            && !contact.contains(' ')
            && !contact.starts_with('@')
            && !contact.ends_with('@');
        if !looks_like_url && !looks_like_email {
            errors.push(ManifestValidationError::InvalidContactUrl(contact.clone()));
        }
    }

    // Reserved fields: refuse non-empty values for every field
    // whose runtime hasn't shipped. Each entry pairs a field name
    // with `bool: is the field empty?`; one helper, one predicate,
    // no per-field "is empty" semantic drift. Adding a new
    // reserved field is one line in this list.
    let reserved: &[(&'static str, bool)] = &[
        ("commands", manifest.commands.is_empty()),
        ("menus", manifest.menus.is_empty()),
        ("url_handlers", manifest.url_handlers.is_empty()),
        ("extensions", manifest.extensions.is_empty()),
    ];
    for (field, empty) in reserved {
        if !empty {
            errors.push(ManifestValidationError::ReservedFieldNotEmpty { field });
        }
    }

    // Localisation: a surface-visible field may be `%key%`, resolved by the UI
    // against `manifest.i18n`. Every key used must have an en-US fallback, so it
    // always resolves to something.
    let fallback = manifest.i18n.get(FALLBACK_LOCALE);
    let mut check_l10n = |location: &'static str, value: &str| {
        if let Some(e) = check_localizable(fallback, location, value) {
            errors.push(e);
        }
    };
    check_l10n("displayName", &manifest.display_name);
    for setting in &manifest.settings {
        check_l10n("settings[].label", &setting.label);
        if let Some(d) = &setting.description {
            check_l10n("settings[].description", d);
        }
        if let Some(opts) = &setting.options {
            for opt in opts {
                check_l10n("settings[].options[].label", &opt.label);
            }
        }
    }
    for comp in manifest.components.values() {
        if let Some(l) = &comp.label {
            check_l10n("components[].label", l);
        }
        if let Some(a) = &comp.action {
            check_l10n("components[].action.label", &a.label);
        }
    }
    // Reject empty i18n values (a `%key%` resolving to "" is a silent bug).
    for (loc, table) in &manifest.i18n {
        for (k, v) in table {
            if v.trim().is_empty() {
                errors.push(ManifestValidationError::EmptyI18nValue {
                    locale: loc.clone(),
                    key: k.clone(),
                });
            }
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

/// Heuristic for the reserved localisation syntax `%key%`: leading
/// `%`, identifier characters (letters, digits, underscore, dot),
/// trailing `%`. Used to refuse manifest strings that would collide
/// with future i18n bundles.
/// The locale every `%key%` must be defined for, so a fallback always exists.
const FALLBACK_LOCALE: &str = "en-US";

/// If `s` is a `%key%` reference, return the inner key. Grammar: leading `%`,
/// non-empty inner of `[A-Za-z0-9_.]`, trailing `%`. The UI resolver matches this.
pub(crate) fn i18n_key(s: &str) -> Option<&str> {
    s.strip_prefix('%')
        .and_then(|s| s.strip_suffix('%'))
        .filter(|inner| {
            !inner.is_empty()
                && inner
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.')
        })
}

/// Validate a localizable field: if it's a `%key%`, the key must exist in the
/// en-US fallback table.
fn check_localizable(
    fallback: Option<&std::collections::BTreeMap<String, String>>,
    location: &'static str,
    value: &str,
) -> Option<ManifestValidationError> {
    let key = i18n_key(value)?;
    if fallback.is_some_and(|m| m.contains_key(key)) {
        None
    } else {
        Some(ManifestValidationError::UnresolvedI18nKey {
            location,
            key: key.to_string(),
        })
    }
}

/// Canonical plugin-name rules. 1-100 chars, lowercase ASCII +
/// digits + hyphens. Used by every install path so the rules can
/// never drift between entry points; legacy JSON installs delegate
/// here too.
pub fn validate_name(name: &str) -> Result<(), ManifestValidationError> {
    if name.is_empty() || name.len() > 100 {
        return Err(ManifestValidationError::InvalidName(name.to_string()));
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return Err(ManifestValidationError::InvalidName(name.to_string()));
    }
    Ok(())
}

fn validate_engines(
    manifest: &PluginManifest,
    ctx: &ValidationContext,
) -> Result<(), ManifestValidationError> {
    // Real semver matching. The plugin declares a constraint
    // (e.g. ">=1.5.0", "^2.0", "1.x"); the running backend
    // declares its current version via env!("CARGO_PKG_VERSION").
    let req_str = manifest.engines.nosdesk.trim();
    let req = semver::VersionReq::parse(req_str).map_err(|_| {
        ManifestValidationError::InvalidEngineRequirement {
            kind: "nosdesk",
            requirement: req_str.into(),
        }
    })?;
    // The running backend's version comes from the build, not the
    // manifest, so a parse failure here is a build-system bug, not
    // a plugin-author problem. Treat it as `expect`-worthy: the
    // value is fed by `env!("CARGO_PKG_VERSION")` which Cargo
    // guarantees is valid semver.
    let current = semver::Version::parse(ctx.nosdesk_version)
        .expect("backend CARGO_PKG_VERSION must be valid semver");
    if !req.matches(&current) {
        return Err(ManifestValidationError::EngineNotSatisfied {
            kind: "nosdesk",
            requirement: req_str.into(),
            current: ctx.nosdesk_version.into(),
        });
    }

    if manifest.engines.plugin_api != SUPPORTED_PLUGIN_API_VERSION {
        return Err(ManifestValidationError::PluginApiMismatch {
            requested: manifest.engines.plugin_api.clone(),
            supported: SUPPORTED_PLUGIN_API_VERSION,
        });
    }
    Ok(())
}

fn validate_author(
    manifest: &PluginManifest,
    ctx: &ValidationContext,
) -> Result<(), ManifestValidationError> {
    match ctx.tier {
        // Official plugins are signed by the Nosdesk root key. By
        // convention the manifest author MUST equal "Nosdesk".
        // Future spec change: derive from a registry-published
        // "official author" string so we don't hardcode here.
        ResolvedTier::Official => match manifest.author.as_deref() {
            Some("Nosdesk") => Ok(()),
            Some(other) => Err(ManifestValidationError::AuthorMismatch {
                manifest_author: other.into(),
                publisher_display_name: "Nosdesk".into(),
            }),
            None => Err(ManifestValidationError::AuthorRequired),
        },
        ResolvedTier::Verified | ResolvedTier::Community => {
            let publisher = ctx
                .publisher_display_name
                .ok_or(ManifestValidationError::AuthorRequired)?;
            match manifest.author.as_deref() {
                Some(a) if a == publisher => Ok(()),
                Some(a) => Err(ManifestValidationError::AuthorMismatch {
                    manifest_author: a.into(),
                    publisher_display_name: publisher.into(),
                }),
                None => Err(ManifestValidationError::AuthorRequired),
            }
        }
        // Local plugins are signed by the instance's own key.
        // Author is informational; skip the check.
        ResolvedTier::Local => Ok(()),
    }
}

fn validate_component(component: &PluginComponentConfig) -> Result<(), ManifestValidationError> {
    // v1 only implements `Slot`. Other variants parse so future
    // plugins can declare them, but are refused at install with a
    // clear "kind not yet supported" message.
    if component.kind != PluginComponentKind::Slot {
        return Err(ManifestValidationError::UnsupportedComponentKind(
            component.kind.as_str().to_string(),
        ));
    }

    if !crate::services::plugins::slot_registry::is_known_slot(&component.slot) {
        return Err(ManifestValidationError::InvalidSlot(component.slot.clone()));
    }
    for ctx in &component.context {
        if !KNOWN_CONTEXTS.contains(&ctx.as_str()) {
            return Err(ManifestValidationError::InvalidContext(ctx.clone()));
        }
    }
    Ok(())
}

fn validate_setting(setting: &PluginSettingDefinition) -> Result<(), ManifestValidationError> {
    if !KNOWN_SETTING_TYPES.contains(&setting.setting_type.as_str()) {
        return Err(ManifestValidationError::InvalidSettingType(
            setting.setting_type.clone(),
        ));
    }
    // Per-user scope is reserved: the manifest field exists so
    // plugin authors declare intent, but the storage layer is
    // global-only today. Refuse `user`-scoped settings until the
    // per-user setting table lands.
    if matches!(setting.scope, crate::models::PluginSettingScope::User) {
        return Err(ManifestValidationError::UnsupportedSettingScope {
            key: setting.key.clone(),
        });
    }
    Ok(())
}

fn validate_screenshot_path(path: &str) -> Result<(), ManifestValidationError> {
    // Screenshots resolve to entries inside the signed zip, so the
    // path must be a relative POSIX-style path. Reject absolute,
    // traversal, Windows-style, scheme-bearing, and protocol-relative
    // forms.
    if path.is_empty()
        || path.starts_with('/')
        || path.starts_with("//")
        || path.contains("..")
        || path.contains('\\')
        || path.contains("://")
    {
        return Err(ManifestValidationError::InvalidScreenshotPath(path.into()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{
        CollectionDefinition, PluginComponentConfig, PluginEngines, PluginLifecyclePolicy,
    };
    use crate::services::plugins::types::{Host, Permission};
    use std::collections::HashMap;

    fn ctx_official() -> ValidationContext<'static> {
        ValidationContext {
            tier: &ResolvedTier::Official,
            publisher_display_name: None,
            nosdesk_version: "0.1.0",
        }
    }

    fn perm(s: &str) -> Permission {
        Permission::parse(s).expect("test permission must parse")
    }

    fn host(s: &str) -> Host {
        Host::parse(s).expect("test host must parse")
    }

    /// `validate` accumulates errors; tests usually want to check
    /// "did this specific variant appear?" without caring about
    /// other errors in the Vec. This macro keeps the assertions
    /// terse and gives a useful panic message on mismatch.
    macro_rules! assert_err {
        ($result:expr, $pat:pat $(,)?) => {
            match $result {
                Err(errs) => assert!(
                    errs.iter().any(|e| matches!(e, $pat)),
                    "expected error matching `{}`, got: {:?}",
                    stringify!($pat),
                    errs
                ),
                Ok(()) => panic!(
                    "expected error matching `{}`, got Ok",
                    stringify!($pat)
                ),
            }
        };
        ($result:expr, $pat:pat if $cond:expr $(,)?) => {
            match $result {
                Err(errs) => assert!(
                    errs.iter().any(|e| matches!(e, $pat if $cond)),
                    "expected error matching `{} if {}`, got: {:?}",
                    stringify!($pat),
                    stringify!($cond),
                    errs
                ),
                Ok(()) => panic!(
                    "expected error matching `{} if {}`, got Ok",
                    stringify!($pat),
                    stringify!($cond)
                ),
            }
        };
    }

    fn baseline_manifest() -> PluginManifest {
        PluginManifest {
            manifest_version: 1,
            name: "github-integration".into(),
            display_name: "GitHub".into(),
            version: "1.0.0".into(),
            description: None,
            license: Some("MIT".into()),
            author: Some("Nosdesk".into()),
            repository: None,
            homepage: None,
            bugs: None,
            support_contact: None,
            engines: PluginEngines {
                nosdesk: ">=0.1.0".into(),
                plugin_api: "1".into(),
            },
            dependencies: std::collections::BTreeMap::new(),
            categories: vec!["integrations".into()],
            tags: vec!["github".into()],
            screenshots: vec![],
            permissions: vec![perm("ticket:read"), perm("network:api.github.com")],
            components: std::collections::BTreeMap::new(),
            events: vec!["ticket:created".into()],
            settings: vec![],
            collections: std::collections::BTreeMap::new(),
            auth: std::collections::BTreeMap::new(),
            lifecycle: PluginLifecyclePolicy::default(),
            commands: vec![],
            menus: std::collections::BTreeMap::new(),
            url_handlers: vec![],
            extensions: std::collections::BTreeMap::new(),
            i18n: std::collections::BTreeMap::new(),
        }
    }

    #[test]
    fn accepts_baseline() {
        validate(&baseline_manifest(), &ctx_official()).unwrap();
    }

    #[test]
    fn rejects_wrong_manifest_version() {
        let mut m = baseline_manifest();
        m.manifest_version = 2;
        assert_err!(
            validate(&m, &ctx_official()),
            ManifestValidationError::UnsupportedManifestVersion(2),
        );
    }

    #[test]
    fn unknown_permission_fails_at_deserialise() {
        // Permission allowlist enforcement is structural now: the
        // typed `Permission` enum's serde impl rejects unknowns at
        // parse time, so they never reach the validator. Verify
        // that `Permission::parse` itself rejects the cases the
        // old `InvalidPermission` test covered.
        assert!(Permission::parse("not-a-real-permission").is_err());
        assert!(Permission::parse("tickets:read").is_err());
    }

    #[test]
    fn accepts_network_permission() {
        let mut m = baseline_manifest();
        m.permissions.push(perm("network:example.com"));
        validate(&m, &ctx_official()).unwrap();
    }

    #[test]
    fn malformed_network_permission_fails_at_deserialise() {
        // `network:` with empty host, IP literal, port, userinfo,
        // wildcard-only: all rejected at parse time by `HostPattern::parse`.
        assert!(Permission::parse("network:").is_err());
        assert!(Permission::parse("network:127.0.0.1").is_err());
        assert!(Permission::parse("network:foo:8080").is_err());
        assert!(Permission::parse("network:user@foo.com").is_err());
        assert!(Permission::parse("network:*").is_err());
    }

    #[test]
    fn auth_host_covered_by_wildcard_permission() {
        // `network:*.github.com` covers `auth.api.github.com`.
        let mut m = baseline_manifest();
        m.permissions = vec![perm("network:*.github.com")];
        m.auth.insert(
            host("api.github.com"),
            crate::models::PluginAuthConfig::Bearer {
                secret: "tok".into(),
            },
        );
        validate(&m, &ctx_official()).unwrap();
    }

    #[test]
    fn auth_host_uppercased_normalises_to_match() {
        // The typed `Host` lowercases on construction, so a
        // mixed-case auth host can still be cross-checked against
        // a permission. This kills the historical drift where
        // the validator used byte-equal string compares.
        let mut m = baseline_manifest();
        m.permissions = vec![perm("network:api.github.com")];
        m.auth.insert(
            host("API.GITHUB.COM"),
            crate::models::PluginAuthConfig::Bearer {
                secret: "tok".into(),
            },
        );
        validate(&m, &ctx_official()).unwrap();
    }

    #[test]
    fn rejects_unknown_event() {
        let mut m = baseline_manifest();
        m.events.push("ticket:undefined".into());
        assert_err!(
            validate(&m, &ctx_official()),
            ManifestValidationError::InvalidEvent(_),
        );
    }

    #[test]
    fn rejects_unsupported_component_kind() {
        let mut m = baseline_manifest();
        m.components.insert(
            "Foo".into(),
            PluginComponentConfig {
                kind: PluginComponentKind::AdminPage,
                slot: "ticket-sidebar".into(),
                entry: "Foo".into(),
                context: vec![],
                label: None,
                icon: None,
                action: None,
            },
        );
        assert_err!(
            validate(&m, &ctx_official()),
            ManifestValidationError::UnsupportedComponentKind(k) if k == "admin_page",
        );
    }

    #[test]
    fn rejects_unknown_slot() {
        let mut m = baseline_manifest();
        m.components.insert(
            "Foo".into(),
            PluginComponentConfig {
                kind: PluginComponentKind::Slot,
                slot: "nonexistent-slot".into(),
                entry: "Foo".into(),
                context: vec![],
                label: None,
                icon: None,
                action: None,
            },
        );
        assert_err!(
            validate(&m, &ctx_official()),
            ManifestValidationError::InvalidSlot(_)
        );
    }

    #[test]
    fn rejects_unknown_collection_schema_version() {
        let mut m = baseline_manifest();
        m.collections.insert(
            "things".into(),
            CollectionDefinition {
                schema_version: 99,
                label: None,
                fields: HashMap::new(),
            },
        );
        assert_err!(
            validate(&m, &ctx_official()),
            ManifestValidationError::UnsupportedCollectionSchemaVersion { .. },
        );
    }

    #[test]
    fn rejects_official_with_wrong_author() {
        let mut m = baseline_manifest();
        m.author = Some("Some Other Person".into());
        assert_err!(
            validate(&m, &ctx_official()),
            ManifestValidationError::AuthorMismatch { .. },
        );
    }

    #[test]
    fn rejects_auth_for_undeclared_host() {
        let mut m = baseline_manifest();
        // Auth for api.example.com but only api.github.com is declared.
        m.auth.insert(
            host("api.example.com"),
            crate::models::PluginAuthConfig::Bearer {
                secret: "tok".into(),
            },
        );
        assert_err!(
            validate(&m, &ctx_official()),
            ManifestValidationError::AuthRefersToUndeclaredHost(_),
        );
    }

    #[test]
    fn rejects_unknown_category() {
        let mut m = baseline_manifest();
        m.categories.push("unknown-category".into());
        assert_err!(
            validate(&m, &ctx_official()),
            ManifestValidationError::InvalidCategory(_),
        );
    }

    #[test]
    fn rejects_screenshot_path_traversal() {
        let mut m = baseline_manifest();
        m.screenshots.push("../etc/passwd".into());
        assert_err!(
            validate(&m, &ctx_official()),
            ManifestValidationError::InvalidScreenshotPath(_),
        );
    }

    #[test]
    fn rejects_wrong_plugin_api_version() {
        let mut m = baseline_manifest();
        m.engines.plugin_api = "2".into();
        assert_err!(
            validate(&m, &ctx_official()),
            ManifestValidationError::PluginApiMismatch { .. },
        );
    }

    #[test]
    fn rejects_unsatisfied_nosdesk_constraint() {
        let mut m = baseline_manifest();
        m.engines.nosdesk = ">=99.0.0".into();
        assert_err!(
            validate(&m, &ctx_official()),
            ManifestValidationError::EngineNotSatisfied {
                kind: "nosdesk",
                ..
            },
        );
    }

    #[test]
    fn accepts_satisfied_nosdesk_constraint() {
        let mut m = baseline_manifest();
        m.engines.nosdesk = ">=0.0.0".into();
        validate(&m, &ctx_official()).unwrap();
    }

    #[test]
    fn rejects_unparseable_nosdesk_constraint() {
        // Distinct error variant for malformed requirements; lets
        // the install UI tell authors "fix your semver string"
        // separately from "your constraint excludes us".
        let mut m = baseline_manifest();
        m.engines.nosdesk = "not a semver req".into();
        assert_err!(
            validate(&m, &ctx_official()),
            ManifestValidationError::InvalidEngineRequirement {
                kind: "nosdesk",
                ..
            },
        );
    }

    #[test]
    fn rejects_invalid_dependency_version() {
        let mut m = baseline_manifest();
        m.dependencies
            .insert("calendar-core".into(), "garbage".into());
        assert_err!(
            validate(&m, &ctx_official()),
            ManifestValidationError::InvalidDependencyVersion { plugin, .. } if plugin == "calendar-core",
        );
    }

    #[test]
    fn accepts_valid_dependency() {
        let mut m = baseline_manifest();
        m.dependencies
            .insert("calendar-core".into(), ">=1.0.0".into());
        validate(&m, &ctx_official()).unwrap();
    }

    #[test]
    fn rejects_user_scope_setting() {
        use crate::models::{PluginSettingDefinition, PluginSettingScope};
        let mut m = baseline_manifest();
        m.settings.push(PluginSettingDefinition {
            key: "user_token".into(),
            setting_type: "secret".into(),
            label: "Token".into(),
            description: None,
            required: false,
            default: None,
            scope: PluginSettingScope::User,
            options: None,
        });
        assert_err!(
            validate(&m, &ctx_official()),
            ManifestValidationError::UnsupportedSettingScope { .. },
        );
    }

    #[test]
    fn rejects_non_empty_extensions() {
        // Closes the M3 inconsistency: previously `extensions:
        // serde_json::Value` used `is_null()`, which let an empty
        // object `{}` slip through. The field is now a BTreeMap
        // and the same `is_empty()` predicate applies as for the
        // other reserved fields.
        let mut m = baseline_manifest();
        m.extensions
            .insert("future_feature".into(), serde_json::json!({"x": 1}));
        assert_err!(
            validate(&m, &ctx_official()),
            ManifestValidationError::ReservedFieldNotEmpty {
                field: "extensions"
            },
        );
    }

    #[test]
    fn rejects_non_empty_commands() {
        use crate::models::PluginCommandDefinition;
        let mut m = baseline_manifest();
        m.commands.push(PluginCommandDefinition {
            id: "github.sync".into(),
            title: "Sync".into(),
            when: None,
        });
        assert_err!(
            validate(&m, &ctx_official()),
            ManifestValidationError::ReservedFieldNotEmpty { field: "commands" },
        );
    }

    #[test]
    fn rejects_i18n_key_without_en_us_fallback() {
        let mut m = baseline_manifest();
        m.display_name = "%my.app.name%".into();
        assert_err!(
            validate(&m, &ctx_official()),
            ManifestValidationError::UnresolvedI18nKey {
                location: "displayName",
                ..
            },
        );
    }

    #[test]
    fn accepts_i18n_key_with_en_us_fallback() {
        let mut m = baseline_manifest();
        m.display_name = "%my.app.name%".into();
        m.i18n.insert(
            "en-US".into(),
            std::collections::BTreeMap::from([("my.app.name".to_string(), "My App".to_string())]),
        );
        assert!(validate(&m, &ctx_official()).is_ok());
    }

    #[test]
    fn rejects_empty_i18n_value() {
        let mut m = baseline_manifest();
        m.i18n.insert(
            "en-US".into(),
            std::collections::BTreeMap::from([("k".to_string(), "  ".to_string())]),
        );
        assert_err!(
            validate(&m, &ctx_official()),
            ManifestValidationError::EmptyI18nValue { .. },
        );
    }

    #[test]
    fn rejects_invalid_bugs_url() {
        let mut m = baseline_manifest();
        m.bugs = Some("not a url".into());
        assert_err!(
            validate(&m, &ctx_official()),
            ManifestValidationError::InvalidBugsUrl(_),
        );
    }

    #[test]
    fn rejects_non_https_bugs_url() {
        // `WebUrl::parse` rejects non-https schemes. Closes the
        // C5 finding from the architectural review: previously
        // `url::Url::parse` accepted any scheme so `javascript:`
        // and `file:` slipped through.
        for bad in [
            "http://example.com",
            "javascript:alert(1)",
            "file:///etc/passwd",
        ] {
            let mut m = baseline_manifest();
            m.bugs = Some(bad.into());
            assert_err!(
                validate(&m, &ctx_official()),
                ManifestValidationError::InvalidBugsUrl(_),
            );
        }
    }

    #[test]
    fn accepts_email_support_contact() {
        let mut m = baseline_manifest();
        m.support_contact = Some("support@example.com".into());
        validate(&m, &ctx_official()).unwrap();
    }

    #[test]
    fn accepts_url_support_contact() {
        let mut m = baseline_manifest();
        m.support_contact = Some("https://nosdesk.com/help".into());
        validate(&m, &ctx_official()).unwrap();
    }

    #[test]
    fn rejects_garbage_support_contact() {
        let mut m = baseline_manifest();
        m.support_contact = Some("not an email or url".into());
        assert_err!(
            validate(&m, &ctx_official()),
            ManifestValidationError::InvalidContactUrl(_),
        );
    }

    #[test]
    fn rejects_non_https_support_contact_url() {
        for bad in ["http://example.com", "javascript:alert(1)"] {
            let mut m = baseline_manifest();
            m.support_contact = Some(bad.into());
            assert_err!(
                validate(&m, &ctx_official()),
                ManifestValidationError::InvalidContactUrl(_),
            );
        }
    }

    #[test]
    fn rejects_screenshot_with_backslash() {
        // M6: tighter screenshot path validation. Reject Windows
        // separators since they'd otherwise traverse on a Windows
        // host extracting the zip.
        let mut m = baseline_manifest();
        m.screenshots.push("..\\foo.png".into());
        assert_err!(
            validate(&m, &ctx_official()),
            ManifestValidationError::InvalidScreenshotPath(_),
        );
    }

    #[test]
    fn rejects_screenshot_with_scheme() {
        let mut m = baseline_manifest();
        m.screenshots.push("https://attacker.test/x.png".into());
        assert_err!(
            validate(&m, &ctx_official()),
            ManifestValidationError::InvalidScreenshotPath(_),
        );
    }

    #[test]
    fn rejects_screenshot_protocol_relative() {
        let mut m = baseline_manifest();
        m.screenshots.push("//cdn.attacker.test/x.png".into());
        assert_err!(
            validate(&m, &ctx_official()),
            ManifestValidationError::InvalidScreenshotPath(_),
        );
    }

    #[test]
    fn validate_accumulates_multiple_errors() {
        // The accumulator behaviour: an author with three problems
        // hears about all three, not just the first. Saves install
        // round-trips during plugin development.
        let mut m = baseline_manifest();
        m.events.push("ticket:undefined".into());
        m.categories.push("unknown-category".into());
        m.screenshots.push("../etc/passwd".into());

        let errs = validate(&m, &ctx_official()).unwrap_err();
        assert!(errs
            .iter()
            .any(|e| matches!(e, ManifestValidationError::InvalidEvent(_))));
        assert!(errs
            .iter()
            .any(|e| matches!(e, ManifestValidationError::InvalidCategory(_))));
        assert!(errs
            .iter()
            .any(|e| matches!(e, ManifestValidationError::InvalidScreenshotPath(_))));
        assert!(
            errs.len() >= 3,
            "expected >= 3 errors, got {}: {errs:?}",
            errs.len()
        );
    }

    #[test]
    fn manifest_version_mismatch_short_circuits() {
        // The exception to accumulation: if the schema version is
        // wrong, every other check is meaningless. Single error.
        let mut m = baseline_manifest();
        m.manifest_version = 99;
        m.events.push("ticket:undefined".into()); // would also fail
        m.categories.push("unknown-category".into()); // would also fail

        let errs = validate(&m, &ctx_official()).unwrap_err();
        assert_eq!(errs.len(), 1);
        assert!(matches!(
            errs[0],
            ManifestValidationError::UnsupportedManifestVersion(99)
        ));
    }
}
