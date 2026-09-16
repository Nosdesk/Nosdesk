use super::plugin_collections::CollectionDefinition;
use super::plugins::{Plugin, PluginActivity, PluginData, PluginState};
use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ===== PLUGIN API TYPES =====

/// Plugin manifest structure (matches frontend manifest.json format).
///
/// `deny_unknown_fields` is load-bearing: every field a plugin
/// declares must be one this binary understands, otherwise we fail
/// closed at install. Combined with `manifest_version`, that lets
/// us evolve the schema without ambiguity. v2 plugins declare
/// `manifest_version: 2` and the parser dispatches to a different
/// struct; v1 plugins are forever interpreted by the rules below.
///
/// Trust-affecting fields (`name`, `permissions`, `engines`, etc.)
/// are part of the canonical archive digest because they live in
/// `manifest.json`, so the signer commits to all of them.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginManifest {
    /// MUST be 1 for the schema described here. Future bumps go to
    /// 2/3/etc. Validators dispatch on this.
    pub manifest_version: u32,

    /// Stable plugin identifier. Lowercase ASCII letters, digits,
    /// and hyphens. Used as the DB key and display URL slug.
    pub name: String,

    /// User-facing name. Free-form, locale-neutral.
    #[serde(rename = "displayName")]
    pub display_name: String,

    /// SemVer string (e.g. "2.1.0"). Compared between installs to
    /// detect upgrades.
    pub version: String,

    /// Short user-facing description. Free-form.
    pub description: Option<String>,

    /// SPDX license identifier (e.g. "MIT", "Apache-2.0",
    /// "BUSL-1.1"). Optional but strongly recommended.
    pub license: Option<String>,

    /// Author display name. For non-official plugins (verified /
    /// community tier), the install pipeline asserts this matches
    /// the publishers.json entry for the signing key. Local-tier
    /// installs skip the check.
    pub author: Option<String>,

    /// Source repository URL.
    pub repository: Option<String>,

    /// Plugin homepage / documentation URL.
    pub homepage: Option<String>,

    /// Issue tracker URL. Distinct from `repository` because some
    /// plugins host code on one host and bugs on another (e.g.
    /// Bugzilla, Linear, internal tracker).
    pub bugs: Option<String>,

    /// Support contact: email or URL. Surfaced on the registry
    /// browse UI so users know where to ask for help. Format
    /// validated lightly: must contain `@` or look like a URL.
    pub support_contact: Option<String>,

    /// Engine compatibility. Plugin will be refused if the
    /// instance doesn't satisfy these constraints.
    pub engines: PluginEngines,

    /// Other plugins this one depends on. Each value is a semver
    /// requirement against the dep's `version`. The install
    /// pipeline refuses if a declared dep isn't installed; it does
    /// NOT auto-install transitively (registry-driven install
    /// surfaces the prompt for the operator). Reserved shape for
    /// future inter-plugin APIs and ordering guarantees; even
    /// without those, having the declaration prevents silent
    /// "plugin assumes peer is present" footguns.
    #[serde(default)]
    pub dependencies: std::collections::BTreeMap<String, String>,

    /// Discovery taxonomy for the registry browse UI. Values are
    /// validated against an allowlist of known categories.
    #[serde(default)]
    pub categories: Vec<String>,

    /// Free-form discovery tags. No allowlist; the registry build
    /// can lowercase + dedupe but doesn't reject unknowns.
    #[serde(default)]
    pub tags: Vec<String>,

    /// Paths inside the zip pointing at PNG/SVG screenshots for
    /// the registry browse UI. Validated at install.
    #[serde(default)]
    pub screenshots: Vec<String>,

    /// Capability grants the plugin requests. Parsed at manifest
    /// load time into typed `Permission` values; unknown or
    /// malformed entries fail deserialisation, so consumers past
    /// this point never see raw permission strings.
    #[serde(default)]
    pub permissions: Vec<crate::services::plugins::types::Permission>,

    /// Optional author-supplied justifications, keyed by the exact
    /// permission string (`"resource:img:*.tile.openstreetmap.org"`),
    /// surfaced verbatim on the consent screen so the operator can
    /// judge each grant. Untrusted display text: the consent UI
    /// escapes it and never treats it as a grant. Keys that don't
    /// match a requested permission are ignored.
    #[serde(default)]
    pub permission_reasons: std::collections::BTreeMap<String, String>,

    /// Components this plugin contributes. Keyed by component name
    /// (used as the entry-point key in the bundle's default export).
    #[serde(default)]
    pub components: std::collections::BTreeMap<String, PluginComponentConfig>,

    /// Events the plugin subscribes to. Validated against an
    /// allowlist; unknown events refused.
    #[serde(default)]
    pub events: Vec<String>,

    /// Plugin-defined settings rendered in the admin UI.
    #[serde(default)]
    pub settings: Vec<PluginSettingDefinition>,

    /// Plugin-owned collections. Each carries its own
    /// `schema_version` so future migrations can be expressed.
    #[serde(default)]
    pub collections: std::collections::BTreeMap<String, CollectionDefinition>,

    /// Declarative auth configuration: maps exact hostnames to
    /// auth strategies the proxy injects automatically. Wildcards
    /// are NOT permitted as auth keys (a future schema bump can
    /// loosen this if a real use case appears); each declared host
    /// must be covered by at least one `network:` permission.
    #[serde(default)]
    pub auth: std::collections::BTreeMap<crate::services::plugins::types::Host, PluginAuthConfig>,

    /// Lifecycle policy declarations. Default cascades plugin data
    /// on uninstall; plugins that store user-meaningful work
    /// should declare `on_uninstall: "preserve"`.
    #[serde(default)]
    pub lifecycle: PluginLifecyclePolicy,

    /// Palette-triggerable actions the plugin contributes. Reserved
    /// in v1: declared, validated, but the runtime palette is not
    /// yet implemented. Refused at install if non-empty until the
    /// dispatcher lands.
    #[serde(default)]
    pub commands: Vec<PluginCommandDefinition>,

    /// Menu contributions, keyed by menu identifier (e.g.
    /// `ticket-context`). Reserved in v1.
    #[serde(default)]
    pub menus: std::collections::BTreeMap<String, Vec<PluginMenuItem>>,

    /// URL-handler claims, e.g. `nosdesk://plugin/<plugin-name>/...`
    /// patterns this plugin owns. Reserved in v1.
    #[serde(default)]
    pub url_handlers: Vec<PluginUrlHandler>,

    /// Forward-compat bucket for typed inter-plugin exports.
    /// Modelled as a `BTreeMap<String, serde_json::Value>` so the
    /// same `is_empty()` predicate gates every reserved field;
    /// previously this was `serde_json::Value` with an
    /// `is_null()` check that let `{}` slip through. v1 refuses
    /// any non-empty value at install.
    #[serde(default)]
    pub extensions: std::collections::BTreeMap<String, serde_json::Value>,

    /// Per-locale string tables for localizing surface-visible fields. A
    /// localizable field (`displayName`, `components[].label`/`action.label`,
    /// `settings[].label`/`description`/`options[].label`) may be `%key%`, which
    /// the UI resolves against `i18n[<locale>][<key>]`, falling back to
    /// `i18n["en-US"][<key>]`, then the literal. Every `%key%` used MUST be
    /// defined for `en-US` (validated at install), so a fallback always exists.
    #[serde(default)]
    pub i18n: std::collections::BTreeMap<String, std::collections::BTreeMap<String, String>>,
}

