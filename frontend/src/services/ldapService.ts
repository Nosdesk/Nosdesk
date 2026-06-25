/**
 * Admin-side service for the LDAP / Active Directory directory integration.
 *
 * Backs `/admin/ldap`. Every call goes through the authenticated `apiClient`;
 * the endpoints require `role = admin`. The bind password is write-only: the
 * GET never returns it (a `has_bind_password` flag tells the UI whether one is
 * stored), and the PUT sends it only when rotating or clearing it.
 */
import apiClient from './apiConfig';

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
};

export default ldapService;
