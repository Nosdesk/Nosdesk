/**
 * Plugin Types
 * For the plugin system - runtime, API, and UI slots
 */

// =============================================================================
// Plugin Manifest
// =============================================================================

// Mirrors the Rust PluginManifest. The schema is locked behind
// manifest_version: 1; future schema bumps land as a new
// PluginManifestV2 type rather than mutations of this one.
export interface PluginManifest {
  manifest_version: 1;
  name: string;
  displayName: string;
  version: string;
  description?: string;
  license?: string;
  author?: string;
  repository?: string;
  homepage?: string;
  /** Issue tracker URL, distinct from `repository`. */
  bugs?: string;
  /** Email or URL for end-user support. */
  support_contact?: string;
  // Plugin-level icon is NOT stored in the manifest. By convention,
  // plugins ship `icon.svg` at the zip root; the backend extracts
  // it into the `plugins.icon_svg` column and serves it from
  // GET /api/plugins/<uuid>/icon. UI components use that URL.
  engines: PluginEngines;
  /** Other plugins this one requires. Map of plugin name to
   * semver requirement. The install pipeline validates the
   * requirement is well-formed; runtime enforcement of dep
   * presence lands in a future Nosdesk version. */
  dependencies?: Record<string, string>;
  categories?: string[];
  tags?: string[];
  screenshots?: string[];
  permissions: string[];
  components: Record<string, PluginComponentConfig>;
  events: string[];
  settings: PluginSettingDefinition[];
  collections?: Record<string, CollectionDefinition>;
  lifecycle?: PluginLifecyclePolicy;
  /** RESERVED in v1: declared, validated, but the runtime
   * dispatcher hasn't shipped. Plugins must leave these empty
   * until support lands. */
  commands?: PluginCommandDefinition[];
  menus?: Record<string, PluginMenuItem[]>;
  url_handlers?: PluginUrlHandler[];
  /** RESERVED for typed inter-plugin exports. */
  extensions?: unknown;
}

export interface PluginCommandDefinition {
  /** Stable namespaced identifier, e.g. `github.sync`. */
  id: string;
  title: string;
  /** Optional context filter that scopes when the command is
   * available (e.g. `ticket`). */
  when?: string;
}

export interface PluginMenuItem {
  command: string;
  /** Optional grouping hint, e.g. `integrations`. */
  group?: string;
}

export interface PluginUrlHandler {
  /** Glob-like pattern under the plugin's namespace, e.g.
   * `link/*` becomes `nosdesk://plugin/<plugin-name>/link/*`. */
  pattern: string;
  command?: string;
}

export interface PluginEngines {
  /** SemVer requirement against the running Nosdesk version. */
  nosdesk: string;
  /** Plugin runtime API major version. v1 is "1". */
  plugin_api: string;
}

export interface PluginLifecyclePolicy {
  /** What to do with plugin-owned data on uninstall. Default cascade. */
  on_uninstall?: 'cascade' | 'preserve';
}

/** Component "kind". Only `slot` is implemented in v1; the others
 * are reserved so a forward-looking manifest can declare them now
 * (the install pipeline rejects with a clear "kind not yet
 * supported" error). */
export type PluginComponentKind =
  | 'slot'
  | 'settings'
  | 'admin_page'
  | 'worker'
  | 'webhook';

export interface PluginComponentConfig {
  /** Defaults to 'slot' on the wire when unset. */
  kind?: PluginComponentKind;
  slot: PluginSlot;
  entry: string;
  context?: string[];
  label?: string;
  icon?: string;
  action?: {
    label: string;
  };
}

export interface PluginSettingDefinition {
  key: string;
  type:
    | 'string'
    | 'number'
    | 'boolean'
    | 'secret'
    | 'select'
    // Reserved — wire-format-supported, frontend renderer pending:
    | 'multiline_string'
    | 'select_multi'
    | 'json'
    | 'url'
    | 'date';
  label: string;
  description?: string;
  required?: boolean;
  default?: unknown;
  /** Storage scope. `global` (default) means one value per
   * instance; `user` means one value per logged-in user. RESERVED
   * in v1: backend refuses `user` until per-user storage lands. */
  scope?: 'global' | 'user';
  options?: { value: string; label: string }[];
}

// =============================================================================
// Plugin Data Types
// =============================================================================

export type PluginTrustLevel = 'official' | 'verified' | 'community' | 'local';
export type PluginSource = 'provisioned' | 'uploaded' | 'cli' | 'registry';