/// Engine compatibility constraints. Both values are required.
/// Refused at install when not satisfied.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginEngines {
    /// SemVer requirement against the running Nosdesk version
    /// (e.g. ">=1.5.0", "^2.0", "1.4.x").
    pub nosdesk: String,

    /// Plugin runtime API major version the plugin was built
    /// against. Currently must be "1". The runtime exposes the
    /// supported version range to plugin code via `api.version`.
    pub plugin_api: String,
}

/// Declarative lifecycle policy. v1 honours `on_uninstall` only;
/// future fields here can land without breaking older manifests
/// because new defaults are added with `#[serde(default)]`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct PluginLifecyclePolicy {
    /// What happens to plugin-owned data when the plugin is
    /// uninstalled. `cascade` deletes all `plugin_data` and
    /// `plugin_collection_rows` for the plugin; `preserve` keeps
    /// them, supporting reinstall-without-data-loss flows.
    #[serde(default)]
    pub on_uninstall: PluginUninstallPolicy,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum PluginUninstallPolicy {
    #[default]
    Cascade,
    Preserve,
}

/// Palette command contributed by a plugin. Reserved for the
/// future command-palette dispatcher; v1 install refuses non-empty
/// `commands` arrays.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginCommandDefinition {
    /// Stable namespaced identifier, e.g. `github.sync`.
    pub id: String,
    /// User-facing label.
    pub title: String,
    /// Optional context filter (matches `KNOWN_CONTEXTS`).
    pub when: Option<String>,
}

