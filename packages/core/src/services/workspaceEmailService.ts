import apiClient from '../apiClient';

export interface DkimRecord {
  name: string;
  txt_value: string;
}

export type SendingMode = 'fallback' | 'verified_domain' | 'smtp_relay';
export type SmtpSecurity = 'starttls' | 'tls' | 'plaintext';

/** The server's own SMTP settings (environment), what `fallback` sends with.
 *  Hosted (`managed`) withholds the relay and reports the effective From. */
export interface ServerEmailConfig {
  provider?: 'smtp';
  managed?: boolean;
  /** Hosted only: which identity the workspace's mail actually uses. */
  mode?: 'managed' | 'verified_domain' | 'smtp_relay' | 'platform';
  smtp_host?: string;
  smtp_port?: number;
  smtp_password_configured?: boolean;
  from_name: string;
  from_email: string;
  enabled: boolean;
  is_configured: boolean;
  error?: string;
}

/** The own-server form, saved or tested. A blank password keeps the stored one. */
export interface RelaySettings {
  from_name: string;
  from_email: string;
  smtp_host: string;
  smtp_port: number;
  smtp_security: SmtpSecurity;
  smtp_username: string;
  password?: string;
}

export interface OutboundSettings {
  sending_mode: SendingMode;
  from_name: string;
  from_email: string;
  sending_domain: string | null;
  verification_status: string;
  verified_at: string | null;
  dkim_record: DkimRecord | null;
  /** The workspace's own SMTP server, kept across mode changes. */
  smtp_host: string;
  smtp_port: number;
  smtp_security: SmtpSecurity;
  smtp_username: string;
  /** A password is stored; it is never returned. */
  password_configured: boolean;
  port_security: { level: 'ok' | 'warn' | 'error'; message: string | null };
}

/** The result of a test send. `code` says which step failed. */
export interface EmailTestResult {
  ok: boolean;
  /** Where the test went: the requesting admin's own address. */
  to: string;
  code:
    | 'incomplete'
    | 'invalid'
    | 'dns'
    | 'egress_blocked'
    | 'auth'
    | 'rejected'
    | 'timeout'
    | 'tls'
    | 'connect'
    | null;
  /** The server's own words, for the detail line. */
  detail: string | null;
}

export interface SetDomainResponse {
  dkim_record: DkimRecord;
  verification_status: string;
}

export type CheckStatus = 'pass' | 'warn' | 'fail' | 'info';

export interface RecordCheck {
  status: CheckStatus;
  summary: string;
  value: string | null;
}

export interface EmailAuthReport {
  domain: string;
  spf: RecordCheck;
  dkim: RecordCheck;
  dmarc: RecordCheck;
  mx: RecordCheck;
}

/// Admin API for how a workspace sends: the server default, a verified
/// domain, or its own SMTP server. Mirrors `/admin/email/outbound`.
export default {
  async get(): Promise<OutboundSettings> {
    const response = await apiClient.get<OutboundSettings>('/admin/email/outbound');
    return response.data;
  },

  async getServerConfig(): Promise<ServerEmailConfig> {
    const response = await apiClient.get<ServerEmailConfig>('/admin/email/config');
    return response.data;
  },

  /** Send with an identity that is already saved; nothing is cleared. */
  async setMode(mode: SendingMode): Promise<OutboundSettings> {
    const response = await apiClient.put<OutboundSettings>('/admin/email/outbound/mode', { mode });
    return response.data;
  },

  /** Save the workspace's own SMTP server and send through it. */
  async saveRelay(relay: RelaySettings): Promise<OutboundSettings> {
    const response = await apiClient.put<OutboundSettings>('/admin/email/outbound/relay', relay);
    return response.data;
  },

  /** Try unsaved relay settings by sending to the requesting admin. */
  async testRelay(relay: RelaySettings): Promise<EmailTestResult> {
    const response = await apiClient.post<EmailTestResult>(
      '/admin/email/outbound/relay/test',
      relay,
    );
    return response.data;
  },

  async removeRelayPassword(): Promise<OutboundSettings> {
    const response = await apiClient.delete<OutboundSettings>(
      '/admin/email/outbound/relay/password',
    );
    return response.data;
  },

  async setDomain(payload: { from_name: string; from_email: string }): Promise<SetDomainResponse> {
    const response = await apiClient.put<SetDomainResponse>(
      '/admin/email/outbound/domain',
      payload,
    );
    return response.data;
  },

  async verify(): Promise<{ verification_status: string }> {
    const response = await apiClient.post<{ verification_status: string }>(
      '/admin/email/outbound/verify',
      {},
    );
    return response.data;
  },

  async dnsCheck(): Promise<EmailAuthReport> {
    const response = await apiClient.get<EmailAuthReport>('/admin/email/outbound/dns-check');
    return response.data;
  },

  /** Test whatever this workspace sends with now, to the caller. */
  async sendTest(): Promise<EmailTestResult> {
    const response = await apiClient.post<EmailTestResult>('/admin/email/outbound/test', {});
    return response.data;
  },

  /** Remove the sending domain (its DKIM key too) and use the server default. */
  async reset(): Promise<void> {
    await apiClient.delete('/admin/email/outbound');
  },
};