/** Lifecycle state of a plugin row, mirroring the Rust enum. */
export type PluginState =
  | 'installed'
  | 'disabled'
  | 'quarantined'
  | 'uninstalled';

// =============================================================================
// Registry types (mirror the JSON served by nosdesk.com)
// =============================================================================

export interface RegistryPublisher {
  pubkey: string;
  display_name: string;
  tier: 'verified' | 'community';
  website: string | null;
  added_at: string;
  revoked_at: string | null;
}

export interface RegistryVersion {
  version: string;
  released_at: string;
  download_url: string;
  sha256: string;
  min_nosdesk_version: string | null;
}

export interface RegistryPlugin {
  name: string;
  display_name: string;
  tier: PluginTrustLevel;
  publisher_pubkey: string;
  description: string | null;
  homepage: string | null;
  /** https URL of the plugin's icon SVG. Optional; missing means
   * the registry build didn't pick one up from the source repo. */
  icon_url?: string | null;
  versions: RegistryVersion[];
}

export interface RegistrySnapshot {
  fetched_at: string;
  publishers: {
    version: number;
    generated_at: string;
    publishers: RegistryPublisher[];
  };
  index: {
    version: number;
    generated_at: string;
    plugins: RegistryPlugin[];
  };
}

/**
 * Tagged response shape from `GET /admin/plugins/registry`. The
 * backend always returns 200; the `status` field carries operator
 * intent so the UI can render distinct states for "snapshot
 * ready", "operator opted out", "still syncing", and "sync errored".
 *
 *   - `available`: snapshot is included
 *   - `disabled` : NOSDESK_REGISTRY_URL is empty (operator config)
 *   - `pending`  : boot warm-up; sync hasn't completed this process
 *   - `failed`   : sync attempted and errored, reason included
 */
export type RegistryState =
  | { status: 'available'; snapshot: RegistrySnapshot }
  | { status: 'disabled' }
  | { status: 'pending' }
  | { status: 'failed'; reason: string };

export interface InstallFromRegistryRequest {
  plugin_name: string;
  version?: string;
}

export interface Plugin {
  uuid: string;
  name: string;
  display_name: string;
  version: string;
  description: string | null;
  manifest: PluginManifest;
  /** Lifecycle state. `installed` is active; `disabled` is admin-
   * paused; `quarantined` is a trust-failure parking lot; and
   * `uninstalled` is a row preserved for data attachment after
   * a plugin declared `lifecycle.on_uninstall: "preserve"`. The
   * loader treats only `installed` as a serving-eligible state. */
  state: PluginState;
  trust_level: PluginTrustLevel;
  source: PluginSource;
  installed_by: string | null;
  installed_at: string;
  updated_at: string;
  // Bundle metadata
  bundle_hash: string | null;
  bundle_size: number | null;
  bundle_uploaded_at: string | null;
}

export interface PluginSetting {
  key: string;
  value: unknown | null;
  is_secret: boolean;
}

export interface PluginStorage {
  key: string;
  value: unknown | null;
}

// Consolidated request type for both settings and storage
export interface SetPluginDataRequest {
  key: string;
  value: unknown;
}

export interface PluginActivity {
  uuid: string;
  action: string;
  details: Record<string, unknown> | null;
  user_uuid: string | null;
  created_at: string;
}

// =============================================================================
// Plugin Collections
// =============================================================================

export interface CollectionFieldDefinition {
  type: 'string' | 'number' | 'boolean' | 'date' | 'datetime' | 'uuid' | 'json' | 'reference';
  label?: string;
  required?: boolean;
  reference?: string;
}

export interface CollectionDefinition {
  /** Required in manifest_version 1. Future migrations declare a
   * higher value plus a `migrations` array (not yet implemented). */
  schema_version: 1;
  label?: string;
  fields: Record<string, CollectionFieldDefinition>;
}

export interface CollectionRow {
  uuid: string;
  data: Record<string, unknown>;
  created_by: string | null;
  created_at: string;
  updated_at: string;
}

export interface CollectionListResponse {
  rows: CollectionRow[];
  total: number;
}

export interface CollectionSchemaInfo {
  uuid: string;
  collection_name: string;
  schema: CollectionDefinition;
  version: number;
  row_count: number;
}

// =============================================================================
// API Request/Response Types
// =============================================================================

export interface UpdatePluginRequest {
  enabled?: boolean;
  manifest?: PluginManifest;
}