/// Menu item contributed by a plugin. Reserved in v1.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginMenuItem {
    /// Command id this entry invokes.
    pub command: String,
    /// Optional grouping hint (e.g. `integrations`).
    pub group: Option<String>,
}

/// URL handler claim, e.g. `nosdesk://plugin/<plugin-name>/<pattern>`.
/// Reserved in v1.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginUrlHandler {
    /// Glob-like pattern under the plugin's namespace, e.g. `link/*`.
    pub pattern: String,
    /// Command id to invoke when matched.
    pub command: Option<String>,
}

/// Authentication configuration for a specific domain/host pattern
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PluginAuthConfig {
    /// Authorization: Bearer <secret_value>
    Bearer { secret: String },
    /// Authorization: Basic base64(username:password)
    Basic {
        username_secret: String,
        password_secret: String,
    },
    /// Custom header with secret value (e.g. X-API-Key)
    ApiKey { header: String, secret: String },
    /// OAuth2 Client Credentials flow: exchanges client_id + client_secret for a bearer token
    Oauth2ClientCredentials {
        token_url: String,
        client_id_secret: String,
        client_secret_secret: String,
    },
}

/// Plugin component configuration in manifest. The `kind` field
/// reserves space for future component shapes (settings tabs,
/// admin pages, background workers, webhook handlers); v1 only
/// implements `slot`-kind components, but the field is required
/// so future plugins can be expressed without a manifest version
/// bump.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginComponentConfig {
    /// What this component IS. Defaults to `slot` for backward
    /// readability; future kinds expand the allowed set.
    #[serde(default)]
    pub kind: PluginComponentKind,

    /// For `kind = slot`: the slot identifier (validated against
    /// allowlist). For other kinds, semantics differ.
    pub slot: String,

    /// Entry-point key inside the plugin's bundle default export.
    pub entry: String,

    /// Context types the component receives at render time
    /// (e.g. `["ticket"]`). Validated against allowlist.
    #[serde(default)]
    pub context: Vec<String>,

    pub label: Option<String>,
    pub icon: Option<String>,
    pub action: Option<PluginComponentAction>,

    /// Override the host chrome drawn around this component, for `panel`
    /// slots only. `None` (the usual case) takes the slot's default from
    /// the generated slot registry. Set `Chrome::None` for genuinely
    /// full-bleed content (a map, a media surface) that a card would frame
    /// wrongly. Rejected on `action` slots, which have no iframe to wrap.
    pub chrome: Option<PluginComponentChrome>,
}

/// Host chrome around a panel contribution. Mirrors the TS `SlotChrome`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PluginComponentChrome {
    /// The app's `SectionCard`: border, radius, surface and a header pill
    /// titled from `label`. The plugin fills the body only.
    Card,
    /// Bare frame, no host chrome.
    None,
}

impl PluginComponentChrome {
    /// Wire-format string (matches the serde `rename_all`).
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Card => "card",
            Self::None => "none",
        }
    }
}

