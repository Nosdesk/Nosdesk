/**
 * Plugin Types
 * For the plugin system - runtime, API, and UI slots
 */

// =============================================================================
// Plugin Manifest
// =============================================================================

export interface PluginManifest {
  name: string;
  displayName: string;
  version: string;
  description?: string;
  permissions: string[];
  components: Record<string, PluginComponentConfig>;
  events: string[];
  settings: PluginSettingDefinition[];
}

export interface PluginComponentConfig {
  slot: PluginSlot;
  entry: string;
  context?: string[];
  label?: string;
  icon?: string;
}

export interface PluginSettingDefinition {
  key: string;
  type: 'string' | 'number' | 'boolean' | 'secret' | 'select';
  label: string;
  description?: string;
  required?: boolean;
  default?: unknown;
  options?: { value: string; label: string }[];
}

// =============================================================================
// Plugin Data Types
// =============================================================================

export type PluginTrustLevel = 'official' | 'verified' | 'community';
export type PluginSource = 'provisioned' | 'uploaded';

export interface Plugin {
  uuid: string;
  name: string;
  display_name: string;
  version: string;
  description: string | null;
  manifest: PluginManifest;
  enabled: boolean;
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
// API Request/Response Types
// =============================================================================

export interface InstallPluginRequest {
  manifest: PluginManifest;
  trust_level?: PluginTrustLevel;
}

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

export type PluginPermission =
  | 'tickets:read'
  | 'tickets:comment'
  | 'tickets:link'
  | 'devices:read'
  | 'documents:read'
  | 'storage'
  | `external:${string}`;

export const PLUGIN_PERMISSIONS: { value: PluginPermission; label: string; description: string }[] = [
  { value: 'tickets:read', label: 'Read Tickets', description: 'Read ticket data' },
  { value: 'tickets:comment', label: 'Comment on Tickets', description: 'Add comments to tickets' },
  { value: 'tickets:link', label: 'Link Tickets', description: 'Add external links to tickets' },
  { value: 'devices:read', label: 'Read Devices', description: 'Read device data' },
  { value: 'documents:read', label: 'Read Documents', description: 'Read document data' },
  { value: 'storage', label: 'Plugin Storage', description: 'Store plugin-specific data' },
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