// Use SetPluginDataRequest for both settings and storage (consolidated backend)
export type SetPluginSettingRequest = SetPluginDataRequest;
export type SetPluginStorageRequest = SetPluginDataRequest;

export interface PluginProxyRequest {
  url: string;
  method?: 'GET' | 'POST' | 'PUT' | 'PATCH' | 'DELETE' | 'HEAD' | 'OPTIONS';
  headers?: Record<string, string>;
  body?: unknown;
  content_type?: 'json' | 'form';
}

export interface PluginProxyResponse {
  status: number;
  headers: Record<string, string>;
  body: unknown | null;
}

// =============================================================================
// Plugin UI Slots
// =============================================================================

export type PluginSlot =
  // Global slots
  | 'navbar-items'
  | 'settings-integrations'
  // Ticket context
  | 'ticket-header-actions'
  | 'ticket-sidebar'
  | 'ticket-tabs'
  | 'ticket-footer-actions'
  // Document context
  | 'document-toolbar'
  | 'document-sidebar'
  // Device context
  | 'device-header-actions'
  | 'device-info-panels';

export const PLUGIN_SLOTS: Record<PluginSlot, { multiple: boolean; description: string }> = {
  // Global slots
  'navbar-items': { multiple: true, description: 'Add items to the navigation bar' },
  'settings-integrations': { multiple: true, description: 'Add pages to Settings > Integrations' },
  // Ticket context
  'ticket-header-actions': { multiple: true, description: 'Add buttons to ticket header' },
  'ticket-sidebar': { multiple: true, description: 'Add panels to ticket sidebar' },
  'ticket-tabs': { multiple: true, description: 'Add tabs to ticket view' },
  'ticket-footer-actions': { multiple: true, description: 'Add buttons to ticket footer' },
  // Document context
  'document-toolbar': { multiple: true, description: 'Add actions to document toolbar' },
  'document-sidebar': { multiple: true, description: 'Add panels to document sidebar' },
  // Device context
  'device-header-actions': { multiple: true, description: 'Add buttons to device header' },
  'device-info-panels': { multiple: true, description: 'Add info panels to device view' },
};

// =============================================================================
// Plugin Permissions
// =============================================================================

// Canonical permission namespace mirrors the backend allowlist
// in `services/plugins/manifest_validate.rs::KNOWN_PERMISSIONS`
// and is enforced at runtime by `hasPermission()` checks in
// `plugins/api.ts`. Singular resource names, `<resource>:<action>`
// shape, plus a `network:<host>` prefix for outbound HTTP claims.
//
// Adding a permission: extend the backend allowlist, add the
// runtime check at the relevant `api.*` method, and add the
// literal to this union. The user-visible label/description
// table below feeds the install confirmation UI.
export type PluginPermission =
  | 'ticket:read'
  | 'ticket:write'
  | 'ticket:comment'
  | 'ticket:delete'
  | 'device:read'
  | 'device:write'
  | 'user:read'
  | 'storage:plugin'
  | 'collection:read'
  | 'collection:write'
  | `network:${string}`;

export const PLUGIN_PERMISSIONS: { value: PluginPermission; label: string; description: string }[] = [
  { value: 'ticket:read',       label: 'Read Tickets',       description: 'Read ticket data' },
  { value: 'ticket:write',      label: 'Write Tickets',      description: 'Create and update tickets' },
  { value: 'ticket:comment',    label: 'Comment on Tickets', description: 'Add comments to tickets' },
  { value: 'ticket:delete',     label: 'Delete Tickets',     description: 'Delete tickets' },
  { value: 'device:read',       label: 'Read Devices',       description: 'Read device data' },
  { value: 'device:write',      label: 'Write Devices',      description: 'Create and update devices' },
  { value: 'user:read',         label: 'Read Users',         description: 'Read user profile data' },
  { value: 'storage:plugin',    label: 'Plugin Storage',     description: 'Store plugin-scoped key-value data' },
  { value: 'collection:read',   label: 'Read Collections',   description: 'Read typed collection rows' },
  { value: 'collection:write',  label: 'Write Collections',  description: 'Create and update typed collection rows' },
];

// =============================================================================
// Plugin Events
// =============================================================================

export const PLUGIN_EVENTS = [
  'ticket:created',
  'ticket:updated',
  'ticket:status_changed',
  'ticket:assigned',
  'ticket:comment_added',
  'document:created',
  'document:updated',
  'device:created',
  'device:updated',
] as const;

export type PluginEvent = (typeof PLUGIN_EVENTS)[number];
