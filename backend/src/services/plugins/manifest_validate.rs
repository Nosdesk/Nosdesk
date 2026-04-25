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

/// All permission strings the v1 backend recognises. Adding a
/// permission means adding to this list AND adding the
/// corresponding enforcement in the relevant handler. A plugin
/// that requests a permission missing here is refused at install,
/// fail-closed.
// Permission allowlist enforcement is handled structurally by
// `crate::services::plugins::types::Permission`: an unknown
// permission string fails to deserialise into the typed enum, so
// it never reaches the validator. The list of known capabilities
// lives in that module's `Permission::parse`.

/// Slot identifiers from the canonical taxonomy declared in
/// `frontend/src/types/plugin.ts::PLUGIN_SLOTS`. Plugins may
/// declare any slot in this list at install time; if the runtime
/// hasn't yet added a `<PluginSlot slot-name="x">` mount point
/// for the slot, the plugin's contribution is a silent no-op
/// (matches the VS Code contribution-point model).
///
/// Adding a slot is a two-step coordinated change: extend the
/// frontend `PluginSlot` union + `PLUGIN_SLOTS` registry, and
/// add the literal here. Mounting the slot in a Vue template is
/// independent — plugins can author against the slot before
/// the template lands, the contribution just doesn't render
/// until the mount point exists.
pub const KNOWN_SLOTS: &[&str] = &[
    // Global
    "navbar-items",
    "settings-integrations",
    // Ticket context
    "ticket-header-actions",
    "ticket-sidebar",
    "ticket-tabs",
    "ticket-footer-actions",
    // Document context
    "document-toolbar",
    "document-sidebar",
    // Device context
    "device-header-actions",
    "device-info-panels",
];

/// Context types a component can request. The runtime passes the
/// matching object on the `context` prop.
pub const KNOWN_CONTEXTS: &[&str] = &[
    "ticket",
    // Reserved:
    "device",
    "user",
    "comment",
    "documentation_page",
];

/// Events plugins can subscribe to. Canonical taxonomy mirrored
/// from `frontend/src/types/plugin.ts::PLUGIN_EVENTS`.
///
/// Subscribing to an event whose dispatch site doesn't yet exist
/// is a silent no-op (handler never fires) — the same pub/sub
/// "loose subscription" pattern industry plugin systems use.
/// New events are added by extending the dispatcher's SSE map or
/// `TICKET_FIELD_TO_EVENT` table on the frontend, then mirroring
/// the literal here.
pub const KNOWN_EVENTS: &[&str] = &[
    "ticket:created",
    "ticket:updated",
    "ticket:status_changed",
    "ticket:assigned",
    "ticket:comment_added",
    "document:created",
    "document:updated",
    "device:created",
    "device:updated",
];

