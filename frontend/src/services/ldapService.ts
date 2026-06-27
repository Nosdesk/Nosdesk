/**
 * Admin-side service for the LDAP / Active Directory directory integration.
 *
 * Backs `/admin/ldap`. Every call goes through the authenticated `apiClient`;
 * the endpoints require `role = admin`. The bind password is write-only: the
 * GET never returns it (a `has_bind_password` flag tells the UI whether one is
 * stored), and the PUT sends it only when rotating or clearing it.
 */
import apiClient from '@nosdesk/core/apiClient';

/** The persisted, editable LDAP config (mirrors the backend row, minus the
 *  serde-skipped encrypted columns). */
export interface LdapSettings {
  enabled: boolean;
  host: string;
  port: number;
  tls_mode: 'ldaps' | 'starttls';
  verify_certs: boolean;
  ca_cert_pem: string | null;
  follow_referrals: boolean;
  connect_timeout_secs: number;
  auth_mode: 'simple_bind' | 'mtls';
  bind_dn: string;
  user_base_dn: string;
  username_attribute: string;
  user_filter: string;
  page_size: number;
  attribute_map: Record<string, unknown>;
  group_config: Record<string, unknown>;
  provisioning: Record<string, unknown>;
  workspace_id?: number;
}

/** The editable subset sent on PUT (no workspace_id; it's request-scoped). */
export type UpsertLdapSettings = Omit<LdapSettings, 'workspace_id'>;

export interface LdapSettingsResponse {
  settings: LdapSettings | null;
  has_bind_password: boolean;
}

/** A provider preset for pick-from-catalog (AD / OpenLDAP / FreeIPA / JumpCloud). */
export interface LdapPreset {
  id: string;
  label: string;
  defaults: Partial<UpsertLdapSettings>;
}

export interface TestConnectionResult {
  ok: boolean;
  error?: string;
}

export interface LdapSyncStats {
  seen: number;
  skipped: number;
  synced: number;
  errors: number;
}

export interface LdapSyncResult {
  session_id: number;
  stats: LdapSyncStats;
}

/** A past sync run (a sync_history row), as shown in the admin UI. */
export interface LdapSyncRun {
  id: number;
  sync_type: string; // 'ldap_users' | 'ldap_reconcile'
  status: string; // 'completed' | 'completed_with_errors' | 'failed' | 'running'
  started_at: string;
  completed_at: string | null;
  error_message: string | null;
  records_processed: number | null;
  records_updated: number | null;
  records_failed: number | null;
  is_delta: boolean;
}

export interface LdapSyncHistory {
  runs: LdapSyncRun[];
  cursor: {
    /** True once a DirSync cursor exists, i.e. incremental sync is active. */
    incremental_active: boolean;
    last_full_reconcile_at: string | null;
  };
}

export const ldapService = {
  async getSettings(): Promise<LdapSettingsResponse> {
    return (await apiClient.get<LdapSettingsResponse>('/ldap/settings')).data;
  },

  /** Persist the config. `bindPassword` rotates the stored secret only when
   *  non-empty; `clearBindPassword` removes it (and wins over a rotation). */
  async updateSettings(
    settings: UpsertLdapSettings,
    bindPassword?: string,
    clearBindPassword = false,
  ): Promise<LdapSettingsResponse> {
    const body = {
      settings,
      bind_password: bindPassword && bindPassword.length > 0 ? bindPassword : undefined,
      clear_bind_password: clearBindPassword,
    };
    return (await apiClient.put<LdapSettingsResponse>('/ldap/settings', body)).data;
  },

  async getPresets(): Promise<LdapPreset[]> {
    return (await apiClient.get<LdapPreset[]>('/ldap/presets')).data;
  },

  /** Connect + service-bind against the SAVED config. */
  async testConnection(): Promise<TestConnectionResult> {
    return (await apiClient.post<TestConnectionResult>('/ldap/test-connection')).data;
  },

  /** Run a full sync now (synchronous; returns the run stats). */
  async runSync(): Promise<LdapSyncResult> {
    return (await apiClient.post<LdapSyncResult>('/ldap/sync')).data;
  },

  /** Recent sync runs + cursor state. */
  async getSyncHistory(): Promise<LdapSyncHistory> {
    return (await apiClient.get<LdapSyncHistory>('/ldap/sync-history')).data;
  },

  /** Browse directory groups under the saved group base DN (role-rule picker). */
  async discoverGroups(): Promise<DiscoverGroupsResult> {
    return (await apiClient.get<DiscoverGroupsResult>('/ldap/discover-groups')).data;
  },

  /** Blast-radius preview for the saved config (read-only). */
  async previewSync(): Promise<PreviewResult> {
    return (await apiClient.post<PreviewResult>('/ldap/preview')).data;
  },
};

export interface RolePreviewRule {
  group: string;
  role: string;
  found: boolean;
  member_count: number;
  member_capped: boolean;
}

export interface RolePreview {
  user_count: number;
  user_capped: boolean;
  rules: RolePreviewRule[];
}

export interface PreviewResult {
  ok: boolean;
  preview?: RolePreview;
  error?: string;
}

export interface DiscoveredGroup {
  name: string;
  dn: string;
  external_id: string | null;
}

export interface DiscoverGroupsResult {
  ok: boolean;
  groups: DiscoveredGroup[];
  error?: string;
}

/** A single group->role rule, stored in group_config.role_mappings. */
export interface RoleMapping {
  group: string;
  role: 'member' | 'agent' | 'admin';
}

export default ldapService;