/// Component kind. Only `Slot` is implemented in v1; the others
/// are reserved enum variants so a future plugin declaring
/// `kind: "admin_page"` is parseable today (and rejected at
/// install with a clear "kind not yet supported" error rather
/// than a parse failure that looks like a bug).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum PluginComponentKind {
    #[default]
    Slot,
    /// Reserved: a settings panel rendered inside the plugin's
    /// settings dialog instead of the declarative settings form.
    Settings,
    /// Reserved: a full admin page mounted at /admin/plugins/<name>/...
    AdminPage,
    /// Reserved: a backend worker invoked on a schedule.
    Worker,
    /// Reserved: a webhook handler matching a registered path.
    Webhook,
}

impl PluginComponentKind {
    /// Wire-format string for this kind (matches the serde
    /// `rename_all = "snake_case"`). Used by validators when
    /// reporting "kind X is not supported" without depending on
    /// `serde_json` to round-trip.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Slot => "slot",
            Self::Settings => "settings",
            Self::AdminPage => "admin_page",
            Self::Worker => "worker",
            Self::Webhook => "webhook",
        }
    }
}

/// Plugin component action for unified "+ Add" menu
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginComponentAction {
    pub label: String,
}

/// Plugin setting definition in manifest
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginSettingDefinition {
    pub key: String,
    #[serde(rename = "type")]
    pub setting_type: String,
    pub label: String,
    pub description: Option<String>,
    #[serde(default)]
    pub required: bool,
    pub default: Option<serde_json::Value>,
    /// Storage scope. `global` (default) means one value per
    /// instance; `user` means one value per logged-in user
    /// (e.g. each user's own GitHub PAT). Reserved in v1: the
    /// install validator refuses `user`-scoped settings until the
    /// per-user storage layer lands. Declaring the field now
    /// prevents the storage layout from being implicitly committed
    /// to "everything global" by the first wave of plugins.
    #[serde(default)]
    pub scope: PluginSettingScope,
    #[serde(default)]
    pub options: Option<Vec<PluginSettingOption>>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum PluginSettingScope {
    #[default]
    Global,
    User,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginSettingOption {
    pub value: String,
    pub label: String,
}

/// Request to toggle a plugin's lifecycle state. The endpoint
/// only honours the enabled-toggle (Installed <-> Disabled);
/// manifest edits used to be allowed here but were removed
/// because they bypassed signature reverification: an admin
/// could rewrite a verified plugin's stored manifest while the
/// signer fields kept claiming the original signer signed it.
/// Manifest changes now flow through the signed install paths
/// (zip upload, registry install) which re-verify end-to-end.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdatePluginRequest {
    pub enabled: Option<bool>,
}

/// Plugin response (for API)
#[derive(Debug, Serialize)]
pub struct PluginResponse {
    pub uuid: Uuid,
    pub name: String,
    pub display_name: String,
    pub version: String,
    pub description: Option<String>,
    pub manifest: PluginManifest,
    /// Lifecycle state. Serialises to one of `installed` /
    /// `disabled` / `quarantined` / `uninstalled` on the wire.
    /// The frontend toggles render rows where this is `installed`
    /// or `disabled`; the others are rendered as read-only audit
    /// entries.
    pub state: PluginState,
    pub trust_level: String,
    pub installed_by: Option<Uuid>,
    pub installed_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
    pub bundle_hash: Option<String>,
    pub bundle_size: Option<i32>,
    pub bundle_uploaded_at: Option<NaiveDateTime>,
    pub source: String,
    /// The permission set the admin consented to, as a string array. `None` for
    /// legacy rows installed before the consent gate (the frontend then falls
    /// back to the manifest's requested set). This is the AUTHORITATIVE grant the
    /// UI + runtime gate against, not `manifest.permissions`.
    pub consented_permissions: Option<Vec<String>>,
    /// When non-null, the publisher that signed this plugin has been
    /// revoked from `plugin_trusted_publishers`. The plugin keeps
    /// running (we don't tear down installed rows on revocation),
    /// but the admin UI surfaces the state so operators can decide
    /// whether to uninstall or keep with the trust caveat. NULL for
    /// official-tier plugins (signed by the Nosdesk root) and
    /// local-tier plugins (signed by the instance key), since
    /// neither resolves through plugin_trusted_publishers.
    pub signer_revoked_at: Option<NaiveDateTime>,
}

impl Plugin {
    /// Parse the manifest JSON into a PluginManifest struct
    pub fn parse_manifest(&self) -> Result<PluginManifest, serde_json::Error> {
        serde_json::from_value(self.manifest.clone())
    }
}

impl TryFrom<Plugin> for PluginResponse {
    type Error = serde_json::Error;

    fn try_from(p: Plugin) -> Result<Self, Self::Error> {
        let manifest = p.parse_manifest()?;
        Ok(PluginResponse {
            uuid: p.uuid,
            name: p.name,
            display_name: p.display_name,
            version: p.version,
            description: p.description,
            manifest,
            state: p.state,
            trust_level: p.trust_level,
            installed_by: p.installed_by,
            installed_at: p.installed_at,
            updated_at: p.updated_at,
            bundle_hash: p.bundle_hash,
            bundle_size: p.bundle_size,
            bundle_uploaded_at: p.bundle_uploaded_at,
            source: p.source,
            consented_permissions: p
                .consented_permissions
                .and_then(|v| serde_json::from_value::<Vec<String>>(v).ok()),
            // Default to None; handlers enrich via a separate
            // revocation map lookup so the conversion stays
            // dependency-free and the bulk list endpoint can resolve
            // every plugin's revocation in a single round-trip.
            signer_revoked_at: None,
        })
    }
}

/// Plugin setting response (hides secret values)
#[derive(Debug, Serialize)]
pub struct PluginSettingResponse {
    pub key: String,
    pub value: Option<serde_json::Value>,
    pub is_secret: bool,
}

impl From<PluginData> for PluginSettingResponse {
    fn from(d: PluginData) -> Self {
        PluginSettingResponse {
            key: d.key,
            // Hide secret values in response
            value: if d.is_secret { None } else { d.value },
            is_secret: d.is_secret,
        }
    }
}

/// Request to set a plugin setting or storage
#[derive(Debug, Deserialize)]
pub struct SetPluginDataRequest {
    pub key: String,
    pub value: serde_json::Value,
}

/// Plugin storage response
#[derive(Debug, Serialize)]
pub struct PluginStorageResponse {
    pub key: String,
    pub value: Option<serde_json::Value>,
}

impl From<PluginData> for PluginStorageResponse {
    fn from(d: PluginData) -> Self {
        PluginStorageResponse {
            key: d.key,
            value: d.value,
        }
    }
}

/// Plugin activity response
#[derive(Debug, Serialize)]
pub struct PluginActivityResponse {
    pub uuid: Uuid,
    pub action: String,
    pub details: Option<serde_json::Value>,
    pub user_uuid: Option<Uuid>,
    pub created_at: NaiveDateTime,
}

impl From<PluginActivity> for PluginActivityResponse {
    fn from(a: PluginActivity) -> Self {
        PluginActivityResponse {
            uuid: a.uuid,
            action: a.action,
            details: a.details,
            user_uuid: a.user_uuid,
            created_at: a.created_at,
        }
    }
}

/// Request for proxied external API calls
#[derive(Debug, Deserialize)]
pub struct PluginProxyRequest {
    pub url: String,
    #[serde(default = "default_method")]
    pub method: String,
    pub headers: Option<std::collections::HashMap<String, String>>,
    pub body: Option<serde_json::Value>,
    /// Body encoding: "json" (default) or "form" (application/x-www-form-urlencoded)
    pub content_type: Option<String>,
}

fn default_method() -> String {
    "GET".to_string()
}

/// Response from proxied external API call
#[derive(Debug, Serialize)]
pub struct PluginProxyResponse {
    pub status: u16,
    pub headers: std::collections::HashMap<String, String>,
    pub body: Option<serde_json::Value>,
}