/// Setting types accepted on `settings[].type`. Each maps to a
/// frontend renderer + a backend storage policy (notably:
/// `secret` is encrypted at rest).
pub const KNOWN_SETTING_TYPES: &[&str] = &[
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
pub const KNOWN_CATEGORIES: &[&str] = &[
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
    LocalisationKeyReserved {
        location: &'static str,
        value: String,
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
            Self::LocalisationKeyReserved { location, value } => write!(
                f,
                "{location} value {value:?} matches the reserved localisation syntax %key%; future Nosdesk versions will resolve these from i18n bundles. Use literal text or wait."
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

/// Run every check the v1 manifest schema requires. Errors are
/// returned in declaration order; only the first failure is
/// reported. Plugins authoring against this should treat each
/// possible error as an explicit rule — none is implicit.
pub fn validate(
    manifest: &PluginManifest,
    ctx: &ValidationContext,
) -> Result<(), ManifestValidationError> {
    if manifest.manifest_version != SUPPORTED_MANIFEST_VERSION {
        return Err(ManifestValidationError::UnsupportedManifestVersion(
            manifest.manifest_version,
        ));
    }

    validate_name(&manifest.name)?;
    validate_engines(manifest, ctx)?;
    validate_author(manifest, ctx)?;

    // Permissions are now typed (`Vec<Permission>`); deserialisation
    // already rejected unknown strings and malformed `network:`
    // patterns before we got here. We just collect the network
    // patterns for the auth cross-check below.

    for event in &manifest.events {
        if !KNOWN_EVENTS.contains(&event.as_str()) {
            return Err(ManifestValidationError::InvalidEvent(event.clone()));
        }
    }

    for component in manifest.components.values() {
        validate_component(component)?;
    }

    for setting in &manifest.settings {
        validate_setting(setting)?;
    }

    for category in &manifest.categories {
        if !KNOWN_CATEGORIES.contains(&category.as_str()) {
            return Err(ManifestValidationError::InvalidCategory(category.clone()));
        }
    }

    for screenshot in &manifest.screenshots {
        validate_screenshot_path(screenshot)?;
    }

    for (collection_name, def) in &manifest.collections {
        if def.schema_version != 1 {
            return Err(ManifestValidationError::UnsupportedCollectionSchemaVersion {
                collection: collection_name.clone(),
                version: def.schema_version,
            });
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
        let auth_pattern =
            crate::services::plugins::types::HostPattern::Exact(host.clone());
        if !network_patterns.iter().any(|p| p.covers(&auth_pattern)) {
            return Err(ManifestValidationError::AuthRefersToUndeclaredHost(
                host.as_str().to_string(),
            ));
        }
    }

    for (plugin_name, requirement) in &manifest.dependencies {
        semver::VersionReq::parse(requirement.trim()).map_err(|_| {
            ManifestValidationError::InvalidDependencyVersion {
                plugin: plugin_name.clone(),
                requirement: requirement.clone(),
            }
        })?;
    }

    // URL-shaped fields. `bugs` must be a `WebUrl` (https-only).
    // `support_contact` accepts either a `WebUrl` or an email
    // (the at-sign is the cheap check; we don't validate RFC 5322
    // compliance). The https-only constraint closes a class of
    // dangerous-scheme links rendered in the registry UI
    // (`javascript:`, `file:`, `data:`).
    if let Some(bugs) = &manifest.bugs {
        if crate::services::plugins::types::WebUrl::parse(bugs).is_err() {
            return Err(ManifestValidationError::InvalidBugsUrl(bugs.clone()));
        }
    }
    if let Some(contact) = &manifest.support_contact {
        let looks_like_url =
            crate::services::plugins::types::WebUrl::parse(contact).is_ok();
        let looks_like_email = contact.contains('@')
            && !contact.contains(' ')
            && !contact.starts_with('@')
            && !contact.ends_with('@');
        if !looks_like_url && !looks_like_email {
            return Err(ManifestValidationError::InvalidContactUrl(contact.clone()));
        }
    }

    // Reserved fields: refuse non-empty values for everything not
    // yet implemented at runtime. Plugin authors who want to use
    // these will hit a clear "not yet supported" message rather
    // than silent no-ops.
    if !manifest.commands.is_empty() {
        return Err(ManifestValidationError::ReservedFieldNotEmpty { field: "commands" });
    }
    if !manifest.menus.is_empty() {
        return Err(ManifestValidationError::ReservedFieldNotEmpty { field: "menus" });
    }
    if !manifest.url_handlers.is_empty() {
        return Err(ManifestValidationError::ReservedFieldNotEmpty {
            field: "url_handlers",
        });
    }
    if !manifest.extensions.is_null() {
        return Err(ManifestValidationError::ReservedFieldNotEmpty { field: "extensions" });
    }

    // Localisation syntax reservation: any string of the form
    // `%key%` is reserved for a future i18n resolver. Refuse
    // surface-visible strings that look like keys today so plugins
    // can't accidentally claim the syntax.
    let l10n_re = LocalisationKeyMatcher::new();
    if l10n_re.looks_like_key(&manifest.display_name) {
        return Err(ManifestValidationError::LocalisationKeyReserved {
            location: "displayName",
            value: manifest.display_name.clone(),
        });
    }
    for setting in &manifest.settings {
        if l10n_re.looks_like_key(&setting.label) {
            return Err(ManifestValidationError::LocalisationKeyReserved {
                location: "settings[].label",
                value: setting.label.clone(),
            });
        }
        if let Some(d) = &setting.description {
            if l10n_re.looks_like_key(d) {
                return Err(ManifestValidationError::LocalisationKeyReserved {
                    location: "settings[].description",
                    value: d.clone(),
                });
            }
        }
    }

    Ok(())
}

/// Heuristic for the reserved localisation syntax `%key%`. Matches
/// strings that consist of a leading `%`, identifier characters
/// (letters, digits, underscore, dot), and a trailing `%`. Used to
/// refuse manifest strings that would collide with future i18n
/// bundles.
struct LocalisationKeyMatcher;

impl LocalisationKeyMatcher {
    fn new() -> Self {
        Self
    }

    fn looks_like_key(&self, s: &str) -> bool {
        let bytes = s.as_bytes();
        if bytes.len() < 3 {
            return false;
        }
        if bytes[0] != b'%' || bytes[bytes.len() - 1] != b'%' {
            return false;
        }
        let inner = &s[1..s.len() - 1];
        if inner.is_empty() {
            return false;
        }
        inner
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.')
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
    // Refuse install if the constraint isn't satisfied.
    let req_str = manifest.engines.nosdesk.trim();
    let req = semver::VersionReq::parse(req_str).map_err(|_| {
        ManifestValidationError::EngineNotSatisfied {
            kind: "nosdesk",
            requirement: req_str.into(),
            current: ctx.nosdesk_version.into(),
        }
    })?;
    let current = semver::Version::parse(ctx.nosdesk_version).map_err(|_| {
        ManifestValidationError::EngineNotSatisfied {
            kind: "nosdesk",
            requirement: req_str.into(),
            current: ctx.nosdesk_version.into(),
        }
    })?;
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

fn validate_component(
    component: &PluginComponentConfig,
) -> Result<(), ManifestValidationError> {
    // v1 only implements `Slot`. Other variants parse so future
    // plugins can declare them, but are refused at install with a
    // clear "kind not yet supported" message.
    if component.kind != PluginComponentKind::Slot {
        return Err(ManifestValidationError::UnsupportedComponentKind(
            component.kind.as_str().to_string(),
        ));
    }

    if !KNOWN_SLOTS.contains(&component.slot.as_str()) {
        return Err(ManifestValidationError::InvalidSlot(component.slot.clone()));
    }
    for ctx in &component.context {
        if !KNOWN_CONTEXTS.contains(&ctx.as_str()) {
            return Err(ManifestValidationError::InvalidContext(ctx.clone()));
        }
    }
    Ok(())
}

fn validate_setting(
    setting: &PluginSettingDefinition,
) -> Result<(), ManifestValidationError> {
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
            dependencies: HashMap::new(),
            categories: vec!["integrations".into()],
            tags: vec!["github".into()],
            screenshots: vec![],
            permissions: vec![perm("ticket:read"), perm("network:api.github.com")],
            components: HashMap::new(),
            events: vec!["ticket:created".into()],
            settings: vec![],
            collections: HashMap::new(),
            auth: HashMap::new(),
            lifecycle: PluginLifecyclePolicy::default(),
            commands: vec![],
            menus: HashMap::new(),
            url_handlers: vec![],
            extensions: serde_json::Value::Null,
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
        match validate(&m, &ctx_official()) {
            Err(ManifestValidationError::UnsupportedManifestVersion(2)) => {}
            other => panic!("expected UnsupportedManifestVersion, got {other:?}"),
        }
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
        // wildcard-only — all rejected at parse time by `HostPattern::parse`.
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
            crate::models::PluginAuthConfig::Bearer { secret: "tok".into() },
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
            crate::models::PluginAuthConfig::Bearer { secret: "tok".into() },
        );
        validate(&m, &ctx_official()).unwrap();
    }

    #[test]
    fn rejects_unknown_event() {
        let mut m = baseline_manifest();
        m.events.push("ticket:undefined".into());
        assert!(matches!(
            validate(&m, &ctx_official()),
            Err(ManifestValidationError::InvalidEvent(_))
        ));
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
        match validate(&m, &ctx_official()) {
            Err(ManifestValidationError::UnsupportedComponentKind(k)) if k == "admin_page" => {}
            other => panic!("expected UnsupportedComponentKind, got {other:?}"),
        }
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
        assert!(matches!(
            validate(&m, &ctx_official()),
            Err(ManifestValidationError::InvalidSlot(_))
        ));
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
        assert!(matches!(
            validate(&m, &ctx_official()),
            Err(ManifestValidationError::UnsupportedCollectionSchemaVersion { .. })
        ));
    }

    #[test]
    fn rejects_official_with_wrong_author() {
        let mut m = baseline_manifest();
        m.author = Some("Some Other Person".into());
        assert!(matches!(
            validate(&m, &ctx_official()),
            Err(ManifestValidationError::AuthorMismatch { .. })
        ));
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
        assert!(matches!(
            validate(&m, &ctx_official()),
            Err(ManifestValidationError::AuthRefersToUndeclaredHost(_))
        ));
    }

    #[test]
    fn rejects_unknown_category() {
        let mut m = baseline_manifest();
        m.categories.push("unknown-category".into());
        assert!(matches!(
            validate(&m, &ctx_official()),
            Err(ManifestValidationError::InvalidCategory(_))
        ));
    }

    #[test]
    fn rejects_screenshot_path_traversal() {
        let mut m = baseline_manifest();
        m.screenshots.push("../etc/passwd".into());
        assert!(matches!(
            validate(&m, &ctx_official()),
            Err(ManifestValidationError::InvalidScreenshotPath(_))
        ));
    }

    #[test]
    fn rejects_wrong_plugin_api_version() {
        let mut m = baseline_manifest();
        m.engines.plugin_api = "2".into();
        assert!(matches!(
            validate(&m, &ctx_official()),
            Err(ManifestValidationError::PluginApiMismatch { .. })
        ));
    }

    #[test]
    fn rejects_unsatisfied_nosdesk_constraint() {
        let mut m = baseline_manifest();
        m.engines.nosdesk = ">=99.0.0".into();
        match validate(&m, &ctx_official()) {
            Err(ManifestValidationError::EngineNotSatisfied { kind, .. }) if kind == "nosdesk" => {}
            other => panic!("expected EngineNotSatisfied(nosdesk), got {other:?}"),
        }
    }

    #[test]
    fn accepts_satisfied_nosdesk_constraint() {
        let mut m = baseline_manifest();
        m.engines.nosdesk = ">=0.0.0".into();
        validate(&m, &ctx_official()).unwrap();
    }

    #[test]
    fn rejects_unparseable_nosdesk_constraint() {
        let mut m = baseline_manifest();
        m.engines.nosdesk = "not a semver req".into();
        assert!(matches!(
            validate(&m, &ctx_official()),
            Err(ManifestValidationError::EngineNotSatisfied { kind: "nosdesk", .. })
        ));
    }

    #[test]
    fn rejects_invalid_dependency_version() {
        let mut m = baseline_manifest();
        m.dependencies
            .insert("calendar-core".into(), "garbage".into());
        match validate(&m, &ctx_official()) {
            Err(ManifestValidationError::InvalidDependencyVersion { plugin, .. })
                if plugin == "calendar-core" => {}
            other => panic!("expected InvalidDependencyVersion, got {other:?}"),
        }
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
        assert!(matches!(
            validate(&m, &ctx_official()),
            Err(ManifestValidationError::UnsupportedSettingScope { .. })
        ));
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
        assert!(matches!(
            validate(&m, &ctx_official()),
            Err(ManifestValidationError::ReservedFieldNotEmpty { field: "commands" })
        ));
    }

    #[test]
    fn rejects_localisation_key_in_display_name() {
        let mut m = baseline_manifest();
        m.display_name = "%my.app.name%".into();
        assert!(matches!(
            validate(&m, &ctx_official()),
            Err(ManifestValidationError::LocalisationKeyReserved {
                location: "displayName",
                ..
            })
        ));
    }

    #[test]
    fn rejects_invalid_bugs_url() {
        let mut m = baseline_manifest();
        m.bugs = Some("not a url".into());
        assert!(matches!(
            validate(&m, &ctx_official()),
            Err(ManifestValidationError::InvalidBugsUrl(_))
        ));
    }

    #[test]
    fn rejects_non_https_bugs_url() {
        // `WebUrl::parse` rejects non-https schemes. Closes the
        // C5 finding from the architectural review: previously
        // `url::Url::parse` accepted any scheme so `javascript:`
        // and `file:` slipped through.
        for bad in ["http://example.com", "javascript:alert(1)", "file:///etc/passwd"] {
            let mut m = baseline_manifest();
            m.bugs = Some(bad.into());
            assert!(
                matches!(validate(&m, &ctx_official()), Err(ManifestValidationError::InvalidBugsUrl(_))),
                "expected {bad:?} to be rejected as bugs URL"
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
        assert!(matches!(
            validate(&m, &ctx_official()),
            Err(ManifestValidationError::InvalidContactUrl(_))
        ));
    }

    #[test]
    fn rejects_non_https_support_contact_url() {
        for bad in ["http://example.com", "javascript:alert(1)"] {
            let mut m = baseline_manifest();
            m.support_contact = Some(bad.into());
            assert!(
                matches!(validate(&m, &ctx_official()), Err(ManifestValidationError::InvalidContactUrl(_))),
                "expected {bad:?} to be rejected as support_contact"
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
        assert!(matches!(
            validate(&m, &ctx_official()),
            Err(ManifestValidationError::InvalidScreenshotPath(_))
        ));
    }

    #[test]
    fn rejects_screenshot_with_scheme() {
        let mut m = baseline_manifest();
        m.screenshots.push("https://attacker.test/x.png".into());
        assert!(matches!(
            validate(&m, &ctx_official()),
            Err(ManifestValidationError::InvalidScreenshotPath(_))
        ));
    }

    #[test]
    fn rejects_screenshot_protocol_relative() {
        let mut m = baseline_manifest();
        m.screenshots.push("//cdn.attacker.test/x.png".into());
        assert!(matches!(
            validate(&m, &ctx_official()),
            Err(ManifestValidationError::InvalidScreenshotPath(_))
        ));
    }
}
