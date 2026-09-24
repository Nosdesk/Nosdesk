import apiClient from '../apiClient';

export interface DkimRecord {
  name: string;
  txt_value: string;
}

export interface OutboundSettings {
  sending_mode: string;
  from_name: string;
  from_email: string;
  sending_domain: string | null;
  verification_status: string;
  verified_at: string | null;
  dkim_record: DkimRecord | null;
  /** The workspace's own SMTP server, kept across mode changes. */
  smtp_host: string;
  smtp_port: number;
  smtp_security: 'starttls' | 'tls' | 'plaintext';
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

/// Admin API for a workspace's verified sending domain (DKIM via the instance
/// relay). Mirrors the `/admin/email/outbound` endpoints.
export default {
  async get(): Promise<OutboundSettings> {
    const response = await apiClient.get<OutboundSettings>('/admin/email/outbound');
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

  async reset(): Promise<void> {
    await apiClient.delete('/admin/email/outbound');
  },
};
